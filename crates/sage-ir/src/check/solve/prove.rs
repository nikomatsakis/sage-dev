use std::cell::RefCell;
use std::future::Future;
use std::pin::Pin;
use std::rc::Rc;
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll, Wake, Waker};

use rustc_hash::FxHashMap;
use sage_stash::{Ptr, Slice, StashCopy, Stashed};

use crate::check::infer::runtime::{ReadyQueue, ScopedTasks};
use crate::check::infer::unify::{UnifyError, try_unify, unify_in_probe};
use crate::check::infer::version::Version;
use crate::generic_param::{GenericParam, GenericParamKind};
use crate::ty::{TraitRef, Ty, WherePredicate};

use super::boundary::{
    AppliedCertainty, IrCopier, QueryProofState, apply_query_response, extract_query_result,
    extract_query_result_at, extract_query_result_with_output_at, instantiate_query,
};
use super::canonical::canonicalize_goal;
use super::clauses::{
    Candidate, assemble_candidates, instantiate_candidate, load_prepared_associated_value,
    prepare_value_candidate,
};
use super::merge::merge_candidate_results;
use super::{Assumption, Atom, Goal, GoalQueryData, GoalResult, MAX_PROOF_DEPTH, QueryResult};
use super::{GoalOutput, SolverGoal};

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
struct FrameId(u32);

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
struct SubscriptionId(u32);

type ProducerFuture<'db> =
    Pin<Box<dyn Future<Output = (FrameId, Stashed<QueryResult<'db>>)> + 'db>>;

enum FrameState<'db> {
    Pending,
    Ready(Stashed<QueryResult<'db>>),
    Abandoned,
}

struct ProofFrame<'db> {
    query: Stashed<GoalQueryData<'db>>,
    parent: Option<FrameId>,
    remaining_depth: u32,
    state: FrameState<'db>,
    producer_started: bool,
    subscriptions: FxHashMap<SubscriptionId, Option<Waker>>,
}

struct ProducerWake {
    frame: FrameId,
    ready: Arc<Mutex<ReadyQueue<FrameId>>>,
}

impl Wake for ProducerWake {
    fn wake(self: Arc<Self>) {
        self.ready.lock().unwrap().enqueue(self.frame);
    }

    fn wake_by_ref(self: &Arc<Self>) {
        self.ready.lock().unwrap().enqueue(self.frame);
    }
}

struct RootWake(Arc<Mutex<bool>>);

impl Wake for RootWake {
    fn wake(self: Arc<Self>) {
        *self.0.lock().unwrap() = true;
    }

    fn wake_by_ref(self: &Arc<Self>) {
        *self.0.lock().unwrap() = true;
    }
}

struct WhiteboardInner<'db> {
    next_subscription: u32,
    frames: Vec<ProofFrame<'db>>,
    entries: FxHashMap<(Stashed<GoalQueryData<'db>>, Option<FrameId>, u32), FrameId>,
    producers: Vec<Option<ProducerFuture<'db>>>,
    ready: Arc<Mutex<ReadyQueue<FrameId>>>,
}

#[derive(Clone)]
struct Whiteboard<'db> {
    inner: Rc<RefCell<WhiteboardInner<'db>>>,
}

impl<'db> Default for Whiteboard<'db> {
    fn default() -> Self {
        Self {
            inner: Rc::new(RefCell::new(WhiteboardInner {
                next_subscription: 0,
                frames: Vec::new(),
                entries: FxHashMap::default(),
                producers: Vec::new(),
                ready: Arc::new(Mutex::new(ReadyQueue::default())),
            })),
        }
    }
}

impl<'db> Whiteboard<'db> {
    fn request(
        &self,
        db: &'db dyn crate::Db,
        query: Stashed<GoalQueryData<'db>>,
        parent: Option<FrameId>,
        depth: u32,
    ) -> ProofFuture<'db> {
        let remaining_depth = MAX_PROOF_DEPTH - depth;
        // ANCHOR: example_whiteboard_lookup
        let mut ancestor = parent;
        while let Some(frame) = ancestor {
            let inner = self.inner.borrow();
            let data = &inner.frames[frame.0 as usize];
            if data.query == query {
                return ProofFuture::immediate(no_query_result());
            }
            ancestor = data.parent;
        }

        let key = (query.clone(), parent, remaining_depth);
        {
            let inner = self.inner.borrow();
            if let Some(frame) = inner.entries.get(&key) {
                match &inner.frames[frame.0 as usize].state {
                    FrameState::Ready(result) => {
                        return ProofFuture::immediate(result.clone());
                    }
                    FrameState::Pending => {}
                    FrameState::Abandoned => {
                        panic!("abandoned whiteboard frame retained a lookup key")
                    }
                }
            }
        }
        // ANCHOR_END: example_whiteboard_lookup
        let (frame, subscription, start) = {
            let mut inner = self.inner.borrow_mut();
            let frame = if let Some(frame) = inner.entries.get(&key) {
                *frame
            } else {
                let frame = FrameId(inner.frames.len() as u32);
                inner.frames.push(ProofFrame {
                    query: query.clone(),
                    parent,
                    remaining_depth,
                    state: FrameState::Pending,
                    producer_started: false,
                    subscriptions: FxHashMap::default(),
                });
                inner.producers.push(None);
                inner.entries.insert(key, frame);
                frame
            };
            let subscription = SubscriptionId(inner.next_subscription);
            inner.next_subscription += 1;
            assert!(
                inner.frames[frame.0 as usize]
                    .subscriptions
                    .insert(subscription, None)
                    .is_none()
            );
            let start = !inner.frames[frame.0 as usize].producer_started;
            if start {
                inner.frames[frame.0 as usize].producer_started = true;
            }
            (frame, subscription, start)
        };

        if start {
            let whiteboard = self.clone();
            let producer_query = query;
            let producer = Box::pin(async move {
                let result =
                    solve_atomic_frame(db, &whiteboard, &producer_query, frame, depth + 1).await;
                (frame, result)
            });
            let mut inner = self.inner.borrow_mut();
            assert!(
                inner.producers[frame.0 as usize]
                    .replace(producer)
                    .is_none()
            );
            inner.ready.lock().unwrap().enqueue(frame);
        }

        ProofFuture {
            whiteboard: Some(self.clone()),
            frame,
            subscription,
            immediate: None,
        }
    }

    fn drive<F: Future>(&self, future: F) -> F::Output {
        let mut root = Box::pin(future);
        let root_ready = Arc::new(Mutex::new(true));
        let root_waker = Waker::from(Arc::new(RootWake(root_ready.clone())));
        loop {
            if std::mem::take(&mut *root_ready.lock().unwrap()) {
                let mut context = Context::from_waker(&root_waker);
                if let Poll::Ready(output) = root.as_mut().poll(&mut context) {
                    self.assert_drained();
                    return output;
                }
            }

            let mut made_progress = false;
            while let Some(frame) = {
                let ready = self.inner.borrow().ready.clone();
                ready.lock().unwrap().pop()
            } {
                let producer = {
                    let mut inner = self.inner.borrow_mut();
                    if !matches!(inner.frames[frame.0 as usize].state, FrameState::Pending) {
                        continue;
                    }
                    inner.producers[frame.0 as usize].take()
                };
                let Some(mut producer) = producer else {
                    continue;
                };
                made_progress = true;
                let ready = self.inner.borrow().ready.clone();
                let waker = Waker::from(Arc::new(ProducerWake { frame, ready }));
                let mut context = Context::from_waker(&waker);
                match producer.as_mut().poll(&mut context) {
                    Poll::Ready((completed, result)) => {
                        assert_eq!(completed, frame);
                        drop(producer);
                        self.complete(frame, result);
                    }
                    Poll::Pending => {
                        let mut inner = self.inner.borrow_mut();
                        if matches!(inner.frames[frame.0 as usize].state, FrameState::Pending) {
                            assert!(
                                inner.producers[frame.0 as usize]
                                    .replace(producer)
                                    .is_none()
                            );
                        }
                    }
                }
            }

            if !*root_ready.lock().unwrap()
                && self.inner.borrow().ready.lock().unwrap().is_empty()
                && !made_progress
            {
                panic!("trait solver deadlock: pending proof with no runnable producer");
            }
        }
    }

    fn complete(&self, frame: FrameId, result: Stashed<QueryResult<'db>>) {
        let (ready, wakers) = {
            let mut inner = self.inner.borrow_mut();
            let ready = inner.ready.clone();
            let data = &mut inner.frames[frame.0 as usize];
            assert!(
                matches!(data.state, FrameState::Pending),
                "double frame completion"
            );
            data.state = FrameState::Ready(result);
            let wakers = data
                .subscriptions
                .drain()
                .filter_map(|(_, waker)| waker)
                .collect::<Vec<_>>();
            (ready, wakers)
        };
        ready.lock().unwrap().remove(frame);
        for waker in wakers {
            waker.wake();
        }
    }

    fn unsubscribe(&self, frame: FrameId, subscription: SubscriptionId) {
        let (ready, producer) = {
            let mut inner = self.inner.borrow_mut();
            let ready = inner.ready.clone();
            let data = &mut inner.frames[frame.0 as usize];
            data.subscriptions.remove(&subscription);
            if matches!(data.state, FrameState::Pending) && data.subscriptions.is_empty() {
                let key = (data.query.clone(), data.parent, data.remaining_depth);
                inner.entries.remove(&key);
                inner.frames[frame.0 as usize].state = FrameState::Abandoned;
                (ready, inner.producers[frame.0 as usize].take())
            } else {
                (ready, None)
            }
        };
        if producer.is_some() {
            ready.lock().unwrap().remove(frame);
        }
        drop(producer);
    }

    fn assert_drained(&self) {
        let inner = self.inner.borrow();
        assert!(
            inner
                .frames
                .iter()
                .all(|frame| !matches!(frame.state, FrameState::Pending)),
            "query completed with a pending whiteboard producer"
        );
        assert!(
            inner.producers.iter().all(Option::is_none),
            "query completed with a live whiteboard producer"
        );
        assert!(
            inner
                .frames
                .iter()
                .all(|frame| frame.subscriptions.is_empty()),
            "query completed with a live whiteboard subscription"
        );
        assert!(
            inner.ready.lock().unwrap().is_empty(),
            "query completed with queued producer wakeups"
        );
    }
}

struct ProofFuture<'db> {
    whiteboard: Option<Whiteboard<'db>>,
    frame: FrameId,
    subscription: SubscriptionId,
    immediate: Option<Stashed<QueryResult<'db>>>,
}

impl<'db> ProofFuture<'db> {
    fn immediate(result: Stashed<QueryResult<'db>>) -> Self {
        Self {
            whiteboard: None,
            frame: FrameId(u32::MAX),
            subscription: SubscriptionId(u32::MAX),
            immediate: Some(result),
        }
    }
}

impl<'db> Future for ProofFuture<'db> {
    type Output = Stashed<QueryResult<'db>>;

    fn poll(mut self: Pin<&mut Self>, context: &mut Context<'_>) -> Poll<Self::Output> {
        if let Some(result) = self.immediate.take() {
            return Poll::Ready(result);
        }
        let whiteboard = self.whiteboard.as_ref().expect("missing whiteboard");
        let result = {
            let mut inner = whiteboard.inner.borrow_mut();
            let frame = &mut inner.frames[self.frame.0 as usize];
            match &frame.state {
                FrameState::Ready(result) => Some(result.clone()),
                FrameState::Pending => {
                    frame
                        .subscriptions
                        .insert(self.subscription, Some(context.waker().clone()));
                    None
                }
                FrameState::Abandoned => panic!("polled an abandoned proof frame"),
            }
        };
        if let Some(result) = result {
            self.whiteboard = None;
            Poll::Ready(result)
        } else {
            Poll::Pending
        }
    }
}

impl Drop for ProofFuture<'_> {
    fn drop(&mut self) {
        if let Some(whiteboard) = self.whiteboard.take() {
            whiteboard.unsubscribe(self.frame, self.subscription);
        }
    }
}

fn no_query_result<'db>() -> Stashed<QueryResult<'db>> {
    let mut stash = sage_stash::Stash::new();
    let bound_vars = stash.alloc_slice(&[]);
    Stashed::new(
        stash,
        QueryResult {
            bound_vars,
            value: super::QueryResultData::No,
        },
    )
}

// ANCHOR: example_prove_query
pub(crate) fn prove_query<'db>(
    db: &'db dyn crate::Db,
    query: &Stashed<GoalQueryData<'db>>,
) -> Stashed<QueryResult<'db>> {
    assert!(matches!(query.root().goal, SolverGoal::Prove(_)));
    solve_query(db, query)
}

pub(crate) fn solve_query<'db>(
    db: &'db dyn crate::Db,
    query: &Stashed<GoalQueryData<'db>>,
) -> Stashed<QueryResult<'db>> {
    super::goal::validate_goal_query(db, query)
        .unwrap_or_else(|error| panic!("invalid canonical goal query: {error}"));
    let mut state = instantiate_query(query);
    let whiteboard = Whiteboard::default();
    if matches!(state.goal, SolverGoal::Normalize(_)) {
        return whiteboard.drive(whiteboard.request(db, query.clone(), None, 0));
    }
    let version = state.version;
    let assumptions = state.assumptions;
    let SolverGoal::Prove(goal) = state.goal else {
        unreachable!()
    };
    let result = whiteboard.drive(prove_goal(
        db,
        &whiteboard,
        &mut state,
        version,
        assumptions,
        goal,
        0,
        None,
    ));
    assert!(
        state.egraph.version_tree().is_leaf(version),
        "proof query completed with live candidate/frame branches"
    );
    extract_query_result(db, &state, result)
}
// ANCHOR_END: example_prove_query

fn prove_goal<'a, 'db: 'a>(
    db: &'db dyn crate::Db,
    whiteboard: &'a Whiteboard<'db>,
    state: &'a mut QueryProofState<'db>,
    version: Version,
    environment: Slice<Assumption<'db>>,
    goal: Goal<'db>,
    depth: u32,
    parent_frame: Option<FrameId>,
) -> Pin<Box<dyn Future<Output = GoalResult<'db>> + 'a>> {
    Box::pin(async move {
        match goal {
            Goal::Atom(atom) => {
                prove_atom(
                    db,
                    whiteboard,
                    state,
                    version,
                    environment,
                    atom,
                    depth,
                    parent_frame,
                )
                .await
            }
            Goal::All(goals) => {
                prove_conjunction(
                    db,
                    whiteboard,
                    state,
                    version,
                    environment,
                    goals,
                    depth,
                    parent_frame,
                )
                .await
            }
            Goal::Exists(binder) => {
                prove_exists(
                    db,
                    whiteboard,
                    state,
                    version,
                    environment,
                    binder,
                    depth,
                    parent_frame,
                )
                .await
            }
            Goal::Implies(assumptions, inner) => {
                let mut combined = state.stash[environment].to_vec();
                combined.extend_from_slice(&state.stash[assumptions]);
                let combined = Assumption::flatten(&mut state.stash, combined);
                let inner = state.stash[inner];
                match prove_goal(
                    db,
                    whiteboard,
                    state,
                    version,
                    combined,
                    inner,
                    depth,
                    parent_frame,
                )
                .await
                {
                    GoalResult::Yes { modulo } if modulo.is_trivially_true(&state.stash) => {
                        GoalResult::Yes { modulo }
                    }
                    GoalResult::Yes { modulo } => {
                        let modulo = state.stash.alloc(modulo);
                        GoalResult::Yes {
                            modulo: Goal::Implies(assumptions, modulo),
                        }
                    }
                    other => other,
                }
            }
            Goal::Maybe => GoalResult::Maybe,
        }
    })
}

// ANCHOR: example_prove_atom
async fn prove_atom<'db>(
    db: &'db dyn crate::Db,
    whiteboard: &Whiteboard<'db>,
    state: &mut QueryProofState<'db>,
    version: Version,
    environment: Slice<Assumption<'db>>,
    atom: Atom<'db>,
    depth: u32,
    parent_frame: Option<FrameId>,
) -> GoalResult<'db> {
    if depth >= MAX_PROOF_DEPTH {
        return GoalResult::Maybe;
    }
    let canonical = canonicalize_goal(
        db,
        &state.stash,
        &state.egraph,
        version,
        state.local_crate,
        state.canonical_universe,
        state.assumptions_complete,
        environment,
        Goal::Atom(atom),
    );
    let response = whiteboard
        .request(db, canonical.data.clone(), parent_frame, depth)
        .await;

    match apply_query_response(
        db,
        &mut state.stash,
        &mut state.egraph,
        version,
        &canonical.data,
        &canonical.mapping,
        &response,
    ) {
        Ok(applied) => match applied.certainty {
            AppliedCertainty::Yes {
                output: GoalOutput::Proven,
                modulo,
            } => GoalResult::Yes { modulo },
            AppliedCertainty::Yes {
                output: GoalOutput::Type(_),
                ..
            } => unreachable!("proof operation returned a type output"),
            AppliedCertainty::Maybe => GoalResult::Maybe,
            AppliedCertainty::No => GoalResult::No,
        },
        Err(_) => GoalResult::No,
    }
}
// ANCHOR_END: example_prove_atom

async fn solve_atomic_frame<'db>(
    db: &'db dyn crate::Db,
    whiteboard: &Whiteboard<'db>,
    query: &Stashed<GoalQueryData<'db>>,
    frame_id: FrameId,
    depth: u32,
) -> Stashed<QueryResult<'db>> {
    let mut frame = instantiate_query(query);
    let SolverGoal::Prove(goal) = frame.goal else {
        let SolverGoal::Normalize(alias) = frame.goal else {
            unreachable!()
        };
        return solve_normalize_frame(db, whiteboard, query, &mut frame, alias, frame_id, depth)
            .await;
    };
    let Goal::Atom(atom) = goal else {
        return extract_query_result(db, &frame, GoalResult::Maybe);
    };
    match atom {
        Atom::Equals(left, right) => {
            let result = match try_unify(
                &mut frame.egraph,
                &mut frame.stash,
                frame.version,
                left,
                right,
            ) {
                Ok(_) | Err(UnifyError::Reported(_)) => GoalResult::Yes {
                    modulo: Goal::true_(&mut frame.stash),
                },
                Err(_) => GoalResult::No,
            };
            extract_query_result(db, &frame, result)
        }
        Atom::TraitImpl { self_ty, trait_ref } => {
            if trait_atom_contains_error(&frame, self_ty, trait_ref) {
                let modulo = Goal::true_(&mut frame.stash);
                return extract_query_result(db, &frame, GoalResult::Yes { modulo });
            }
            solve_trait_frame(db, whiteboard, query, &mut frame, atom, frame_id, depth).await
        }
    }
}

async fn solve_normalize_frame<'db>(
    db: &'db dyn crate::Db,
    whiteboard: &Whiteboard<'db>,
    query: &Stashed<GoalQueryData<'db>>,
    frame: &mut QueryProofState<'db>,
    alias: crate::ty::AliasTy<'db>,
    frame_id: FrameId,
    depth: u32,
) -> Stashed<QueryResult<'db>> {
    let crate::ty::AliasTy::Associated(projection) = alias else {
        return extract_query_result(db, frame, GoalResult::Maybe);
    };
    if !frame.stash[projection.args].is_empty() {
        return extract_query_result(db, frame, GoalResult::Maybe);
    }
    let atom = Atom::TraitImpl {
        self_ty: projection.self_ty,
        trait_ref: projection.trait_ref,
    };
    let (all_candidates, incomplete) =
        assemble_candidates(db, frame, frame.version, frame.assumptions, atom);
    let environment_candidate_count = all_candidates
        .iter()
        .filter(|candidate| matches!(candidate, Candidate::Environment { .. }))
        .count();
    let candidates: Vec<_> = all_candidates
        .into_iter()
        .filter(|candidate| {
            matches!(
                candidate,
                Candidate::LocalImpl(_) | Candidate::ExternalImpl(_)
            )
        })
        .collect();
    let normalization_fact_count = normalization_facts(frame, projection).len();
    let input_universes = input_universes(frame);
    let next_response_param = frame.next_response_param;

    let mut tasks = ScopedTasks::new();
    for candidate_index in 0..candidates.len() {
        let query = query.clone();
        let whiteboard = whiteboard.clone();
        tasks.spawn(async move {
            let mut candidate_state = instantiate_query(&query);
            let SolverGoal::Normalize(crate::ty::AliasTy::Associated(projection)) =
                candidate_state.goal
            else {
                unreachable!()
            };
            let atom = Atom::TraitImpl {
                self_ty: projection.self_ty,
                trait_ref: projection.trait_ref,
            };
            let (all_candidates, _) = assemble_candidates(
                db,
                &candidate_state,
                candidate_state.version,
                candidate_state.assumptions,
                atom,
            );
            let candidates: Vec<_> = all_candidates
                .into_iter()
                .filter(|candidate| {
                    matches!(
                        candidate,
                        Candidate::LocalImpl(_) | Candidate::ExternalImpl(_)
                    )
                })
                .collect();
            let candidate = candidates[candidate_index];
            let parent = candidate_state.version;
            let child = candidate_state.egraph.branch_from(parent);
            let Some(candidate) =
                prepare_value_candidate(db, &mut candidate_state, child, candidate)
            else {
                let answer =
                    extract_query_result_at(db, &candidate_state, child, GoalResult::Maybe);
                candidate_state.egraph.discard(child);
                return answer;
            };
            let result = match match_candidate_head(
                &mut candidate_state,
                child,
                atom,
                candidate.instantiated.head,
            ) {
                Ok(()) => {
                    let body_goals = candidate_state.stash[candidate.instantiated.body].to_vec();
                    let body = Goal::all(&mut candidate_state.stash, body_goals);
                    let environment = candidate_state.assumptions;
                    prove_goal(
                        db,
                        &whiteboard,
                        &mut candidate_state,
                        child,
                        environment,
                        body,
                        depth,
                        Some(frame_id),
                    )
                    .await
                }
                Err(_) => GoalResult::No,
            };
            let answer = match result {
                GoalResult::Yes { .. } => {
                    let Some(value) = load_prepared_associated_value(
                        db,
                        &mut candidate_state,
                        &candidate,
                        projection.associated_ty,
                    ) else {
                        let answer =
                            extract_query_result_at(db, &candidate_state, child, GoalResult::Maybe);
                        candidate_state.egraph.discard(child);
                        return answer;
                    };
                    extract_query_result_with_output_at(
                        db,
                        &candidate_state,
                        child,
                        GoalOutput::Type(value),
                        result,
                    )
                }
                GoalResult::Maybe | GoalResult::No => {
                    extract_query_result_at(db, &candidate_state, child, result)
                }
            };
            candidate_state.egraph.discard(child);
            answer
        });
    }
    for candidate_index in 0..environment_candidate_count {
        let query = query.clone();
        let whiteboard = whiteboard.clone();
        tasks.spawn(async move {
            let mut candidate_state = instantiate_query(&query);
            let SolverGoal::Normalize(crate::ty::AliasTy::Associated(projection)) =
                candidate_state.goal
            else {
                unreachable!()
            };
            let atom = Atom::TraitImpl {
                self_ty: projection.self_ty,
                trait_ref: projection.trait_ref,
            };
            let (all_candidates, _) = assemble_candidates(
                db,
                &candidate_state,
                candidate_state.version,
                candidate_state.assumptions,
                atom,
            );
            let candidates: Vec<_> = all_candidates
                .into_iter()
                .filter(|candidate| matches!(candidate, Candidate::Environment { .. }))
                .collect();
            let parent = candidate_state.version;
            let child = candidate_state.egraph.branch_from(parent);
            let candidate =
                instantiate_candidate(db, &mut candidate_state, child, candidates[candidate_index]);
            let result =
                match match_candidate_head(&mut candidate_state, child, atom, candidate.head) {
                    Ok(()) => {
                        let body_goals = candidate_state.stash[candidate.body].to_vec();
                        let body = Goal::all(&mut candidate_state.stash, body_goals);
                        let environment = candidate_state.assumptions;
                        prove_goal(
                            db,
                            &whiteboard,
                            &mut candidate_state,
                            child,
                            environment,
                            body,
                            depth,
                            Some(frame_id),
                        )
                        .await
                    }
                    Err(_) => GoalResult::No,
                };
            // A matching trait fact establishes only truth. Even when all of
            // its conditions hold, it contributes uncertainty rather than an
            // invented associated value.
            let result = match result {
                GoalResult::No => GoalResult::No,
                GoalResult::Yes { .. } | GoalResult::Maybe => GoalResult::Maybe,
            };
            let answer = extract_query_result_at(db, &candidate_state, child, result);
            candidate_state.egraph.discard(child);
            answer
        });
    }
    for fact_index in 0..normalization_fact_count {
        let query = query.clone();
        tasks.spawn(async move {
            let mut candidate_state = instantiate_query(&query);
            let SolverGoal::Normalize(crate::ty::AliasTy::Associated(projection)) =
                candidate_state.goal
            else {
                unreachable!()
            };
            let facts = normalization_facts(&candidate_state, projection);
            let (fact_alias, fact_value) = facts[fact_index];
            let parent = candidate_state.version;
            let child = candidate_state.egraph.branch_from(parent);
            let query_alias = candidate_state
                .stash
                .alloc(Ty::Alias(crate::ty::AliasTy::Associated(projection)));
            let fact_alias = candidate_state.stash.alloc(Ty::Alias(fact_alias));
            let result = if try_unify(
                &mut candidate_state.egraph,
                &mut candidate_state.stash,
                child,
                query_alias,
                fact_alias,
            )
            .is_ok()
            {
                GoalResult::Yes {
                    modulo: Goal::true_(&mut candidate_state.stash),
                }
            } else {
                GoalResult::No
            };
            let answer = extract_query_result_with_output_at(
                db,
                &candidate_state,
                child,
                GoalOutput::Type(fact_value),
                result,
            );
            candidate_state.egraph.discard(child);
            answer
        });
    }
    let mut answers = Vec::new();
    while let Some((_task, answer)) = tasks.next_completed().await {
        answers.push(answer);
    }
    merge_candidate_results(
        db,
        next_response_param,
        &input_universes,
        answers,
        incomplete,
    )
}

fn normalization_facts<'db>(
    state: &QueryProofState<'db>,
    projection: crate::ty::ProjectionTy<'db>,
) -> Vec<(crate::ty::AliasTy<'db>, Ptr<Ty<'db>>)> {
    state.stash[state.assumptions]
        .iter()
        .filter_map(|assumption| match *assumption {
            Assumption::NormalizesTo {
                alias: alias @ crate::ty::AliasTy::Associated(fact),
                ty,
            } if fact.associated_ty == projection.associated_ty
                && fact.trait_ref.trait_sym == projection.trait_ref.trait_sym =>
            {
                Some((alias, ty))
            }
            Assumption::NormalizesTo { .. }
            | Assumption::TraitImpl { .. }
            | Assumption::Implies(..)
            | Assumption::All(_) => None,
        })
        .collect()
}

fn input_universes<'db>(
    frame: &QueryProofState<'db>,
) -> FxHashMap<crate::generic_param::AlphaEquivParam<'db>, u32> {
    frame
        .inputs
        .iter()
        .map(|input| {
            let universe = match frame.stash[input.ty] {
                Ty::InferVar(index) => frame.egraph.current_universe(frame.version, index),
                Ty::Param(param) => frame.egraph.placeholder_universe(param),
                Ty::Bool
                | Ty::Char
                | Ty::Int(_)
                | Ty::Uint(_)
                | Ty::Float(_)
                | Ty::Str
                | Ty::Adt(_, _)
                | Ty::Alias(_)
                | Ty::Ref(_, _, _)
                | Ty::Tuple(_)
                | Ty::Slice(_)
                | Ty::Array(_, _)
                | Ty::FnPtr(_, _)
                | Ty::Never
                | Ty::Error(_) => unreachable!("canonical input is not a variable"),
            };
            (input.param, universe.0)
        })
        .collect()
}

async fn solve_trait_frame<'db>(
    db: &'db dyn crate::Db,
    whiteboard: &Whiteboard<'db>,
    query: &Stashed<GoalQueryData<'db>>,
    frame: &mut QueryProofState<'db>,
    atom: Atom<'db>,
    frame_id: FrameId,
    depth: u32,
) -> Stashed<QueryResult<'db>> {
    let (candidates, incomplete) =
        assemble_candidates(db, frame, frame.version, frame.assumptions, atom);
    let input_universes: FxHashMap<_, _> = frame
        .inputs
        .iter()
        .map(|input| {
            let universe = match frame.stash[input.ty] {
                Ty::InferVar(index) => frame.egraph.current_universe(frame.version, index),
                Ty::Param(param) => frame.egraph.placeholder_universe(param),
                Ty::Bool
                | Ty::Char
                | Ty::Int(_)
                | Ty::Uint(_)
                | Ty::Float(_)
                | Ty::Str
                | Ty::Adt(_, _)
                | Ty::Alias(_)
                | Ty::Ref(_, _, _)
                | Ty::Tuple(_)
                | Ty::Slice(_)
                | Ty::Array(_, _)
                | Ty::FnPtr(_, _)
                | Ty::Never
                | Ty::Error(_) => unreachable!("canonical input is not a variable"),
            };
            (input.param, universe.0)
        })
        .collect();
    let next_response_param = frame.next_response_param;
    let candidate_count = candidates.len();
    // ANCHOR: example_run_trait_candidates
    let mut tasks = ScopedTasks::new();
    for candidate_index in 0..candidate_count {
        let query = query.clone();
        let whiteboard = whiteboard.clone();
        tasks.spawn(async move {
            let mut candidate_state = instantiate_query(&query);
            let SolverGoal::Prove(Goal::Atom(candidate_atom)) = candidate_state.goal else {
                unreachable!("candidate query is not atomic")
            };
            let (candidates, _) = assemble_candidates(
                db,
                &candidate_state,
                candidate_state.version,
                candidate_state.assumptions,
                candidate_atom,
            );
            let candidate = candidates[candidate_index];
            let parent = candidate_state.version;
            let child = candidate_state.egraph.branch_from(parent);
            let instantiated = instantiate_candidate(db, &mut candidate_state, child, candidate);
            let result = match match_candidate_head(
                &mut candidate_state,
                child,
                candidate_atom,
                instantiated.head,
            ) {
                Ok(()) => {
                    let body_goals = candidate_state.stash[instantiated.body].to_vec();
                    let body = Goal::all(&mut candidate_state.stash, body_goals);
                    let environment = candidate_state.assumptions;
                    prove_goal(
                        db,
                        &whiteboard,
                        &mut candidate_state,
                        child,
                        environment,
                        body,
                        depth,
                        Some(frame_id),
                    )
                    .await
                }
                Err(_) => GoalResult::No,
            };
            let answer = extract_query_result_at(db, &candidate_state, child, result);
            candidate_state.egraph.discard(child);
            answer
        });
    }

    let mut answers = Vec::new();
    while let Some((_task, answer)) = tasks.next_completed().await {
        let unconditional = is_unconditional_answer(&answer);
        answers.push(answer);
        if unconditional {
            tasks.cancel_all();
            break;
        }
    }
    // ANCHOR_END: example_run_trait_candidates
    merge_candidate_results(
        db,
        next_response_param,
        &input_universes,
        answers,
        incomplete,
    )
}

fn is_unconditional_answer(answer: &Stashed<QueryResult<'_>>) -> bool {
    let (stash, result) = answer.open();
    matches!(
        result.value,
        super::QueryResultData::Yes {
            output: GoalOutput::Proven,
            subst,
            modulo,
        } if stash[subst].is_empty() && modulo.is_trivially_true(stash)
    )
}

// ANCHOR: example_match_candidate_head
fn match_candidate_head<'db>(
    state: &mut QueryProofState<'db>,
    version: Version,
    atom: Atom<'db>,
    head: crate::ty::WherePredicate<'db>,
) -> Result<(), UnifyError<'db>> {
    let Atom::TraitImpl { self_ty, trait_ref } = atom else {
        unreachable!()
    };
    unify_in_probe(
        &mut state.egraph,
        &mut state.stash,
        version,
        self_ty,
        head.self_ty,
    )?;
    for (&goal_arg, &head_arg) in state.stash[trait_ref.args]
        .to_vec()
        .iter()
        .zip(state.stash[head.trait_ref.args].to_vec().iter())
    {
        unify_in_probe(
            &mut state.egraph,
            &mut state.stash,
            version,
            goal_arg,
            head_arg,
        )?;
    }
    state.egraph.rebuild(version, &mut state.stash);
    Ok(())
}
// ANCHOR_END: example_match_candidate_head

// ANCHOR: example_prove_conjunction
async fn prove_conjunction<'db>(
    db: &'db dyn crate::Db,
    whiteboard: &Whiteboard<'db>,
    state: &mut QueryProofState<'db>,
    parent: Version,
    environment: Slice<Assumption<'db>>,
    goals: Slice<Goal<'db>>,
    depth: u32,
    parent_frame: Option<FrameId>,
) -> GoalResult<'db> {
    let child = state.egraph.branch_from(parent);
    let mut pending: Vec<(Goal<'db>, Option<u64>, bool)> = state.stash[goals]
        .iter()
        .copied()
        .map(|goal| (goal, None, false))
        .collect();

    loop {
        if pending.is_empty() {
            state.egraph.collapse_into(child, parent);
            return GoalResult::Yes {
                modulo: Goal::true_(&mut state.stash),
            };
        }
        let revision_before = state.egraph.semantic_revision(child);
        let mut attempted = false;
        let mut index = 0;
        while index < pending.len() {
            let revision = state.egraph.semantic_revision(child);
            if pending[index].1 == Some(revision) {
                index += 1;
                continue;
            }
            attempted = true;
            let goal = pending[index].0;
            match prove_goal(
                db,
                whiteboard,
                state,
                child,
                environment,
                goal,
                depth,
                parent_frame,
            )
            .await
            {
                GoalResult::No => {
                    state.egraph.discard(child);
                    return GoalResult::No;
                }
                GoalResult::Maybe => {
                    pending[index].1 = Some(state.egraph.semantic_revision(child));
                    pending[index].2 = true;
                    index += 1;
                }
                GoalResult::Yes { modulo } if modulo.is_trivially_true(&state.stash) => {
                    pending.remove(index);
                }
                GoalResult::Yes { modulo } => {
                    pending[index] = (modulo, Some(state.egraph.semantic_revision(child)), false);
                    index += 1;
                }
            }
        }
        let revision_after = state.egraph.semantic_revision(child);
        if !attempted || revision_after == revision_before {
            if pending.iter().any(|(_, _, uncertain)| *uncertain) {
                state.egraph.collapse_into(child, parent);
                return GoalResult::Maybe;
            }
            let residuals: Vec<_> = pending.into_iter().map(|(goal, _, _)| goal).collect();
            let modulo = Goal::all(&mut state.stash, residuals);
            state.egraph.collapse_into(child, parent);
            return GoalResult::Yes { modulo };
        }
    }
}
// ANCHOR_END: example_prove_conjunction

async fn prove_exists<'db>(
    db: &'db dyn crate::Db,
    whiteboard: &Whiteboard<'db>,
    state: &mut QueryProofState<'db>,
    parent: Version,
    environment: Slice<Assumption<'db>>,
    binder: crate::ty::Binder<'db, sage_stash::Ptr<Goal<'db>>>,
    depth: u32,
    parent_frame: Option<FrameId>,
) -> GoalResult<'db> {
    let child = state.egraph.branch_from(parent);
    let mut temporary = sage_stash::Stash::new();
    let copied_binder = binder.stash_copy(&state.stash, &mut temporary);
    let mut mapping = FxHashMap::default();
    let mut opened_variables = Vec::new();
    for generic in &temporary[copied_binder.generics] {
        assert_eq!(generic.kind(db), GenericParamKind::Type);
        let index = state.egraph.alloc_var(child, state.canonical_universe);
        let ty = state.stash.alloc(Ty::InferVar(index));
        mapping.insert(*generic, ty);
        opened_variables.push((index, *generic));
    }
    let mut copier = IrCopier::new(&temporary, &mut state.stash, mapping, None);
    let opened = copier.copy_goal(temporary[copied_binder.value]);
    match prove_goal(
        db,
        whiteboard,
        state,
        child,
        environment,
        opened,
        depth,
        parent_frame,
    )
    .await
    {
        GoalResult::No => {
            state.egraph.discard(child);
            GoalResult::No
        }
        GoalResult::Yes { modulo } if modulo.is_trivially_true(&state.stash) => {
            state.egraph.collapse_into(child, parent);
            GoalResult::Yes { modulo }
        }
        GoalResult::Maybe => {
            state.egraph.collapse_into(child, parent);
            GoalResult::Maybe
        }
        GoalResult::Yes { modulo } => {
            let mut roots = FxHashMap::default();
            for (index, generic) in opened_variables {
                let ty = state.stash.alloc(Ty::InferVar(index));
                let root = state.egraph.find(child, ty);
                if matches!(state.stash[root], Ty::InferVar(_)) {
                    roots.insert(root, generic);
                }
            }
            let modulo = reabstract_goal(state, child, modulo, &roots, &mut FxHashMap::default());
            let modulo = state.stash.alloc(modulo);
            state.egraph.collapse_into(child, parent);
            GoalResult::Yes {
                modulo: Goal::Exists(crate::ty::Binder::new(modulo, binder.generics)),
            }
        }
    }
}

fn reabstract_goal<'db>(
    state: &mut QueryProofState<'db>,
    version: Version,
    goal: Goal<'db>,
    roots: &FxHashMap<Ptr<Ty<'db>>, GenericParam<'db>>,
    memo: &mut FxHashMap<Ptr<Ty<'db>>, Ptr<Ty<'db>>>,
) -> Goal<'db> {
    match goal {
        Goal::Exists(binder) => {
            let inner = reabstract_goal(state, version, state.stash[binder.value], roots, memo);
            Goal::Exists(crate::ty::Binder::new(
                state.stash.alloc(inner),
                binder.generics,
            ))
        }
        Goal::Implies(assumptions, inner) => {
            let assumptions = state.stash[assumptions].to_vec();
            let assumptions: Vec<_> = assumptions
                .into_iter()
                .map(|assumption| reabstract_assumption(state, version, assumption, roots, memo))
                .collect();
            let assumptions = Assumption::flatten(&mut state.stash, assumptions);
            let inner_goal = reabstract_goal(state, version, state.stash[inner], roots, memo);
            Goal::Implies(assumptions, state.stash.alloc(inner_goal))
        }
        Goal::All(goals) => {
            let goals = state.stash[goals].to_vec();
            let goals: Vec<_> = goals
                .into_iter()
                .map(|goal| reabstract_goal(state, version, goal, roots, memo))
                .collect();
            Goal::all(&mut state.stash, goals)
        }
        Goal::Atom(atom) => Goal::Atom(match atom {
            Atom::TraitImpl { self_ty, trait_ref } => Atom::TraitImpl {
                self_ty: reabstract_ty(state, version, self_ty, roots, memo),
                trait_ref: reabstract_trait_ref(state, version, trait_ref, roots, memo),
            },
            Atom::Equals(left, right) => Atom::Equals(
                reabstract_ty(state, version, left, roots, memo),
                reabstract_ty(state, version, right, roots, memo),
            ),
        }),
        Goal::Maybe => Goal::Maybe,
    }
}

fn reabstract_assumption<'db>(
    state: &mut QueryProofState<'db>,
    version: Version,
    assumption: Assumption<'db>,
    roots: &FxHashMap<Ptr<Ty<'db>>, GenericParam<'db>>,
    memo: &mut FxHashMap<Ptr<Ty<'db>>, Ptr<Ty<'db>>>,
) -> Assumption<'db> {
    match assumption {
        Assumption::TraitImpl { self_ty, trait_ref } => Assumption::TraitImpl {
            self_ty: reabstract_ty(state, version, self_ty, roots, memo),
            trait_ref: reabstract_trait_ref(state, version, trait_ref, roots, memo),
        },
        Assumption::NormalizesTo { alias, ty } => Assumption::NormalizesTo {
            alias: reabstract_alias(state, version, alias, roots, memo),
            ty: reabstract_ty(state, version, ty, roots, memo),
        },
        Assumption::Implies(conditions, consequence) => {
            let conditions = state.stash[conditions].to_vec();
            let conditions: Vec<_> = conditions
                .into_iter()
                .map(|goal| reabstract_goal(state, version, goal, roots, memo))
                .collect();
            let consequence = state.stash[consequence];
            let consequence = reabstract_predicate(state, version, consequence, roots, memo);
            Assumption::Implies(
                state.stash.alloc_slice(&conditions),
                state.stash.alloc(consequence),
            )
        }
        Assumption::All(assumptions) => {
            let assumptions = state.stash[assumptions].to_vec();
            let assumptions: Vec<_> = assumptions
                .into_iter()
                .map(|item| reabstract_assumption(state, version, item, roots, memo))
                .collect();
            Assumption::All(Assumption::flatten(&mut state.stash, assumptions))
        }
    }
}

fn reabstract_alias<'db>(
    state: &mut QueryProofState<'db>,
    version: Version,
    alias: crate::ty::AliasTy<'db>,
    roots: &FxHashMap<Ptr<Ty<'db>>, GenericParam<'db>>,
    memo: &mut FxHashMap<Ptr<Ty<'db>>, Ptr<Ty<'db>>>,
) -> crate::ty::AliasTy<'db> {
    let ty = state.stash.alloc(Ty::Alias(alias));
    let ty = reabstract_ty(state, version, ty, roots, memo);
    let Ty::Alias(alias) = state.stash[ty] else {
        unreachable!("reabstracting an alias preserves its outer constructor")
    };
    alias
}

fn reabstract_predicate<'db>(
    state: &mut QueryProofState<'db>,
    version: Version,
    predicate: WherePredicate<'db>,
    roots: &FxHashMap<Ptr<Ty<'db>>, GenericParam<'db>>,
    memo: &mut FxHashMap<Ptr<Ty<'db>>, Ptr<Ty<'db>>>,
) -> WherePredicate<'db> {
    WherePredicate {
        self_ty: reabstract_ty(state, version, predicate.self_ty, roots, memo),
        trait_ref: reabstract_trait_ref(state, version, predicate.trait_ref, roots, memo),
    }
}

fn reabstract_trait_ref<'db>(
    state: &mut QueryProofState<'db>,
    version: Version,
    trait_ref: TraitRef<'db>,
    roots: &FxHashMap<Ptr<Ty<'db>>, GenericParam<'db>>,
    memo: &mut FxHashMap<Ptr<Ty<'db>>, Ptr<Ty<'db>>>,
) -> TraitRef<'db> {
    let arguments = state.stash[trait_ref.args].to_vec();
    let arguments: Vec<_> = arguments
        .into_iter()
        .map(|argument| reabstract_ty(state, version, argument, roots, memo))
        .collect();
    TraitRef {
        trait_sym: trait_ref.trait_sym,
        args: state.stash.alloc_slice(&arguments),
    }
}

fn reabstract_ty<'db>(
    state: &mut QueryProofState<'db>,
    version: Version,
    ty: Ptr<Ty<'db>>,
    roots: &FxHashMap<Ptr<Ty<'db>>, GenericParam<'db>>,
    memo: &mut FxHashMap<Ptr<Ty<'db>>, Ptr<Ty<'db>>>,
) -> Ptr<Ty<'db>> {
    let root = state.egraph.find(version, ty);
    if let Some(copied) = memo.get(&root) {
        return *copied;
    }
    if let Some(generic) = roots.get(&root) {
        let copied = state.stash.alloc(Ty::Param(*generic));
        memo.insert(root, copied);
        return copied;
    }
    let decomposed = crate::check::infer::skeleton::decompose(&state.stash, root);
    if decomposed.children.is_empty() {
        memo.insert(root, root);
        return root;
    }
    let children: Vec<_> = decomposed
        .children
        .iter()
        .map(|child| reabstract_ty(state, version, *child, roots, memo))
        .collect();
    let copied =
        crate::check::infer::skeleton::recompose(&mut state.stash, decomposed.skeleton, &children);
    memo.insert(root, copied);
    copied
}

fn trait_atom_contains_error(
    state: &QueryProofState<'_>,
    self_ty: sage_stash::Ptr<Ty<'_>>,
    trait_ref: TraitRef<'_>,
) -> bool {
    contains_error(state, self_ty)
        || state.stash[trait_ref.args]
            .iter()
            .any(|argument| contains_error(state, *argument))
}

fn contains_error(state: &QueryProofState<'_>, ty: sage_stash::Ptr<Ty<'_>>) -> bool {
    let ty = state.egraph.find(state.version, ty);
    match state.stash[ty] {
        Ty::Error(_) => true,
        Ty::Bool
        | Ty::Char
        | Ty::Int(_)
        | Ty::Uint(_)
        | Ty::Float(_)
        | Ty::Str
        | Ty::Adt(_, _)
        | Ty::Alias(_)
        | Ty::Ref(_, _, _)
        | Ty::Tuple(_)
        | Ty::Slice(_)
        | Ty::Array(_, _)
        | Ty::FnPtr(_, _)
        | Ty::Param(_)
        | Ty::InferVar(_)
        | Ty::Never => crate::check::infer::skeleton::decompose(&state.stash, ty)
            .children
            .into_iter()
            .any(|child| contains_error(state, child)),
    }
}

#[cfg(test)]
mod whiteboard_tests {
    use super::*;
    use crate::db::Database;
    use crate::local_syms::mods::{LocalModSym, ModBodySource};
    use crate::name::Name;
    use crate::scope::{LocalCrateSymbol, local_crate};
    use crate::source::SourceFile;
    use crate::span::{AbsoluteSpan, ParseSource};
    use crate::ty::IntTy;
    use sage_stash::Stash;
    use salsa::Database as _;

    #[salsa::tracked]
    fn make_test_crate<'db>(db: &'db dyn crate::Db, file: SourceFile) -> LocalCrateSymbol<'db> {
        let mut attrs = Stash::new();
        let empty = attrs.alloc_slice::<crate::cst::attrs::AttrCst>(&[]);
        let root = LocalModSym::new(
            db,
            Name::new(db, String::new()),
            None,
            ModBodySource::File(file),
            Stashed::new(attrs, empty),
            AbsoluteSpan {
                source: ParseSource::SourceFile(file),
                start: 0,
                end: 0,
            },
        );
        local_crate(db, root)
    }

    fn with_crate(f: impl for<'db> FnOnce(&'db dyn crate::Db, LocalCrateSymbol<'db>)) {
        let mut db = Database::default();
        let file = db.add_source_file("lib.rs".to_owned(), String::new());
        db.attach(|db| {
            f(db, make_test_crate(db, file));
        });
    }

    fn equality_query<'db>(krate: LocalCrateSymbol<'db>, kind: u8) -> Stashed<GoalQueryData<'db>> {
        let mut stash = Stash::new();
        let ty = match kind {
            0 => stash.alloc(Ty::Bool),
            1 => stash.alloc(Ty::Int(IntTy::I32)),
            2 => stash.alloc(Ty::Char),
            _ => unreachable!(),
        };
        let canonical_vars = stash.alloc_slice(&[]);
        let assumptions = stash.alloc_slice(&[]);
        Stashed::new(
            stash,
            GoalQueryData {
                local_crate: krate,
                canonical_universe: 0,
                canonical_vars,
                next_response_param: 0,
                assumptions_complete: true,
                assumptions,
                goal: SolverGoal::Prove(Goal::Atom(Atom::Equals(ty, ty))),
            },
        )
    }

    #[test]
    fn same_parent_requests_share_one_live_producer_and_result() {
        with_crate(|db, krate| {
            let query = equality_query(krate, 0);
            let whiteboard = Whiteboard::default();
            let first = whiteboard.request(db, query.clone(), None, 0);
            let second = whiteboard.request(db, query.clone(), None, 0);
            assert_eq!(first.frame, second.frame);
            assert_eq!(
                whiteboard.inner.borrow().frames[first.frame.0 as usize]
                    .subscriptions
                    .len(),
                2
            );

            let (first, second) = whiteboard.drive(async move { (first.await, second.await) });
            assert_eq!(first, second);
            let reused = whiteboard.request(db, query, None, 0);
            assert!(reused.whiteboard.is_none());
            let reused = whiteboard.drive(reused);
            assert_eq!(first, reused);
            let inner = whiteboard.inner.borrow();
            assert_eq!(inner.frames.len(), 1);
            assert!(matches!(inner.frames[0].state, FrameState::Ready(_)));
            assert!(inner.frames[0].subscriptions.is_empty());
        });
    }

    #[test]
    fn parent_identity_partitions_frames_and_last_drop_abandons() {
        with_crate(|db, krate| {
            let parent_a_query = equality_query(krate, 0);
            let parent_b_query = equality_query(krate, 1);
            let target_query = equality_query(krate, 2);
            let whiteboard = Whiteboard::default();
            let parent_a = whiteboard.request(db, parent_a_query, None, 0);
            let parent_b = whiteboard.request(db, parent_b_query, None, 0);
            let first = whiteboard.request(db, target_query.clone(), Some(parent_a.frame), 1);
            let shared = whiteboard.request(db, target_query.clone(), Some(parent_a.frame), 1);
            let distinct = whiteboard.request(db, target_query.clone(), Some(parent_b.frame), 1);

            assert_eq!(first.frame, shared.frame);
            assert_ne!(first.frame, distinct.frame);
            let old = first.frame;
            drop(first);
            assert!(matches!(
                whiteboard.inner.borrow().frames[old.0 as usize].state,
                FrameState::Pending
            ));
            drop(shared);
            assert!(matches!(
                whiteboard.inner.borrow().frames[old.0 as usize].state,
                FrameState::Abandoned
            ));

            let replacement = whiteboard.request(db, target_query, Some(parent_a.frame), 1);
            assert_ne!(replacement.frame, old);
            drop(replacement);
            drop(distinct);
            drop(parent_b);
            drop(parent_a);
            whiteboard.assert_drained();
        });
    }
}
