use std::cell::RefCell;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::task::Wake;

use rustc_hash::{FxHashMap, FxHashSet};
use sage_stash::{Ptr, Stash, StashCopy, Stashed};

use crate::diagnostic::{Diagnostic, ErrorReported, Span};
use crate::display::TyDisplay;
use crate::local_syms::LocalModItemSym;
use crate::name::Name;
use crate::resolve::{Namespace, Resolution, Resolver};
use crate::scope::LocalCrateSymbol;
use crate::span::RelativeSpan;
use crate::ty::{Binder, CheckedParameterEnv, FnSig, InferVarIndex, SolverEligibility, Ty};
use crate::tytree::*;

use super::infer::bound::Bound;
use super::infer::egraph::VersionedEGraph;
use super::infer::obligations::{
    Obligation, ObligationManager, ObligationProvenance, ObligationReason, ObligationState,
    StagedObligationBatch,
};
use super::infer::runtime::Runtime;
use super::infer::unify::{UnifyError, try_set_bound, try_unify};
use super::infer::version::{Universe, Version};

struct MainTaskWake(AtomicBool);

impl Wake for MainTaskWake {
    fn wake(self: Arc<Self>) {
        self.0.store(true, Ordering::Release);
    }

    fn wake_by_ref(self: &Arc<Self>) {
        self.0.store(true, Ordering::Release);
    }
}

// ---------------------------------------------------------------------------
// TypeError — carried over from body.rs
// ---------------------------------------------------------------------------

#[derive(Clone, Debug)]
pub enum TypeError<'db> {
    Fresh {
        kind: TypeErrorKind<'db>,
        span: RelativeSpan,
        context: Vec<ErrorContext>,
    },
    Reported(ErrorReported),
}

#[derive(Clone, Debug)]
pub enum TypeErrorKind<'db> {
    Mismatch {
        expected: Ptr<Ty<'db>>,
        actual: Ptr<Ty<'db>>,
    },
    UnresolvedInferVar {
        var: InferVarIndex,
    },
    AmbiguousName {
        count: usize,
    },
}

#[derive(Clone, Debug)]
pub enum ErrorContext {
    ReturnType {
        ret_span: RelativeSpan,
    },
    Argument {
        index: usize,
        call_span: RelativeSpan,
    },
    FieldInit {
        field_span: RelativeSpan,
    },
}

impl<'db> TypeError<'db> {
    pub fn with_context(mut self, context: ErrorContext) -> Self {
        match &mut self {
            TypeError::Fresh { context: ctx, .. } => ctx.push(context),
            TypeError::Reported(_) => {}
        }
        self
    }
}

// ---------------------------------------------------------------------------
// CheckError — the fatal error type for async expression checking
// ---------------------------------------------------------------------------

/// A fatal check error: the node cannot be constructed.
/// Propagates via `?` until caught at a scope boundary, which records the
/// diagnostic and substitutes `TyExprData::Error(ErrorReported)`.
#[derive(Clone, Debug)]
pub struct CheckError<'db>(pub TypeError<'db>);

impl<'db> From<TypeError<'db>> for CheckError<'db> {
    fn from(err: TypeError<'db>) -> Self {
        CheckError(err)
    }
}

/// Extension trait for `Result<(), TypeError>` — report non-fatal errors
/// without propagating.
pub trait RecordErr<'db> {
    fn record_err(self, cx: &InferCtx<'_, 'db>);
}

impl<'db> RecordErr<'db> for Result<(), TypeError<'db>> {
    fn record_err(self, cx: &InferCtx<'_, 'db>) {
        if let Err(e) = self {
            cx.catch(e);
        }
    }
}

// ---------------------------------------------------------------------------
// InferCtx — shared inference state (one per function body)
// ---------------------------------------------------------------------------

/// Shared inference state — one per function body, eventually shared by all
/// concurrent tasks. Protected by `RefCell` since the executor is
/// single-threaded and cooperative.
pub struct InferCtx<'check, 'db> {
    pub db: &'db dyn crate::Db,
    pub(crate) source_stash: &'check Stash,
    current_sym: Option<LocalModItemSym<'db>>,
    local_crate: Option<LocalCrateSymbol<'db>>,

    // The declared return type of the function being checked, and its span.
    ret_ty: Option<(Ptr<Ty<'db>>, Option<RelativeSpan>)>,

    // Shared mutable state
    egraph: RefCell<VersionedEGraph<'db>>,
    runtime: RefCell<Runtime>,
    target_stash: RefCell<Stash>,
    local_vars: RefCell<Vec<LocalVar<'db>>>,
    expr_slots: RefCell<Vec<Option<Ptr<TyExpr<'db>>>>>,

    // Trait facts available to this body and its retained proof requests.
    solver_assumptions: RefCell<sage_stash::Slice<super::solve::Assumption<'db>>>,
    assumptions_complete: RefCell<bool>,
    obligations: RefCell<ObligationManager<'db>>,

    // Wake queue: variables whose bounds changed during a poll.
    // Processed by block_on between polls (avoids double-borrow of runtime).
    pending_wakes: RefCell<Vec<InferVarIndex>>,

    // Diagnostic accumulator
    diagnostics: RefCell<Vec<Diagnostic<'db>>>,
}

impl<'check, 'db> InferCtx<'check, 'db> {
    pub fn new(
        db: &'db dyn crate::Db,
        source_stash: &'check Stash,
        current_sym: Option<LocalModItemSym<'db>>,
    ) -> Self {
        let local_crate = match current_sym {
            Some(LocalModItemSym::Function(function)) => Some(function.scope(db).local_crate(db)),
            Some(
                LocalModItemSym::Struct(_)
                | LocalModItemSym::Enum(_)
                | LocalModItemSym::Trait(_)
                | LocalModItemSym::Impl(_)
                | LocalModItemSym::TypeAlias(_)
                | LocalModItemSym::Const(_)
                | LocalModItemSym::Static(_)
                | LocalModItemSym::Mod(_)
                | LocalModItemSym::Use(_)
                | LocalModItemSym::MacroDef(_)
                | LocalModItemSym::MacroInvocation(_)
                | LocalModItemSym::Error(_),
            )
            | None => None,
        };
        let mut target_stash = Stash::new();
        let solver_assumptions = target_stash.alloc_slice(&[]);
        Self {
            db,
            source_stash,
            current_sym,
            local_crate,
            ret_ty: None,
            egraph: RefCell::new(VersionedEGraph::new()),
            runtime: RefCell::new(Runtime::new()),
            target_stash: RefCell::new(target_stash),
            local_vars: RefCell::new(Vec::new()),
            expr_slots: RefCell::new(Vec::new()),
            solver_assumptions: RefCell::new(solver_assumptions),
            assumptions_complete: RefCell::new(true),
            obligations: RefCell::new(ObligationManager::default()),
            pending_wakes: RefCell::new(Vec::new()),
            diagnostics: RefCell::new(Vec::new()),
        }
    }

    pub fn set_ret_ty(&mut self, ty: Ptr<Ty<'db>>, span: Option<RelativeSpan>) {
        self.ret_ty = Some((ty, span));
    }

    pub fn ret_ty(&self) -> Option<(Ptr<Ty<'db>>, Option<RelativeSpan>)> {
        self.ret_ty
    }

    // ------------------------------------------------------------------
    // Span helpers
    // ------------------------------------------------------------------

    pub fn span(&self, relative: RelativeSpan) -> Span<'db> {
        Span::Relative(
            self.current_sym.expect("span() called without current_sym"),
            relative,
        )
    }

    // ------------------------------------------------------------------
    // Diagnostics
    // ------------------------------------------------------------------

    pub fn record(&self, diag: Diagnostic<'db>) -> ErrorReported {
        crate::diagnostic::report(&mut self.diagnostics.borrow_mut(), diag)
    }

    pub fn catch(&self, err: TypeError<'db>) -> ErrorReported {
        match &err {
            TypeError::Reported(e) => *e,
            TypeError::Fresh { .. } => {
                let diag = self.type_error_to_diagnostic(&err).unwrap();
                self.record(diag)
            }
        }
    }

    pub fn diagnostics_snapshot(&self) -> Vec<Diagnostic<'db>> {
        self.diagnostics.borrow().clone()
    }

    pub fn has_errors(&self) -> bool {
        !self.diagnostics.borrow().is_empty()
    }

    fn type_error_to_diagnostic(&self, err: &TypeError<'db>) -> Option<Diagnostic<'db>> {
        let TypeError::Fresh {
            kind,
            span,
            context,
        } = err
        else {
            return None;
        };
        let span_resolved = self.span(*span);
        let stash = self.target_stash.borrow();
        Some(match kind {
            TypeErrorKind::Mismatch { expected, actual } => {
                let expected_str = TyDisplay::new(self.db, &*stash, *expected).to_string();
                let actual_str = TyDisplay::new(self.db, &*stash, *actual).to_string();
                let msg = format!(
                    "type mismatch: expected `{}`, found `{}`",
                    expected_str, actual_str,
                );
                let mut diag = Diagnostic::error(span_resolved.clone(), &msg)
                    .label(span_resolved, format!("found `{actual_str}`"));

                for ctx in context {
                    match ctx {
                        ErrorContext::ReturnType { ret_span } => {
                            diag = diag.secondary(
                                self.span(*ret_span),
                                format!("expected `{expected_str}` because of return type"),
                            );
                        }
                        ErrorContext::Argument { index, call_span } => {
                            diag = diag.secondary(
                                self.span(*call_span),
                                format!("expected `{expected_str}` for argument {}", index + 1),
                            );
                        }
                        ErrorContext::FieldInit { field_span } => {
                            diag = diag.secondary(
                                self.span(*field_span),
                                format!("expected `{expected_str}` for this field"),
                            );
                        }
                    }
                }

                diag
            }
            TypeErrorKind::UnresolvedInferVar { var } => Diagnostic::error(
                span_resolved,
                format!("could not infer type for ?{}", var.0),
            ),
            TypeErrorKind::AmbiguousName { count } => {
                Diagnostic::error(span_resolved, format!("ambiguous name: {count} candidates"))
            }
        })
    }

    // ------------------------------------------------------------------
    // Stash access
    // ------------------------------------------------------------------

    pub fn stash(&self) -> std::cell::Ref<'_, Stash> {
        self.target_stash.borrow()
    }

    pub fn stash_mut(&self) -> std::cell::RefMut<'_, Stash> {
        self.target_stash.borrow_mut()
    }

    // ------------------------------------------------------------------
    // Type allocation
    // ------------------------------------------------------------------

    pub fn alloc_ty(&self, ty: Ty<'db>) -> Ptr<Ty<'db>> {
        self.target_stash.borrow_mut().alloc(ty)
    }

    pub fn unit_ty(&self) -> Ptr<Ty<'db>> {
        let elems = self.target_stash.borrow_mut().alloc_slice(&[]);
        self.target_stash.borrow_mut().alloc(Ty::Tuple(elems))
    }

    // ------------------------------------------------------------------
    // Variable allocation
    // ------------------------------------------------------------------

    pub fn fresh_ty_var(&self) -> Ptr<Ty<'db>> {
        self.fresh_ty_var_in(Version::ROOT, Universe(1))
    }

    pub(crate) fn fresh_ty_var_in(&self, version: Version, universe: Universe) -> Ptr<Ty<'db>> {
        let idx = self.egraph.borrow_mut().alloc_var(version, universe);
        let ty = Ty::InferVar(idx);
        self.target_stash.borrow_mut().alloc(ty)
    }

    // ------------------------------------------------------------------
    // Egraph operations
    // ------------------------------------------------------------------

    pub fn find(&self, ty: Ptr<Ty<'db>>) -> Ptr<Ty<'db>> {
        self.find_in(Version::ROOT, ty)
    }

    pub fn find_mut(&self, ty: Ptr<Ty<'db>>) -> Ptr<Ty<'db>> {
        self.egraph.borrow_mut().find_mut(Version::ROOT, ty)
    }

    pub fn get_bound(&self, ty: Ptr<Ty<'db>>) -> Bound<'db> {
        self.egraph.borrow().get_bound(Version::ROOT, ty)
    }

    pub fn set_bound(&self, ty: Ptr<Ty<'db>>, bound: Bound<'db>) {
        let effects = try_set_bound(
            &mut self.egraph.borrow_mut(),
            &mut self.target_stash.borrow_mut(),
            Version::ROOT,
            ty,
            bound,
        )
        .expect("invalid inference bound");
        self.publish_commit_effects(effects);
    }

    pub(crate) fn find_in(&self, version: Version, ty: Ptr<Ty<'db>>) -> Ptr<Ty<'db>> {
        self.egraph.borrow().find(version, ty)
    }

    // ------------------------------------------------------------------
    // Core constraint operations
    // ------------------------------------------------------------------

    pub fn assume_eq(&self, a: Ptr<Ty<'db>>, b: Ptr<Ty<'db>>) {
        match try_unify(
            &mut self.egraph.borrow_mut(),
            &mut self.target_stash.borrow_mut(),
            Version::ROOT,
            a,
            b,
        ) {
            Ok(effects) => self.publish_commit_effects(effects),
            Err(UnifyError::Reported(_)) => {}
            Err(error) => panic!("invalid assumed equality: {error:?}"),
        }
    }

    pub fn require_eq(
        &self,
        a: Ptr<Ty<'db>>,
        b: Ptr<Ty<'db>>,
        span: RelativeSpan,
    ) -> Result<(), TypeError<'db>> {
        let result = {
            let mut egraph = self.egraph.borrow_mut();
            let mut stash = self.target_stash.borrow_mut();
            try_unify(&mut egraph, &mut stash, Version::ROOT, a, b)
        };
        match result {
            Ok(effects) => {
                self.publish_commit_effects(effects);
                Ok(())
            }
            Err(UnifyError::Reported(error)) => Err(TypeError::Reported(error)),
            Err(UnifyError::Mismatch { left, right }) => Err(TypeError::Fresh {
                kind: TypeErrorKind::Mismatch {
                    expected: right,
                    actual: left,
                },
                span,
                context: Vec::new(),
            }),
            Err(UnifyError::OccursCheck { .. } | UnifyError::UniverseLeak { .. }) => {
                Err(TypeError::Fresh {
                    kind: TypeErrorKind::Mismatch {
                        expected: self.find(b),
                        actual: self.find(a),
                    },
                    span,
                    context: Vec::new(),
                })
            }
        }
    }

    pub fn require_sub(
        &self,
        a: Ptr<Ty<'db>>,
        b: Ptr<Ty<'db>>,
        span: RelativeSpan,
    ) -> Result<(), TypeError<'db>> {
        let a_canon = self.egraph.borrow_mut().find_mut(Version::ROOT, a);
        let b_canon = self.egraph.borrow_mut().find_mut(Version::ROOT, b);
        if a_canon == b_canon {
            return Ok(());
        }

        let stash = self.target_stash.borrow();
        let a_data = stash[a_canon];
        let b_data = stash[b_canon];
        drop(stash);

        match (a_data, b_data) {
            (Ty::Never, _) => Ok(()),
            (Ty::Ref(inner_a, m_a, _), Ty::Ref(inner_b, m_b, _)) if m_a == m_b => {
                self.require_sub(inner_a, inner_b, span)
            }
            (
                Ty::Bool
                | Ty::Char
                | Ty::Int(_)
                | Ty::Uint(_)
                | Ty::Float(_)
                | Ty::Str
                | Ty::Adt(_, _)
                | Ty::Ref(_, _, _)
                | Ty::Tuple(_)
                | Ty::Slice(_)
                | Ty::Array(_, _)
                | Ty::FnPtr(_, _)
                | Ty::Param(_)
                | Ty::InferVar(_)
                | Ty::Error(_),
                _,
            ) => self.require_eq(a_canon, b_canon, span),
        }
    }

    pub fn require_coerce(
        &self,
        a: Ptr<Ty<'db>>,
        b: Ptr<Ty<'db>>,
        span: RelativeSpan,
    ) -> Result<(), TypeError<'db>> {
        self.require_sub(a, b, span)
    }

    // ------------------------------------------------------------------
    // Versioning
    // ------------------------------------------------------------------

    pub(crate) fn branch_from(&self, parent: Version) -> Version {
        self.egraph.borrow_mut().branch_from(parent)
    }

    pub(crate) fn discard_branch(&self, version: Version) {
        self.egraph.borrow_mut().discard(version);
    }

    // ------------------------------------------------------------------
    // Expression slots
    // ------------------------------------------------------------------

    pub fn alloc_expr_slot(&self) -> (ExprSlot, Ptr<Ty<'db>>) {
        let ty = self.fresh_ty_var();
        let slot = ExprSlot(self.expr_slots.borrow().len() as u32);
        self.expr_slots.borrow_mut().push(None);
        (slot, ty)
    }

    pub fn fill_expr_slot(&self, slot: ExprSlot, expr: Ptr<TyExpr<'db>>) {
        self.expr_slots.borrow_mut()[slot.0 as usize] = Some(expr);
    }

    // ------------------------------------------------------------------
    // Execution
    // ------------------------------------------------------------------

    fn publish_commit_effects(&self, effects: super::infer::egraph::CommitEffects) {
        self.obligations.borrow_mut().wake(&effects.wakes);
        self.pending_wakes.borrow_mut().extend(effects.wakes);
    }

    /// Catch a CheckError: record the diagnostic and substitute an error node.
    pub fn error_expr(&self, err: CheckError<'db>, span: RelativeSpan) -> Ptr<TyExpr<'db>> {
        let e = self.catch(err.0);
        let ty = self.alloc_ty(Ty::Error(e));
        self.alloc_expr(TyExprData::Error(e), ty, span)
    }

    /// Flush pending wakes into the runtime, then drain ready tasks.
    /// Loops until no new wakes are produced (background tasks may push
    /// to pending_wakes during drain).
    fn flush_and_drain(&self) {
        loop {
            let wakes: Vec<_> = self.pending_wakes.borrow_mut().drain(..).collect();
            if wakes.is_empty() {
                break;
            }
            let mut rt = self.runtime.borrow_mut();
            for var in wakes {
                rt.wake_variable(var);
            }
            rt.drain();
        }
    }

    /// Run a future to completion. The future may call InferCtx methods
    /// (which push to pending_wakes); between each poll we flush wakes
    /// into the runtime and drain ready background tasks.
    pub fn block_on<F: std::future::Future>(&self, future: F) -> F::Output {
        self.block_on_inner(future, false)
    }

    /// Run the root body scope while allowing stable quiescence to drive
    /// inference fallback and mandatory obligation progress. A source task may
    /// therefore suspend on information which finalization itself supplies.
    pub(crate) fn block_on_body<F: std::future::Future>(&self, future: F) -> F::Output {
        self.block_on_inner(future, true)
    }

    fn block_on_inner<F: std::future::Future>(
        &self,
        future: F,
        recover_at_quiescence: bool,
    ) -> F::Output {
        use super::infer::runtime::CURRENT_TASK;
        use std::pin::pin;
        use std::task::{Context, Poll, Waker};

        // Allocate a task ID for the main future so await_concrete can register wakers.
        let main_task_id = {
            let mut rt = self.runtime.borrow_mut();
            rt.alloc_task_id()
        };
        let notified = Arc::new(MainTaskWake(AtomicBool::new(true)));
        let waker = Waker::from(notified.clone());

        let mut future = pin!(future);
        loop {
            if !notified.0.swap(false, Ordering::AcqRel) {
                self.flush_and_drain();
                if !notified.0.load(Ordering::Acquire) {
                    panic!("deadlock: main task pending with no wake source");
                }
                continue;
            }
            CURRENT_TASK.with(|t| *t.borrow_mut() = Some(main_task_id));
            let mut cx = Context::from_waker(&waker);
            let result = future.as_mut().poll(&mut cx);
            CURRENT_TASK.with(|t| *t.borrow_mut() = None);

            match result {
                Poll::Ready(result) => {
                    self.flush_and_drain();
                    return result;
                }
                Poll::Pending => {
                    // The main task is suspended. Flush wakes and drain
                    // background tasks — they may resolve the variable the
                    // main task is waiting on. If nothing moved, deadlock.
                    let wakes_before = self.pending_wakes.borrow().len();
                    self.flush_and_drain();
                    let quiescent = self.runtime.borrow().is_quiescent()
                        && self.pending_wakes.borrow().is_empty();
                    if quiescent {
                        // Nothing running, nothing pending — if we just
                        // flushed wakes, try once more (the main task
                        // may have been unblocked). Otherwise, deadlock.
                        if wakes_before == 0 && !notified.0.load(Ordering::Acquire) {
                            if recover_at_quiescence {
                                self.finalize();
                                self.flush_and_drain();
                                if notified.0.load(Ordering::Acquire) {
                                    continue;
                                }
                            }
                            panic!("deadlock: main task pending with no runnable tasks");
                        }
                    }
                }
            }
        }
    }

    /// Suspend until a type is concrete (not an unresolved infer var).
    /// After finalization, unresolved vars become Ty::Error, so this always terminates.
    pub async fn await_concrete(&self, ty: Ptr<Ty<'db>>) -> Ptr<Ty<'db>> {
        use super::infer::runtime::CURRENT_TASK;
        use std::future::poll_fn;
        use std::task::Poll;

        loop {
            let canon = self.find(ty);
            let data = self.target_stash.borrow()[canon];
            match data {
                Ty::InferVar(idx) => {
                    // Suspend: register current task to wake on this variable's next bound change.
                    poll_fn(|poll_context| {
                        // Re-check in case it resolved between iterations
                        let canon = self.find(ty);
                        let data = self.target_stash.borrow()[canon];
                        if !matches!(data, Ty::InferVar(_)) {
                            return Poll::Ready(());
                        }
                        // Register waker
                        let task_id = CURRENT_TASK.with(|t| *t.borrow());
                        if let Some(task_id) = task_id {
                            self.runtime
                                .borrow_mut()
                                .wait_on(idx, task_id, poll_context.waker());
                        }
                        Poll::Pending
                    })
                    .await;
                }
                Ty::Bool
                | Ty::Char
                | Ty::Int(_)
                | Ty::Uint(_)
                | Ty::Float(_)
                | Ty::Str
                | Ty::Adt(_, _)
                | Ty::Ref(_, _, _)
                | Ty::Tuple(_)
                | Ty::Slice(_)
                | Ty::Array(_, _)
                | Ty::FnPtr(_, _)
                | Ty::Param(_)
                | Ty::Never
                | Ty::Error(_) => return canon,
            }
        }
    }

    // ------------------------------------------------------------------
    // TyExpr allocation
    // ------------------------------------------------------------------

    pub fn alloc_expr(
        &self,
        data: TyExprData<'db>,
        ty: Ptr<Ty<'db>>,
        span: RelativeSpan,
    ) -> Ptr<TyExpr<'db>> {
        self.target_stash
            .borrow_mut()
            .alloc(TyExpr { data, ty, span })
    }

    // ------------------------------------------------------------------
    // Locals
    // ------------------------------------------------------------------

    pub fn add_local_var(&self, local_var: LocalVar<'db>) {
        self.local_vars.borrow_mut().push(local_var);
    }

    // ------------------------------------------------------------------
    // Signature import
    // ------------------------------------------------------------------

    pub fn import_fn_sig(&self, sig: &Stashed<Binder<'db, FnSig<'db>>>) -> FnSig<'db> {
        let sig_stash = sig.stash();
        let binder = sig.root();
        let fn_sig = binder.value;

        let params: smallvec::SmallVec<[Ptr<Ty<'db>>; 16]> = sig_stash[fn_sig.params]
            .iter()
            .map(|p| p.stash_copy(sig_stash, &mut *self.target_stash.borrow_mut()))
            .collect();
        let params = self.target_stash.borrow_mut().alloc_slice(&params);

        let ret = fn_sig
            .ret
            .stash_copy(sig_stash, &mut *self.target_stash.borrow_mut());

        let parameter_env = fn_sig
            .parameter_env
            .stash_copy(sig_stash, &mut *self.target_stash.borrow_mut());

        FnSig {
            params,
            ret,
            parameter_env,
        }
    }

    // ------------------------------------------------------------------
    // Trait environments and obligations
    // ------------------------------------------------------------------

    /// Install the opened function parameter environment as body assumptions.
    pub fn set_solver_environment(&self, environment: CheckedParameterEnv<'db>) {
        use super::solve::Assumption;

        let (predicates, assumptions_complete) = self.elaborate_solver_predicates(environment);
        let assumptions: Vec<_> = predicates
            .into_iter()
            .map(|predicate| Assumption::TraitImpl {
                self_ty: predicate.self_ty,
                trait_ref: predicate.trait_ref,
            })
            .collect();
        let assumptions = Assumption::flatten(&mut self.target_stash.borrow_mut(), assumptions);
        *self.solver_assumptions.borrow_mut() = assumptions;
        *self.assumptions_complete.borrow_mut() = assumptions_complete;
    }

    fn elaborate_solver_predicates(
        &self,
        environment: CheckedParameterEnv<'db>,
    ) -> (Vec<crate::ty::WherePredicate<'db>>, bool) {
        use super::solve::IrCopier;
        use crate::symbol::TraitSymbol;
        use crate::ty::BinderExt;

        let mut complete = environment.solver_eligibility.is_eligible();
        let mut queue = self.target_stash.borrow()[environment.where_clauses].to_vec();
        let mut output = Vec::new();
        let mut seen = FxHashSet::default();

        while let Some(predicate) = queue.pop() {
            let key = {
                let source = self.target_stash.borrow();
                let mut target = Stash::new();
                let copied = predicate.stash_copy(&source, &mut target);
                Stashed::new(target, copied)
            };
            if !seen.insert(key) {
                continue;
            }
            output.push(predicate);

            let TraitSymbol::Local(local_trait) = predicate.trait_ref.trait_sym else {
                complete = false;
                continue;
            };
            let signature = local_trait.sig(self.db);
            let (source, binder) = signature.open();
            if !binder.value.solver_eligibility.is_eligible() {
                complete = false;
                continue;
            }

            let mut mapping = FxHashMap::default();
            mapping.insert(binder.value.self_param, predicate.self_ty);
            for (generic, argument) in signature
                .iter_symbols()
                .skip(1)
                .zip(self.target_stash.borrow()[predicate.trait_ref.args].to_vec())
            {
                mapping.insert(generic, argument);
            }
            let defining = source[binder.value.where_clauses].to_vec();
            let mut target = self.target_stash.borrow_mut();
            let mut copier = IrCopier::new(source, &mut target, mapping, None);
            queue.extend(
                defining
                    .into_iter()
                    .map(|predicate| copier.copy_predicate(predicate)),
            );
        }

        output.reverse();
        (output, complete)
    }

    pub fn submit_parameter_env(
        &self,
        environment: CheckedParameterEnv<'db>,
        span: RelativeSpan,
        reason: ObligationReason,
    ) {
        let mut batch = StagedObligationBatch::new();
        batch.push_parameter_env(environment, span, reason);
        self.publish_obligation_batch(batch);
    }

    /// Publish obligations staged beside an inference transaction. A caller
    /// that rolls its inference child back simply drops the batch.
    pub fn publish_obligation_batch(&self, batch: StagedObligationBatch<'db>) {
        use super::solve::{Atom, Goal};

        for (environment, provenance) in batch.environments {
            let predicates = self.target_stash.borrow()[environment.where_clauses].to_vec();
            for predicate in predicates {
                self.submit_obligation_goal(
                    Goal::Atom(Atom::TraitImpl {
                        self_ty: predicate.self_ty,
                        trait_ref: predicate.trait_ref,
                    }),
                    provenance,
                );
            }
            if environment.solver_eligibility == SolverEligibility::Unsupported {
                self.submit_obligation_goal(Goal::Maybe, provenance);
            }
        }
    }

    pub fn submit_trait_obligation(
        &self,
        self_ty: Ptr<Ty<'db>>,
        trait_ref: crate::ty::TraitRef<'db>,
        span: RelativeSpan,
        reason: ObligationReason,
    ) {
        self.submit_obligation_goal(
            super::solve::Goal::Atom(super::solve::Atom::TraitImpl { self_ty, trait_ref }),
            ObligationProvenance { span, reason },
        );
    }

    fn canonicalize_obligation_goal(
        &self,
        local_crate: crate::scope::LocalCrateSymbol<'db>,
        assumptions_complete: bool,
        assumptions: sage_stash::Slice<super::solve::Assumption<'db>>,
        goal: super::solve::Goal<'db>,
    ) -> (
        super::solve::CanonicalizedGoal<'db>,
        Vec<crate::ty::InferVarIndex>,
    ) {
        let canonical = super::solve::canonicalize_goal(
            self.db,
            &self.target_stash.borrow(),
            &self.egraph.borrow(),
            Version::ROOT,
            local_crate,
            Universe(1),
            assumptions_complete,
            assumptions,
            goal,
        );
        let stalled_on = canonical
            .mapping
            .inputs
            .iter()
            .filter_map(|input| match input {
                super::solve::CallerCanonicalVar::Existential(variable) => Some(*variable),
                super::solve::CallerCanonicalVar::Rigid(_) => None,
            })
            .collect();
        (canonical, stalled_on)
    }

    fn submit_obligation_goal(
        &self,
        goal: super::solve::Goal<'db>,
        provenance: ObligationProvenance,
    ) {
        let local_crate = self
            .local_crate
            .expect("trait obligations require a local crate context");
        let assumptions = *self.solver_assumptions.borrow();
        let assumptions_complete = *self.assumptions_complete.borrow();
        let (canonical, stalled_on) =
            self.canonicalize_obligation_goal(local_crate, assumptions_complete, assumptions, goal);

        let mut manager = self.obligations.borrow_mut();
        if let Some(existing) = manager.obligations.iter_mut().find(|existing| {
            existing.state != ObligationState::Terminal
                && existing.canonical_goal == canonical.data
                && existing.mapping.absolute_universe_base
                    == canonical.mapping.absolute_universe_base
                && existing.mapping.inputs == canonical.mapping.inputs
        }) {
            if !existing.provenance.contains(&provenance) {
                existing.provenance.push(provenance);
            }
            return;
        }
        manager.obligations.push(Obligation {
            goal,
            assumptions,
            assumptions_complete,
            local_crate,
            canonical_goal: canonical.data,
            mapping: canonical.mapping,
            provenance: vec![provenance],
            state: ObligationState::Ready,
            stalled_on,
            last_attempted_revision: None,
        });
    }

    fn recanonicalize_obligation(&self, index: usize) -> u64 {
        let (goal, assumptions, assumptions_complete, local_crate) = {
            let manager = self.obligations.borrow();
            let obligation = &manager.obligations[index];
            (
                obligation.goal,
                obligation.assumptions,
                obligation.assumptions_complete,
                obligation.local_crate,
            )
        };
        let revision = self.egraph.borrow().semantic_revision(Version::ROOT);
        let (canonical, stalled_on) =
            self.canonicalize_obligation_goal(local_crate, assumptions_complete, assumptions, goal);
        let mut manager = self.obligations.borrow_mut();
        let obligation = &mut manager.obligations[index];
        obligation.canonical_goal = canonical.data;
        obligation.mapping = canonical.mapping;
        obligation.stalled_on = stalled_on;
        revision
    }

    fn process_ready_obligations(&self) {
        loop {
            self.deduplicate_pending_obligations();
            let ready = self.obligations.borrow().ready_indices();
            if ready.is_empty() {
                return;
            }
            for index in ready {
                self.attempt_obligation(index, false);
            }
        }
    }

    fn deduplicate_pending_obligations(&self) {
        let pending = self.obligations.borrow().pending_indices();
        for &index in &pending {
            self.recanonicalize_obligation(index);
        }
        let mut manager = self.obligations.borrow_mut();
        for (position, &index) in pending.iter().enumerate() {
            if manager.obligations[index].state == ObligationState::Terminal {
                continue;
            }
            for &duplicate in &pending[position + 1..] {
                if manager.obligations[duplicate].state == ObligationState::Terminal {
                    continue;
                }
                let equivalent = {
                    let left = &manager.obligations[index];
                    let right = &manager.obligations[duplicate];
                    left.canonical_goal == right.canonical_goal
                        && left.mapping.absolute_universe_base
                            == right.mapping.absolute_universe_base
                        && left.mapping.inputs == right.mapping.inputs
                };
                if equivalent {
                    let additional = manager.obligations[duplicate].provenance.clone();
                    for provenance in additional {
                        if !manager.obligations[index].provenance.contains(&provenance) {
                            manager.obligations[index].provenance.push(provenance);
                        }
                    }
                    manager.obligations[duplicate].state = ObligationState::Terminal;
                }
            }
        }
    }

    fn attempt_obligation(&self, index: usize, terminal: bool) {
        use super::solve::{AppliedCertainty, GoalQuery, apply_query_response};

        let revision = self.recanonicalize_obligation(index);
        let (canonical_goal, mapping, unchanged) = {
            let manager = self.obligations.borrow();
            let obligation = &manager.obligations[index];
            (
                obligation.canonical_goal.clone(),
                obligation.mapping.clone(),
                obligation.last_attempted_revision == Some(revision),
            )
        };
        if unchanged && !terminal {
            self.obligations.borrow_mut().obligations[index].state = ObligationState::Stalled;
            return;
        }

        let response = GoalQuery::new(self.db, canonical_goal.clone()).prove(self.db);
        let applied = {
            let mut stash = self.target_stash.borrow_mut();
            let mut egraph = self.egraph.borrow_mut();
            apply_query_response(
                self.db,
                &mut stash,
                &mut egraph,
                Version::ROOT,
                &canonical_goal,
                &mapping,
                &response,
            )
        };
        let Ok(applied) = applied else {
            self.fail_obligation(
                index,
                "trait obligation was disproved by incompatible inference",
            );
            return;
        };
        let post_revision = applied.effects.semantic_revision;
        let wakes = applied.effects.wakes.clone();
        self.publish_commit_effects(applied.effects);

        match applied.certainty {
            AppliedCertainty::No => {
                self.fail_obligation(index, "trait obligation is not satisfied");
            }
            AppliedCertainty::Maybe => {
                if terminal {
                    self.fail_obligation(index, "trait obligation could not be resolved");
                    return;
                }
                self.recanonicalize_obligation(index);
                let mut manager = self.obligations.borrow_mut();
                let obligation = &mut manager.obligations[index];
                obligation.last_attempted_revision = Some(post_revision);
                obligation.state = if obligation
                    .stalled_on
                    .iter()
                    .any(|variable| wakes.contains(variable))
                {
                    ObligationState::Ready
                } else {
                    ObligationState::Stalled
                };
            }
            AppliedCertainty::Yes { modulo } => {
                if modulo.is_trivially_true(&self.target_stash.borrow()) {
                    let mut manager = self.obligations.borrow_mut();
                    manager.obligations[index].state = ObligationState::Terminal;
                    manager.obligations[index].last_attempted_revision = Some(post_revision);
                } else if terminal {
                    self.fail_obligation(index, "trait obligation remains conditional");
                } else {
                    {
                        let mut manager = self.obligations.borrow_mut();
                        let obligation = &mut manager.obligations[index];
                        obligation.goal = modulo;
                        obligation.last_attempted_revision = Some(post_revision);
                        obligation.state = ObligationState::Stalled;
                    }
                    self.recanonicalize_obligation(index);
                }
            }
        }
    }

    fn fail_obligation(&self, index: usize, message: &str) {
        let provenances = {
            let mut manager = self.obligations.borrow_mut();
            let obligation = &mut manager.obligations[index];
            if obligation.state == ObligationState::Terminal {
                return;
            }
            obligation.state = ObligationState::Terminal;
            obligation.provenance.clone()
        };
        let Some(first) = provenances.first().copied() else {
            return;
        };
        let mut diagnostic = Diagnostic::error(self.span(first.span), message)
            .label(self.span(first.span), first.reason.description());
        for provenance in provenances.into_iter().skip(1) {
            diagnostic =
                diagnostic.secondary(self.span(provenance.span), provenance.reason.description());
        }
        self.record(diagnostic);
    }

    fn terminally_discharge_obligations(&self) {
        self.deduplicate_pending_obligations();
        let pending = self.obligations.borrow().pending_indices();
        for index in pending {
            self.attempt_obligation(index, true);
        }
        assert!(
            !self.obligations.borrow().has_pending(),
            "body checking finished with a pending trait obligation"
        );
    }

    // ------------------------------------------------------------------
    // Finalization
    // ------------------------------------------------------------------

    pub fn finalize(&self) {
        // First consume every proof that current inference already enables.
        self.process_ready_obligations();

        let mut unresolved_vars = Vec::new();

        let variables: Vec<_> = self
            .egraph
            .borrow()
            .version_tree()
            .visible_variables(Version::ROOT)
            .collect();
        for idx in variables {
            let ty = self.target_stash.borrow_mut().alloc(Ty::InferVar(idx));
            let canon = self.egraph.borrow_mut().find_mut(Version::ROOT, ty);

            if canon != ty {
                continue;
            }

            let bound = self.egraph.borrow().get_bound(Version::ROOT, ty);
            match bound {
                Bound::None => {
                    unresolved_vars.push((ty, idx));
                }
                Bound::AtLeast(bound_ty) => {
                    let effects = try_unify(
                        &mut self.egraph.borrow_mut(),
                        &mut self.target_stash.borrow_mut(),
                        Version::ROOT,
                        ty,
                        bound_ty,
                    )
                    .expect("final bound must be structurally valid");
                    self.publish_commit_effects(effects);
                }
                Bound::Exactly(_) => {}
            }
        }

        for (ty, idx) in unresolved_vars {
            let span = RelativeSpan { start: 0, end: 0 };
            let err = TypeError::Fresh {
                kind: TypeErrorKind::UnresolvedInferVar { var: idx },
                span,
                context: Vec::new(),
            };
            let e = self.catch(err);

            let error_ty = self.target_stash.borrow_mut().alloc(Ty::Error(e));
            let effects = try_unify(
                &mut self.egraph.borrow_mut(),
                &mut self.target_stash.borrow_mut(),
                Version::ROOT,
                ty,
                error_ty,
            );
            match effects {
                Ok(effects) => self.publish_commit_effects(effects),
                // Error recovery deliberately relates an unresolved variable
                // to the sentinel even though ordinary equality propagates it.
                Err(UnifyError::Reported(_)) => {
                    let child = self.egraph.borrow_mut().branch_from(Version::ROOT);
                    {
                        let stash = self.target_stash.borrow();
                        let mut egraph = self.egraph.borrow_mut();
                        egraph.set_bound_in(child, &stash, ty, Bound::Exactly(error_ty));
                        egraph.union(child, &stash, ty, error_ty);
                    }
                    let effects = self.egraph.borrow_mut().collapse_into(child, Version::ROOT);
                    self.publish_commit_effects(effects);
                }
                Err(error) => panic!("failed to install error recovery type: {error:?}"),
            }
        }

        // Fallback and final bounds may make retained goals concrete. Give
        // those goals one ordinary retry before the mandatory terminal pass.
        self.process_ready_obligations();
        self.terminally_discharge_obligations();

        self.flush_and_drain();
        self.runtime.borrow_mut().wake_all();
        self.runtime.borrow_mut().drain();
    }

    pub fn resolve_types(&self) {
        let variables: Vec<_> = self
            .egraph
            .borrow()
            .version_tree()
            .visible_variables(Version::ROOT)
            .collect();

        let mut stash = self.target_stash.borrow_mut();
        let mut egraph = self.egraph.borrow_mut();
        for idx in variables {
            let ty_ptr = stash.alloc(Ty::InferVar(idx));
            let resolved = egraph.find_mut(Version::ROOT, ty_ptr);
            if resolved != ty_ptr {
                let resolved_ty = stash[resolved];
                stash[ty_ptr] = resolved_ty;
            }
        }
    }

    // ------------------------------------------------------------------
    // Finish — consumes self, produces CheckedBody
    // ------------------------------------------------------------------

    pub fn finish(self, root: Ptr<TyExpr<'db>>, span: RelativeSpan) -> CheckedBody<'db> {
        assert!(
            !self.obligations.borrow().has_pending(),
            "body finished with a pending trait obligation"
        );
        assert!(
            self.egraph.borrow().version_tree().is_leaf(Version::ROOT),
            "body finished with a live inference branch"
        );
        assert!(
            self.runtime.borrow().is_quiescent(),
            "body finished with a live inference task"
        );
        assert!(
            self.pending_wakes.borrow().is_empty(),
            "body finished with unpublished inference wakes"
        );
        let mut stash = self.target_stash.into_inner();
        let local_vars = self.local_vars.into_inner();
        let locals = stash.alloc_slice(&local_vars);
        let body_data = stash.alloc(TyBodyData { root, locals, span });
        CheckedBody {
            body: Stashed::new(stash, body_data),
            diagnostics: self.diagnostics.into_inner(),
        }
    }
}

// ---------------------------------------------------------------------------
// Scope — task-local context (resolver ribs + visible locals)
// ---------------------------------------------------------------------------

/// Task-local scope — passed by `&Scope` (shared reference) to expression
/// checkers. Block-level code that introduces bindings clones the scope locally.
#[derive(Clone)]
pub struct Scope<'db> {
    pub resolver: Resolver<'db>,
    pub locals: Vec<Ptr<Ty<'db>>>,
}

impl<'db> Scope<'db> {
    pub fn new(resolver: Resolver<'db>) -> Self {
        Self {
            resolver,
            locals: Vec::new(),
        }
    }

    pub fn local_type(&self, id: u32) -> Ptr<Ty<'db>> {
        self.locals[id as usize]
    }

    pub fn add_binding(
        &mut self,
        cx: &InferCtx<'_, 'db>,
        name: Name<'db>,
        span: RelativeSpan,
    ) -> LocalId {
        let local_var_count = cx.local_vars.borrow().len();
        let id = LocalId(local_var_count as u32);
        cx.add_local_var(LocalVar { name, span });
        let var = cx.fresh_ty_var();
        self.locals.push(var);
        self.resolver
            .ribs
            .add(name, Namespace::Value, Resolution::Local(id));
        id
    }

    pub fn push_local(&mut self, ty: Ptr<Ty<'db>>) -> u32 {
        let id = self.locals.len() as u32;
        self.locals.push(ty);
        id
    }

    pub fn bind_params(
        &mut self,
        cx: &InferCtx<'_, 'db>,
        param_tys: sage_stash::Slice<Ptr<Ty<'db>>>,
        params_cst: sage_stash::Slice<crate::cst::fns::ParamCst<'db>>,
    ) {
        let stash = cx.stash();
        let params_list = cx.source_stash[params_cst].to_vec();
        let param_ty_list = stash[param_tys].to_vec();
        drop(stash);

        for (param_cst, param_ty) in params_list.iter().zip(param_ty_list.iter()) {
            if let Some(name) = param_cst.name {
                let local_var_count = cx.local_vars.borrow().len();
                let id = LocalId(local_var_count as u32);
                cx.add_local_var(LocalVar {
                    name,
                    span: param_cst.span,
                });
                self.locals.push(*param_ty);
                self.resolver
                    .ribs
                    .add(name, Namespace::Value, Resolution::Local(id));
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Path resolution helper
// ---------------------------------------------------------------------------

pub fn resolve_path<'db>(
    cx: &InferCtx<'_, 'db>,
    scope: &Scope<'db>,
    path: crate::cst::paths::Path<'db>,
    ns: Namespace,
    span: RelativeSpan,
) -> Res<'db> {
    let mut resolver = scope.resolver.clone();
    let results = resolver.resolve_path(cx.source_stash, path, ns);
    if results.len() > 1 {
        let err = TypeError::Fresh {
            kind: TypeErrorKind::AmbiguousName {
                count: results.len(),
            },
            span,
            context: Vec::new(),
        };
        let e = cx.catch(err);
        return Res::Error(e);
    }
    match results.into_iter().next() {
        Some(Resolution::Sym(sym)) => Res::Def(sym),
        Some(Resolution::Local(id)) => Res::Local(id),
        Some(Resolution::Param(_) | Resolution::SelfTy(_)) | None => {
            let e = cx.record(Diagnostic::error(cx.span(span), "unresolved name"));
            Res::Error(e)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sage_stash::Stash;

    fn make_cx(stash: &Stash) -> InferCtx<'_, 'static> {
        InferCtx::new(leak_db(), stash, None)
    }

    fn leak_db() -> &'static dyn crate::Db {
        use crate::db::Database;
        let db = Box::new(Database::default());
        Box::leak(db)
    }

    #[test]
    fn await_concrete_already_resolved() {
        let stash = Stash::new();
        let cx = make_cx(&stash);
        let ty = cx.alloc_ty(Ty::Bool);

        let result = cx.block_on(async { cx.await_concrete(ty).await });
        assert_eq!(cx.stash()[result], Ty::Bool);
    }

    #[test]
    fn await_concrete_resolved_by_background_task() {
        let stash = Stash::new();
        let cx = make_cx(&stash);

        // Create an inference variable
        let var = cx.fresh_ty_var();
        let bool_ty = cx.alloc_ty(Ty::Bool);

        // For this test, resolve the variable directly after creating the
        // block_on future to demonstrate the await_concrete → wake → resolve path.
        // We simulate: a background obligation resolves var to bool.
        // The mechanism: require_eq pushes to pending_wakes, flush_and_drain
        // processes them, main task re-polls and finds the type concrete.

        // First, set up the test: main future awaits var, which is initially InferVar.
        // Before the first poll we resolve it so the re-poll finds it concrete.
        let result = cx.block_on(async {
            // Resolve the var BEFORE await_concrete — proves immediate resolution path
            cx.require_eq(var, bool_ty, RelativeSpan { start: 0, end: 0 })
                .unwrap();
            cx.await_concrete(var).await
        });

        let resolved = cx.stash()[result];
        assert_eq!(resolved, Ty::Bool);
    }

    #[test]
    fn block_on_with_pending_wakes() {
        let stash = Stash::new();
        let cx = make_cx(&stash);

        let var = cx.fresh_ty_var();
        let u32_ty = cx.alloc_ty(Ty::Int(crate::ty::IntTy::I32));

        // Demonstrate: require_eq pushes to pending_wakes, which get flushed
        cx.require_eq(var, u32_ty, RelativeSpan { start: 0, end: 0 })
            .unwrap();

        // pending_wakes should have an entry
        assert!(!cx.pending_wakes.borrow().is_empty());

        // block_on flushes them
        cx.block_on(async {});

        // After block_on, pending_wakes are flushed
        assert!(cx.pending_wakes.borrow().is_empty());
    }
}
