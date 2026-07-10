# Solving implementation sketch

This sketch makes the MVP's inference and task boundaries explicit. A
`ProofCtx` contains a stash, environment, current whiteboard frame, recursion
depth, and an explicit egraph `Version`. All egraph operations take that
version; the solver never switches a shared global `current` version while
cooperative tasks are interleaved.

Inference-variable IDs are unique across the egraph. Each variable records its
owning version, and a context can resolve only variables owned by its version
or an ancestor. Explicit versions therefore control visibility without making
the meaning of `Ty::InferVar(id)` branch-relative.

Concurrent alternatives also require a scoped runtime. A scope may spawn
futures which borrow solver state, joins them before returning, and can cancel
and drain siblings. A version is not discarded or recycled until every future
which can access it has stopped. The existing detached `'static` spawn API and
no-op wakers are insufficient for this sketch.

## Result types

`QueryResult` crosses canonical/egraph boundaries. `GoalResult` stays inside
one proof context, where equalities are implicit in that context's version.

```rust
struct QueryResult {
    /// Existential variables created while extracting this response. Their
    /// AlphaEquivParam indices follow the query input range.
    bound_vars: Slice<ResponseVarInfo>,
    value: QueryResultData,
}

struct ResponseVarInfo {
    param: AlphaEquivParam,
    kind: GenericParamKind,
    relative_universe: u32,
}

enum QueryResultData {
    Yes {
        subst: Subst,
        modulo: Goal,
    },
    Maybe {
        hints: Subst,
    },
    No,
}

enum GoalResult {
    Yes { modulo: Goal },
    Maybe,
    No,
}

/// Definite bindings or hints for existential query inputs. Rigid canonical
/// placeholders are never substitution keys.
type Subst = Slice<(AlphaEquivParam, Ptr<Ty>)>;
```

For a `Yes`, one memoized mapping from fork-local variables to response
variables is used across both `subst` and `modulo`. This preserves sharing. A
`Maybe` hint may intentionally use a fresh response variable at every divergent
occurrence; that shallow form weakens a necessary condition without inventing
one.

## Top-level entry

```rust
fn prove_query(db: &dyn Db, query: GoalQuery) -> Stashed<QueryResult> {
    let query_data = query.open_data(db);
    let state = SolverState::new(
        db,
        query_data.local_crate,
        query_data.canonical_universe,
        query_data.next_response_param,
    );
    // The arena root is immutable. The request and every canonical frame owner
    // are siblings, so a producer never becomes a child of a requester which
    // may be cancelled or transactionally updated.
    let arena_root = state.egraph.root_version();
    let request_version = state.egraph.branch(arena_root);

    state.runtime.block_on_scoped(|query_scope| async {
        let whiteboard = Whiteboard::new(
            &state,
            arena_root,
            query_scope.frame_producer_scope(),
        );
        let cx = ProofCtx::import_query(
            &state,
            &whiteboard,
            request_version,
            query_data.canonical_vars,
            query_data.assumptions_complete,
            query_data.assumptions,
        );
        let goal = cx.import_goal(query_data.goal);
        let result = prove_goal(&cx, goal, query_scope).await;

        let response = match result {
            GoalResult::No => QueryResult::no(),
            GoalResult::Maybe => cx.extract_maybe_response(),
            GoalResult::Yes { modulo } => cx.extract_yes_response(modulo),
        };
        let response = cx.stash_response(response);

        // Drain producers orphaned by cancelled alternatives. A producer joins
        // its descendants and discards its own arena-root child before it can
        // publish a result.
        whiteboard.join_or_cancel_producers().await;
        cx.egraph.discard(request_version);
        response
    })
}
```

Importing canonical variables uses their side-table metadata:

- `RigidPlaceholder` becomes a rigid placeholder at its relative universe;
- `ExistentialInput` becomes an inference variable which may be constrained;
- an existential input's relative universe is its caller's current lowered
  ceiling, never its immutable creation universe;
- an inference variable found in an assumption remains existential;
- only existential inputs are inspected when extracting a substitution.

The canonical traversal treats nested `Exists` declarations as binder-local:
it alpha-renames them through a push/pop binder map but does not add them to the
free-input side table or caller reverse map. Their IDs still occupy the query's
alpha-parameter space, so response allocation begins at
`next_response_param`. The caller-side canonicalization result separately
retains the absolute universe base used to reopen response-relative universes;
a closed query uses its caller's current universe as that base.

`GoalQuery::prove` is Salsa-tracked, so Salsa may reuse its completed output.
`SolverState` and `Whiteboard` are created only when that tracked function is
actually executed and never escape that execution. Frame production uses the
query-owned scope but not the request branch. Every frame imports its canonical
key into a fresh child of `arena_root`, and that branch is discarded after
response extraction. A shared frame therefore does not borrow inference state
or task lifetime from whichever candidate first requested it.

## Transaction primitive

All operations which can fail after writing inference state use one helper:

```rust
fn probe<T>(cx: &ProofCtx, op: impl FnOnce(&ProofCtx) -> Result<T, NoSolution>)
    -> Result<T, NoSolution>
{
    // Precondition: `cx.version` has no other live child. Collapsing a child
    // while a sibling is alive would mutate the sibling's visible ancestor
    // state underneath an in-progress computation.
    assert!(cx.egraph.has_no_live_children(cx.version));

    let child = cx.egraph.branch(cx.version);
    let probe_cx = cx.with_version(child);

    match op(&probe_cx).and_then(|value| {
        probe_cx.egraph.rebuild(child);
        probe_cx.egraph.check_occurs_lower_and_validate_universes(child)?;
        Ok(value)
    }) {
        Ok(value) => {
            // Publish child-local state and buffered wakeups atomically;
            // committed child variables keep their IDs and are re-owned by
            // the parent before the child handle is invalidated.
            cx.egraph.collapse_only_child(child, cx.version);
            Ok(value)
        }
        Err(error) => {
            cx.runtime.cancel_version(child);
            cx.egraph.discard(child);
            Err(error)
        }
    }
}
```

The precondition is why candidate siblings are never committed into their
common parent. They run concurrently, extract independent canonical responses,
and are discarded. A nested transactional probe is collapsed only into an
exclusive candidate/context version after all children of that context have
joined or been cancelled.

`try_unify`, applying a complete response substitution, and applying a complete
hint substitution each run as one probe. Recursive unification may add many
edges, but a mismatch, occurs failure such as `?X = Vec<?X>`, or universe leak
discards all of them. The primitive retains inference's `Ty::Error` recovery
semantics. Semantic revisions and inference-variable wakeups advance only when
the child collapses; path compression alone is not a semantic revision.

Universe validation reaches a fixed point before commit. An eclass uses the
minimum current ceiling of its flexible variables; flexible variables nested
in an equal structural term are transactionally lowered to that ceiling, and a
rigid placeholder above it fails. Thus delayed leaks cannot hide behind a
higher-universe flexible response variable.

## Structural decomposition

```rust
async fn prove_goal(
    cx: &ProofCtx,
    goal: Goal,
    scope: &Scope,
) -> GoalResult {
    match goal {
        Goal::Atom(atom) => prove_atom(cx, atom).await,
        Goal::All(goals) => prove_conjunction(cx, goals, scope).await,

        Goal::Exists(binder) => {
            let inner = cx.open_existential_in_version(binder);
            prove_goal(cx, inner, scope).await
        }

        Goal::Implies(assumptions, inner) => {
            let extended = cx.with_extended_env(assumptions);
            match prove_goal(&extended, *inner, scope).await {
                GoalResult::Yes { modulo } if modulo.is_trivially_true() => {
                    GoalResult::Yes { modulo }
                }
                GoalResult::Yes { modulo } => GoalResult::Yes {
                    // Preserve the environment needed to retry the residual.
                    modulo: Goal::Implies(assumptions, cx.alloc_goal(modulo)),
                },
                GoalResult::Maybe => GoalResult::Maybe,
                GoalResult::No => GoalResult::No,
            }
        }

        Goal::Maybe => GoalResult::Maybe,
    }
}
```

The MVP has no `Goal::ForAll`. Generic impl binders are instantiated while
opening a candidate clause, not proved by structural decomposition.

## Proving an atomic goal

```rust
async fn prove_atom(
    cx: &ProofCtx,
    atom: Atom,
) -> GoalResult {
    if cx.depth >= MAX_DEPTH { // MAX_DEPTH = 64
        return GoalResult::Maybe;
    }

    let (key, mapping) = cx.canonicalize_atom_and_env(atom);
    let response = match cx.whiteboard.lookup(key.clone(), cx.parent_frame) {
        Err(Cycle) => return GoalResult::No,
        Ok(Lookup::Existing(future)) => future.await,
        Ok(Lookup::Created { owner, future }) => {
            owner.spawn_canonical(
                key,
                cx.depth + 1,
                |frame_cx, atom_to_solve, frame_scope| async move {
                    solve_atom(&frame_cx, atom_to_solve, frame_scope).await
                },
            );
            future.await
        }
    };

    apply_response(cx, response, mapping)
}

fn apply_response(
    cx: &ProofCtx,
    response: Stashed<QueryResult>,
    mapping: CanonicalMapping,
) -> GoalResult {
    match response.value() {
        QueryResultData::No => GoalResult::No,

        QueryResultData::Maybe { hints } => {
            let applied = probe(cx, |probe_cx| {
                let vars = probe_cx.instantiate_response_vars(
                    response.bound_vars(),
                    mapping.universe_base(),
                );
                let hints = probe_cx.instantiate_subst(hints, mapping, vars);
                probe_cx.apply_all_subst_entries(hints)
            });
            if applied.is_ok() { GoalResult::Maybe } else { GoalResult::No }
        }

        QueryResultData::Yes { subst, modulo } => {
            let applied = probe(cx, |probe_cx| {
                let vars = probe_cx.instantiate_response_vars(
                    response.bound_vars(),
                    mapping.universe_base(),
                );
                let subst = probe_cx.instantiate_subst(subst, mapping, vars);
                probe_cx.apply_all_subst_entries(subst)?;
                Ok(probe_cx.instantiate_goal_capture_avoiding(
                    modulo,
                    mapping,
                    vars,
                ))
            });
            match applied {
                Ok(modulo) => GoalResult::Yes { modulo },
                Err(_) => GoalResult::No,
            }
        }
    }
}
```

Response variables are instantiated once per response and the same mapping is
used for every occurrence. A failed later substitution entry therefore rolls
back earlier entries and all newly allocated response variables.

`instantiate_goal_capture_avoiding` additionally maintains a push/pop map for
every nested residual binder. It freshens binder declarations and bound
occurrences independently of the free-input and response-variable mappings, so
overlapping alpha indices from separately canonicalized objects cannot capture.

`NewFrameOwner::spawn_canonical` is a whiteboard/query operation, not a method
on the requesting candidate scope. It creates a fresh child of the immutable
producer-arena root, imports the complete canonical key into that branch, and
sets `frame_cx.parent_frame` to the new frame. On success it stashes the
response, joins nested work, discards the frame branch, and only then publishes
the result. On cancellation it joins nested work and discards without
publishing. The returned `ProofFuture` is only a subscription, so dropping the
candidate which first created the frame cannot invalidate a producer still
needed by another candidate.

## Atomic dispatch

```rust
async fn solve_atom(
    cx: &ProofCtx,
    atom: Atom,
    scope: &Scope,
) -> Stashed<QueryResult> {
    match atom {
        Atom::Equals(lhs, rhs) => {
            let result = probe(cx, |probe_cx| {
                probe_cx.try_unify_recursive(lhs, rhs)
            });
            match result {
                Ok(()) => cx.stash_response(
                    cx.extract_yes_response(Goal::trivially_true())
                ),
                Err(_) => cx.stash_response(QueryResult::no()),
            }
        }

        Atom::TraitImpl { self_ty, trait_ref } => {
            if cx.contains_error(self_ty, trait_ref.args) {
                return cx.stash_response(QueryResult::yes(
                    Slice::empty(),
                    Subst::empty(),
                    Goal::trivially_true(),
                ));
            }
            solve_trait_impl(cx, self_ty, trait_ref, scope).await
        }
    }
}
```

Equality is an intrinsic atom. It does not call methods such as `trait_sym()`
which exist only for trait atoms, nor does it enumerate trait clauses.

## Solving trait alternatives

```rust
struct CanonicalHint {
    /// Binder metadata for every response variable referenced by `subst`.
    bound_vars: Slice<ResponseVarInfo>,
    subst: Subst,
}

struct PossibleAnswers {
    /// Retain all definite alternatives until final pairwise subsumption.
    yes: Vec<Stashed<QueryResult>>,
    saw_maybe: bool,
    /// `None` means no possible answer was observed. A nonempty collection may
    /// contain `CanonicalHint::empty()`, which is a real maximally weak hint.
    hint_inputs: Option<NonEmpty<CanonicalHint>>,
}

impl PossibleAnswers {
    fn observe(&mut self, result: Stashed<QueryResult>) {
        match result.value() {
            QueryResultData::No => {}
            QueryResultData::Maybe { .. } => {
                self.saw_maybe = true;
                self.add_hint_input(result.as_canonical_hint());
            }
            QueryResultData::Yes { .. } => {
                self.add_hint_input(result.as_canonical_hint());
                self.yes.push(result);
            }
        }
    }

    fn add_hint_input(&mut self, next: CanonicalHint) {
        match &mut self.hint_inputs {
            None => self.hint_inputs = Some(NonEmpty::new(next)),
            Some(inputs) => inputs.push(next),
        }
    }
}

async fn solve_trait_impl(
    cx: &ProofCtx,
    self_ty: Ptr<Ty>,
    trait_ref: TraitRef,
    scope: &Scope,
) -> Stashed<QueryResult> {
    let atom = Atom::TraitImpl { self_ty, trait_ref };
    let assembled = assemble_trait_candidates(cx, atom);
    if assembled.clauses.is_empty() && !assembled.incomplete {
        return cx.stash_response(QueryResult::no());
    }

    let mut alternatives = scope.futures_unordered();
    for candidate in assembled.clauses {
        // Every sibling gets an explicit child version. None is collapsed
        // into `cx.version`.
        let version = cx.egraph.branch(cx.version);
        alternatives.spawn(try_candidate(cx.with_version(version), atom, candidate, scope));
    }

    let mut possible = PossibleAnswers {
        yes: Vec::new(),
        // Incomplete candidate sources are an unconstrained possibility.
        saw_maybe: assembled.incomplete,
        hint_inputs: assembled
            .incomplete
            .then(|| NonEmpty::new(CanonicalHint::empty())),
    };

    while let Some(result) = alternatives.next().await {
        if result.is_unconditional_yes() {
            // Empty subst + trivially true modulo. Cancellation must finish
            // before any sibling version is discarded.
            alternatives.cancel_and_join().await;
            return result;
        }
        // Never return early for `Maybe`, including empty hints. A pending
        // sibling may still produce an unconditional Yes.
        possible.observe(result);
    }

    finalize_answers(cx, possible)
}
```

`try_candidate` stashes its response before dropping its branch, so the result
does not borrow branch-local inference state. A cancellation guard discards the
branch after its future has stopped.

### Order-independent finalization

```rust
fn finalize_answers(cx: &ProofCtx, possible: PossibleAnswers)
    -> Stashed<QueryResult>
{
    if possible.yes.is_empty() {
        return if possible.saw_maybe {
            let hint = cx.merge_hint_inputs(
                possible.hint_inputs.expect("Maybe contributes a hint input")
            );
            cx.stash_response(QueryResult::maybe(hint.bound_vars, hint.subst))
        } else {
            cx.stash_response(QueryResult::no())
        };
    }

    // Compare every pair after all results arrive. Keep exactly those answers
    // for which no strictly more-general answer exists. Alpha-equivalent or
    // mutually-subsuming responses use a stable canonical tie-break.
    let nondominated = cx.compute_nondominated_answers(possible.yes);

    // The completion loop normally returns this immediately, but keep
    // finalization total and consistent if results were supplied by a test or
    // a non-streaming caller.
    if let Some(unconditional) = nondominated
        .iter()
        .find(|answer| answer.is_unconditional_yes())
    {
        return unconditional.clone();
    }

    if !possible.saw_maybe && nondominated.len() == 1 {
        return nondominated[0].clone();
    }

    let hint = cx.merge_hint_inputs(
        possible.hint_inputs.expect("every possible answer contributes a hint")
    );
    cx.stash_response(QueryResult::maybe(hint.bound_vars, hint.subst))
}
```

Consequently:

- all `No` alternatives produce `No`;
- one most-general `Yes`, with no `Maybe`, produces that `Yes`;
- all-`Maybe` alternatives produce `Maybe`, never `No`;
- a `Maybe` prevents a conditional `Yes` from becoming definite;
- incomparable `Yes` answers produce `Maybe`;
- a later general answer can dominate any number of earlier specific answers;
- arrival order cannot change the result.

The hint input collection includes every non-`No` alternative before dominance
filtering. Each input carries the response binder which gives meaning to the
variables in its substitution. `merge_hint_inputs` alpha-renames those binders
apart, sorts the inputs canonically, and performs one shallow n-ary
anti-unification. It intersects substitution keys: if any possible answer omits
a key, that caller input is unconstrained and the key disappears. Only values
for keys present in every possible answer are structurally anti-unified.

Every fresh divergent witness is assigned the relative universe of its
substitution key. Fresh-per-occurrence witnesses are not shared across keys,
so this is deterministic and maximally general without making the key unable
to bind the result. The merged value is leak-checked against that key before
being returned.

The merged hint is normalized before it is returned. An entry whose right-hand
side is one otherwise-unused fresh response variable is existentially
tautological and is removed; unused response variables are then pruned and the
remainder is renumbered canonically. Thus `?X = u32` versus `?X = i32` produces
an empty hint, while `?X = Vec<u32>` versus `?X = Vec<i32>` retains the useful
outer constraint `?X = Vec<?A>`.

## Directional subsumption

For answers

```text
A = exists V_A. Yes { subst: SA, modulo: MA }
B = exists V_B. Yes { subst: SB, modulo: MB }
```

`A.subsumes(B)` means `Cond(B) => Cond(A)`: `A` is at least as general as
`B`. The full relation is

```text
forall V_B. exists V_A. (SB && MB) => (SA && MA)
```

The MVP recognizes a sound subset of that relation:

1. Create an isolated comparison version.
2. Open `B`'s response variables as rigid placeholders, apply `SB` as the
   antecedent, and then freeze all antecedent/caller equivalence classes.
3. Open `A`'s response variables as existential witnesses.
4. Directionally match each equality required by `SA`. Only `A`'s witnesses
   may be bound; the matcher may not refine a frozen antecedent or caller
   class. Instantiate every response variable at its lowered universe ceiling;
   consequent witnesses may be lowered but may not capture an inaccessible
   antecedent placeholder.
5. Substitute and canonicalize flat residual conjunctions. Require every
   conjunct in `MA` to occur in `MB`; matching may instantiate only `A`'s
   witnesses. This recognizes `X && Y => X`.
6. Discard the comparison version regardless of the result.

Ordinary symmetric `try_unify` is not valid for steps 4 and 5 because it could
bind the antecedent to make an implication appear true. In particular:

```text
unconditional                              subsumes [?X = u32]
[?X = u32]                                does not subsume unconditional
exists<T> [?X = (T, T)]                   preserves the repeated T witness
```

Cases requiring proof of a new trait fact are reported as non-subsuming by the
MVP. This may retain extra answers and produce `Maybe`, but cannot unsafely
discard an applicable answer.

## Trying one candidate

```rust
async fn try_candidate(
    candidate_cx: ProofCtx,
    atom: Atom,
    clause: Clause,
    scope: &Scope,
) -> Stashed<QueryResult> {
    let result = async {
        // Impl/assumption binders are fresh for this candidate only.
        let clause = candidate_cx.open_clause_binder(clause);

        // This nested probe is exclusive under the candidate version and
        // rolls back partial recursive unification on failure.
        probe(&candidate_cx, |probe_cx| {
            probe_cx.try_unify_atoms_directionally(clause.head, atom)
        })?;

        let body = candidate_cx.substitute_goals(clause.conditions);
        Ok(prove_conjunction(&candidate_cx, body, scope).await)
    }.await;

    let response = match result {
        Err(_) | Ok(GoalResult::No) => QueryResult::no(),
        Ok(GoalResult::Maybe) => candidate_cx.extract_maybe_response(),
        Ok(GoalResult::Yes { modulo }) => {
            candidate_cx.extract_yes_response(modulo)
        }
    };

    let response = candidate_cx.stash_response(response);
    candidate_cx.cancel_and_discard_after_children_joined().await;
    response
}
```

Head matching may bind fresh clause variables and existential goal inputs, but
never rigid query placeholders. The candidate branch is always extracted and
discarded; even a successful candidate is not collapsed into the atom's common
parent.

## Extracting responses

Definite extraction walks all response components with one folder:

```rust
fn extract_yes_response(cx: &ProofCtx, modulo: Goal) -> QueryResult {
    // Decide how every reachable egraph class is represented before folding.
    // Pure flexible-variable classes prefer the lowest canonical existential
    // input; a class with a rigid placeholder or concrete/structured member
    // prefers its canonical substituted rigid/structural form. This must not
    // depend on union-find roots.
    let projection = ResponseProjection::compute(cx, &modulo);
    let mut folder = ResponseFolder {
        projection,
        local_to_response: FxHashMap::default(),
        bound_vars: Vec::new(),
        mode: PreserveSharing,
    };

    let subst = cx
        .projected_input_equalities(&folder.projection)
        .map(|(input, ty)| (input, folder.fold_ty(ty)))
        .filter(|(input, ty)| !cx.is_identity_binding(*input, *ty))
        .collect();
    let modulo = folder.fold_goal(cx.substitute(modulo));
    let (bound_vars, subst, modulo) =
        folder.finish_and_prune(subst, modulo);

    QueryResult::yes(bound_vars, subst, modulo)
}
```

`ResponseFolder` maps each unresolved fork-local variable once and records the
variable's lowered current universe ceiling relative to the query universe
base, not merely its creation universe. The same local variable in
`subst` and `modulo`, or twice in one type, therefore maps to the same response
parameter.

When folding `modulo`, it also uses a binder stack to alpha-rename nested
`Exists` declarations and their occurrences without treating them as response
existentials. `finish_and_prune` preserves those binder scopes while
renumbering only the response-bound variables it owns.

`ResponseProjection` first eliminates pure candidate-local aliases through a
canonical query input. If candidate `T` is equated only with input `?X`, every
occurrence of `T` in the residual folds to `?X` and no response variable or
substitution entry is emitted. If two inputs meet through `T`, the lowest input
is the representative and the other gets a deterministic equality. A
structural class such as `?X = Vec<T>` retains `Vec` and allocates a response
variable for unresolved `T`. This is what makes a blanket generic proof
extract as an unconditional `Yes` without losing real outer-type constraints.

`extract_maybe_response` uses the same input-key filtering but a
`FreshPerOccurrence` folder. The later anti-unifier also creates a fresh
response variable for each divergent occurrence. This is the only place where
`(T, T)` may deliberately become `(?A, ?B)`. Final hint normalization removes
bare fresh witnesses which impose no constraint and prunes any response-bound
variables made unused by that removal.

## Conjunction fixpoint

```rust
async fn prove_conjunction(
    cx: &ProofCtx,
    goals: Vec<Goal>,
    scope: &Scope,
) -> GoalResult {
    // The guard owns the child version. Its cancellation path first cancels
    // and joins the nested scope, then discards the child.
    let mut transaction = ScopedVersionTransaction::branch(
        scope,
        cx.egraph,
        cx.version,
    );
    let child_cx = cx.with_version(transaction.version());
    let result = transaction
        .run(|child_scope| {
            prove_conjunction_in_version(&child_cx, goals, child_scope)
        })
        .await;

    match result {
        GoalResult::No => {
            transaction.discard();
            GoalResult::No
        }
        possible => {
            // `run` has joined every nested task. The conjunction child is
            // again exclusive, so its committed hints/equalities can publish
            // atomically to the caller.
            transaction.collapse_only_child();
            possible
        }
    }
}

async fn prove_conjunction_in_version(
    cx: &ProofCtx,
    goals: Vec<Goal>,
    scope: &Scope,
) -> GoalResult {
    #[derive(Clone, PartialEq, Eq)]
    struct Attempt {
        goal: Goal,
        /// Changes only for committed equality/bound facts, not compression.
        revision: SemanticRevision,
    }

    struct PendingGoal {
        current: Goal,
        last_attempt: Option<Attempt>,
    }

    let mut pending: Vec<_> = goals
        .into_iter()
        .map(|current| PendingGoal { current, last_attempt: None })
        .collect();

    loop {
        let mut attempted_any = false;
        let mut next = Vec::new();

        for mut item in pending {
            let substituted = cx.substitute(item.current);
            let attempt = Attempt {
                goal: substituted,
                revision: cx.semantic_revision(),
            };

            if item.last_attempt.as_ref() == Some(&attempt) {
                item.current = substituted;
                next.push(item);
                continue;
            }

            attempted_any = true;
            item.last_attempt = Some(attempt);

            match prove_goal(cx, substituted, scope).await {
                GoalResult::Yes { modulo } if modulo.is_trivially_true() => {}

                GoalResult::Yes { modulo } => {
                    // The solver already produced this residual at the current
                    // post-proof state. Mark the normalized residual itself as
                    // attempted now; do not immediately prove an ever-changing
                    // residual chain at the same parent depth. A later sibling
                    // semantic update changes the revision and enables retry.
                    let residual = cx.substitute(modulo);
                    item.current = residual.clone();
                    item.last_attempt = Some(Attempt {
                        goal: residual,
                        revision: cx.semantic_revision(),
                    });
                    next.push(item);
                }

                GoalResult::Maybe => {
                    // Hints are already transactionally applied. A committed
                    // hint advances the revision and triggers one useful retry.
                    item.current = substituted;
                    next.push(item);
                }

                GoalResult::No => return GoalResult::No,
            }
        }

        pending = next;
        if pending.is_empty() {
            return GoalResult::Yes { modulo: Goal::trivially_true() };
        }

        if !attempted_any {
            let residuals = pending
                .into_iter()
                .map(|item| cx.substitute(item.current))
                .collect();
            return GoalResult::Yes { modulo: Goal::All(residuals) };
        }
    }
}
```

The transaction is also the failure boundary for the sequence: a later `No`
cannot leak equalities committed by an earlier conjunct. The key termination
detail inside it is that a partial `Yes` marks its returned normalized residual
as already attempted at the post-proof semantic revision. Even a rule which
changes `R<T>` into `R<Vec<T>>` on every attempt therefore yields a residual
instead of looping at one parent depth. If another conjunct later constrains a
variable, the substituted goal or semantic revision changes and enables the
needed retry.

## Candidate assembly

```rust
struct AssembledCandidates {
    clauses: Vec<Clause>,
    /// True when deferred candidate metadata prevents definitive failure.
    incomplete: bool,
}

enum ImplCandidate {
    Eligible(Clause),
    Irrelevant,
    /// Potentially relevant, but its complete applicability contract cannot be
    /// represented by the MVP.
    Incomplete,
}

fn assemble_trait_candidates(
    cx: &ProofCtx,
    atom: Atom,
) -> AssembledCandidates {
    let mut candidates = Vec::new();

    // 1. Trait-only assumptions are considered first. Flatten Assumption::All;
    // translate TraitImpl to an empty-body clause and Implies to a body/head
    // trait clause. Equality is goal-only in the MVP.
    candidates.extend(cx.env.flattened_clauses_matching(atom));

    let floundered_impl_search = match atom {
        Atom::TraitImpl { self_ty, .. } => cx.is_bare_infer_var(self_ty),
        Atom::Equals(..) => false,
    };
    let mut incomplete = !cx.assumptions_complete
        || floundered_impl_search
        || cx.trait_candidate_sources_incomplete(atom);

    // 2. Do not enumerate impls to guess a bare receiver. Environment clauses
    // above remain usable, and the omitted impl search becomes a synthetic
    // possible answer so failure cannot become `No`.
    if !floundered_impl_search {
        for impl_sym in local_impls(cx.db, cx.local_crate) {
            match cx.classify_positive_trait_impl(impl_sym, atom) {
                ImplCandidate::Eligible(clause) => candidates.push(clause),
                ImplCandidate::Irrelevant => {}
                ImplCandidate::Incomplete => incomplete = true,
            }
        }
    }

    AssembledCandidates {
        clauses: candidates,
        incomplete,
    }
}
```

`classify_positive_trait_impl` returns a clause only when both the impl and the
referenced local trait signature are `SolverEligibility::Eligible`. Its body
contains the opened impl where-predicates and instantiated local trait
where-predicates. A potentially relevant `Unsupported` signature is
`Incomplete`, not `Irrelevant`; it contributes the same synthetic possibility
as deferred metadata. A local impl of an external trait is likewise deferred
until `TcxDb` exposes the defining predicates, which are never assumed empty.
If no environment candidate proves such a goal unconditionally, finalization
observes a synthetic `Maybe` rather than concluding `No` from an intentionally
incomplete candidate set.

A bare flexible self type follows the same incomplete-source path for impl
enumeration, but only after environment clauses have been assembled. Therefore
an explicit `?X: Trait` assumption can prove the identical goal immediately;
without such a proof, skipped impl enumeration contributes an empty-hint
`Maybe` and the caller retains the goal for retry.

`assumptions_complete = false` also contributes that empty synthetic
possibility. Known represented facts still participate and an unconditional
one wins, but the omitted unsupported environment cannot be mistaken for an
exhaustive failure.

No external metadata, builtin/auto-trait rule, negative impl, normalization
rule, or method lookup participates in MVP assembly.
