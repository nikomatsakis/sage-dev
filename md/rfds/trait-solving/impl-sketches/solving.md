# Solving implementation sketch

This section presents the solver algorithm as pseudo-code. The `cx` parameter represents a shared proof context with access to the egraph, stash, environment, clause database, and a method for spawning/awaiting atomic goal tasks via the whiteboard.

## Result types

There are two result types. `QueryResult` crosses egraph boundaries (carries a substitution or hints extracted from an egraph diff). `GoalResult` flows within the proof tree where equalities are implicit in the shared egraph.

```rust
/// Result that crosses an egraph boundary (returned from whiteboard entries,
/// from try_candidate, and from prove_query). Carries extracted subst/hints.
///
/// Wrapped in a Binder that declares any fresh existential variables introduced
/// by extract_subst (replacing fork-local vars with fresh existentials).
/// The caller opens the binder by allocating inference variables for each bound param.
/// The binder covers the entire result — value, subst, and modulo may all
/// reference the fresh existentials introduced by extract_subst.
type QueryResult = Binder<QueryResultData>;

enum QueryResultData {
    Yes {
        value: ProofValue,
        subst: Subst,
        modulo: Goal,
    },
    Maybe {
        hints: Subst,   // anti-unified, possibly inexact
    },
    No,
}

/// Result flowing within a single egraph context. Equalities are implicit
/// in the egraph — no subst needed.
enum GoalResult {
    Yes {
        value: ProofValue,
        modulo: Goal,
    },
    Maybe,
    No,
}

/// A substitution: bindings from parent-scope variables to types/lifetimes.
/// May reference variables bound by the enclosing Binder.
/// In Yes: definite equalities learned during the proof.
/// In Maybe: anti-unified hints (rigid structure with fresh existentials).
///           Anti-unification is allowed to be inexact (e.g., producing
///           Vec<(?A, ?B)> where Vec<(?A, ?A)> would be more precise) —
///           this is conservative (weaker hints, never wrong).
type Subst = Slice<(AlphaEquivParam, GenericArg)>;
```

## Top-level entry

```rust
/// Entry point from GoalQuery::prove. Creates the internal machinery,
/// runs the proof, extracts the result.
fn prove_query(db: &dyn Db, query: GoalQuery) -> Stashed<QueryResult> {
    let cx = ProofCtx::new(db, query);
    let goal = cx.import_goal(&query.goal());
    let env = cx.import_assumptions(&query.assumptions());

    let result = cx.block_on(prove_goal(&cx, goal, &env));

    // Extract the substitution from the top-level egraph: equalities learned
    // about the query's canonical variables during proof.
    let subst = cx.extract_subst();

    match result {
        GoalResult::No => cx.stash_result(QueryResult::No),
        GoalResult::Maybe => {
            cx.stash_result(QueryResult::Maybe { hints: subst })
        }
        GoalResult::Yes { value, modulo } => {
            cx.stash_result(QueryResult::Yes { value, subst, modulo })
        }
    }
}
```

## Structural decomposition

```rust
/// Prove a (possibly compound) goal. Decomposes structure, delegates atoms.
/// Returns GoalResult — equalities are written directly into cx's egraph.
async fn prove_goal(cx: &ProofCtx, goal: Goal, env: &Env) -> GoalResult {
    match goal {
        Goal::Atom(atom) => prove_atom(cx, atom, env).await,

        Goal::All(goals) => prove_conjunction(cx, goals, env).await,

        Goal::Exists(binder) => {
            let (inner_goal, _vars) = cx.open_existential(binder);
            prove_goal(cx, inner_goal, env).await
        }

        Goal::ForAll(_) => {
            // Deferred — not needed for MVP.
            GoalResult::Maybe
        }

        Goal::Implies(assumptions, inner) => {
            let extended_env = env.extend(assumptions);
            prove_goal(cx, *inner, &extended_env).await
        }

        Goal::Maybe => GoalResult::Maybe,
    }
}
```

## Proving an atomic goal

```rust
/// Prove a single atom. Checks for insta-flounder, then delegates to the
/// whiteboard (which deduplicates and spawns the actual solving task).
/// Applies the QueryResult back into this cx's egraph and returns GoalResult.
async fn prove_atom(cx: &ProofCtx, atom: Atom, env: &Env) -> GoalResult {
    // Insta-flounder: self-type is a bare inference variable.
    if atom.self_type_is_bare_infer_var(cx) {
        return GoalResult::Maybe;
    }

    // Canonicalize and submit to the whiteboard.
    // The whiteboard returns a QueryResult (from a separate egraph).
    let (key, mapping) = cx.canonicalize_atom(atom, env);
    let query_result = cx.await_or_spawn(key, |inner_cx| {
        solve_atom(inner_cx, key.atom(), key.env())
    }).await;

    // Apply the QueryResult into our egraph and convert to GoalResult.
    match query_result {
        QueryResult::No => GoalResult::No,

        QueryResult::Maybe { hints } => {
            // Hints are necessary conditions — apply them to our egraph.
            let hints = cx.instantiate_subst(hints, &mapping);
            if !cx.apply_subst(hints) {
                return GoalResult::No;
            }
            GoalResult::Maybe
        }

        QueryResult::Yes { value, subst, modulo } => {
            // Apply the substitution into our egraph.
            let subst = cx.instantiate_subst(subst, &mapping);
            if !cx.apply_subst(subst) {
                return GoalResult::No;
            }
            let value = cx.instantiate_value(value, &mapping);
            let modulo = cx.instantiate_goal(modulo, &mapping);
            GoalResult::Yes { value, modulo }
        }
    }
}
```

## Solving an atom (the real work)

```rust
/// Internal state for tracking the best answer as alternatives complete.
enum AlternativeState {
    /// No successful candidate yet.
    NotProven,
    /// Exactly one successful candidate (or multiple compatible ones).
    Proven(QueryResult),
    /// Multiple incompatible successful candidates — ambiguous.
    Ambiguous,
}

/// The task spawned by the whiteboard to actually solve an atomic goal.
/// Runs in its own egraph. Assembles candidates, explores alternatives
/// concurrently, refines the answer as results arrive.
/// Returns QueryResult (extracted from its egraph at the boundaries).
async fn solve_atom(cx: &ProofCtx, atom: Atom, env: &Env) -> QueryResult {
    let candidates = cx.assemble_candidates(atom, env);

    if candidates.is_empty() {
        return QueryResult::No;
    }

    // Spawn all alternatives concurrently (each forks the egraph).
    let mut futures = FuturesUnordered::new();
    for clause in &candidates {
        futures.push(try_candidate(cx, atom, clause, env));
    }

    // Pull results as they arrive, refining our state incrementally.
    let mut state = AlternativeState::NotProven;
    // Parallel hint accumulator: anti-unifies across ALL successful results.
    let mut hints = Subst::empty();

    while let Some(result) = futures.next().await {
        match result {
            QueryResult::No => continue,

            QueryResult::Maybe { hints: h } => {
                hints = cx.anti_unify_subst(hints, h);
                if hints.is_empty() {
                    return QueryResult::Maybe { hints: Subst::empty() };
                }
            }

            QueryResult::Yes { ref subst, ref modulo, .. }
                if modulo.is_trivially_true() && subst.is_empty() =>
            {
                // Fully proven with no caller-variable bindings — immediate winner.
                // A non-empty substitution still needs sibling alternatives to run:
                // one candidate may learn ?X = u32 while another learns ?X = i32.
                return result;
            }

            QueryResult::Yes { ref subst, .. } => {
                hints = cx.anti_unify_subst(hints, subst);

                match state {
                    AlternativeState::NotProven => {
                        state = AlternativeState::Proven(result);
                    }
                    AlternativeState::Proven(ref prev) => {
                        if prev.subsumes(&result) {
                            // prev's conditions are weaker, so it covers result.
                        } else if result.subsumes(prev) {
                            // result's conditions are weaker, so it covers prev.
                            state = AlternativeState::Proven(result);
                        } else {
                            // Incompatible — transition to ambiguous.
                            state = AlternativeState::Ambiguous;
                        }
                    }
                    AlternativeState::Ambiguous => {
                        if hints.is_empty() {
                            return QueryResult::Maybe { hints: Subst::empty() };
                        }
                    }
                }
            }
        }
    }

    match state {
        AlternativeState::NotProven => QueryResult::No,
        AlternativeState::Proven(result) => result,
        AlternativeState::Ambiguous => QueryResult::Maybe { hints },
    }
}
```

## Subsuming alternatives

Subsumption compares the **conditions under which a `Yes` answer is valid**. It is not a syntactic subset check over `modulo` alone, because the answer also has a substitution and may be wrapped in a binder that introduces fresh existentials.

For two successful answers:

```text
A = exists V_A. Yes { value: VA, subst: SA, modulo: MA }
B = exists V_B. Yes { value: VB, subst: SB, modulo: MB }
```

Define each answer's applicability condition as:

```text
Cond(A) = exists V_A. SA && MA
Cond(B) = exists V_B. SB && MB
```

`A.subsumes(B)` means `Cond(B) => Cond(A)`: every caller state that can use `B` can also use `A`, producing the same proof value. Equivalently, `A`'s conditions are no stronger than `B`'s conditions, so `A` is the more general answer for the disjunction.

With explicit quantifiers:

```text
A.subsumes(B) =
    forall V_B. exists V_A.
        (SB && MB) => (SA && MA)

B.subsumes(A) =
    forall V_A. exists V_B.
        (SA && MA) => (SB && MB)
```

So:

```text
A.subsumes(B) && !B.subsumes(A)  => A is more general
B.subsumes(A) && !A.subsumes(B)  => B is more general
A.subsumes(B) &&  B.subsumes(A)  => equivalent; keep either
!A.subsumes(B) && !B.subsumes(A) => incompatible / ambiguous
```

The best implementation is to encode the implication check as another trait-solving problem:

1. Open the antecedent answer's binder variables universally.
2. Apply the antecedent substitution and assume the antecedent residual goals.
3. Open the consequent answer's binder variables existentially.
4. Prove the consequent substitution equalities, consequent residual goals, and proof-value equality under those assumptions.

This is precise but recursive: subsumption itself invokes the solver. It should reuse the same depth limit / cycle policy as normal proof search.

The MVP uses a conservative approximation that still handles the common `X && Y => X` case:

1. **Open the antecedent in a comparison egraph.** To test `A.subsumes(B)`, open `B`'s binder variables as fresh comparison variables and apply `SB`.
2. **Instantiate the consequent existentials.** Open `A`'s binder variables as fresh comparison variables. These are witnesses the check may choose while trying to prove `A`.
3. **Check proof values with type equality.** `ProofValue::True` is trivially equal. For type-valued results, use the same structural unification machinery as inference: egraph `find`/`union` plus skeleton decomposition.
4. **Check substitutions with type equality.** For every caller variable bound by `SA`, compare `SA[var]` with the value for `var` after applying `SB`. The comparison may bind `A`'s existential witnesses, but it may not change `B`'s already-assumed caller bindings.
5. **Check residual implication by subset.** Normalize `MA` and `MB` into flat conjunctions, substitute through the comparison egraph, canonicalize each conjunct, and require `conjuncts(MA) ⊆ conjuncts(MB)`. This recognizes `X && Y => X`. `All([])` is the empty set and is therefore implied by anything.
6. If any step fails, return "does not subsume".

The approximation produces more `Maybe` results than a full solver-backed implication check, but it is sound: it never drops an alternative unless it has proven or structurally recognized that another answer is at least as general.

Type equality should reuse the existing inference machinery rather than inventing a second comparer. Today `InferCtx::require_eq` performs structural unification and reports diagnostics on mismatch. The solver should factor out a diagnostic-free primitive, roughly:

```rust
enum TryUnify {
    Ok,
    Mismatch,
}

fn try_unify(
    egraph: &mut VersionedEGraph<'db>,
    stash: &mut Stash,
    a: Ptr<Ty<'db>>,
    b: Ptr<Ty<'db>>,
) -> TryUnify;
```

`InferCtx::require_eq` can call this helper and turn `Mismatch` into a `TypeError`; the solver and subsumption code can use it without manufacturing diagnostics.

Examples:

```text
A: Yes { subst: [],         modulo: All([]) }
B: Yes { subst: [?X = u32], modulo: All([]) }

A subsumes B. If the goal is unconditionally true, the answer that also binds
?X = u32 is just a more specific way to prove it.
```

```text
A: Yes { subst: [?X = u32], modulo: All([]) }
B: Yes { subst: [?X = i32], modulo: All([]) }

Neither subsumes the other. The alternatives learned incompatible caller
bindings, so the aggregate answer must become Maybe with anti-unified hints.
```

```text
A: exists<T> Yes { subst: [?X = Vec<T>], modulo: T: Clone }
B:           Yes { subst: [?X = Vec<u32>], modulo: All([]) }

A subsumes B if the comparison can show `u32: Clone`. A full solver-backed
subsumption check can prove that. The MVP subset approximation cannot, so it
treats the answers as incompatible/ambiguous rather than unsafely dropping one.
```

```text
A: Yes { subst: [?X = Vec<u32>], modulo: T: Clone }
B: Yes { subst: [?X = Vec<u32>], modulo: All([T: Clone, T: Debug]) }

A subsumes B under the MVP check because both substitutions are equal and
`{T: Clone} ⊆ {T: Clone, T: Debug}`.
```

```rust
/// Try one candidate clause against the goal atom.
/// Forks the egraph, so returns a QueryResult (extracted from the fork).
async fn try_candidate(
    cx: &ProofCtx,
    atom: Atom,
    clause: &Clause,
    env: &Env,
) -> QueryResult {
    // Fork the egraph for this alternative.
    let fork = cx.fork_egraph();

    // Unify the clause head with the goal.
    if !fork.unify(clause.head, atom) {
        return QueryResult::No;
    }

    // Prove the clause body as a conjunction in the forked egraph.
    let body_goals = fork.substitute_body(&clause.body);
    let goal_result = prove_conjunction(&fork, body_goals, env).await;

    // Extract the substitution from the fork's egraph diff.
    let subst = fork.extract_subst();

    match goal_result {
        GoalResult::No => QueryResult::No,

        GoalResult::Maybe => {
            QueryResult::Maybe { hints: subst }
        }

        GoalResult::Yes { modulo, .. } => {
            let value = fork.extract_value(atom);
            QueryResult::Yes { value, subst, modulo }
        }
    }
}
```

## Extracting a substitution from an egraph fork

```rust
/// Walks the diff between this fork and its parent. For each variable
/// from the parent scope that was unified with something in this fork:
/// - Take the canonical form of that variable in this fork.
/// - Replace any inference variables local to this fork with fresh
///   existentials (non-repeating). This gives "the rigid structure learned"
///   with unknowns for the undetermined parts.
///
/// Example: parent had ?X, fork unified ?X = Vec<?T> (where ?T is
/// fork-local). Result: ?X = Vec(?fresh).
fn extract_subst(fork: &EGraphFork) -> Subst {
    let mut subst = Vec::new();
    for (var, canonical) in fork.parent_var_equalities() {
        let ty = fork.replace_local_vars_with_fresh(canonical);
        subst.push((var, ty));
    }
    Subst::from(subst)
}
```

## Conjunction

```rust
/// Prove a conjunction of goals sequentially to a fixpoint. Returns GoalResult —
/// equalities are written directly into cx's egraph.
///
/// When a sub-goal returns a QueryResult (via prove_atom), its subst/hints
/// are applied to the egraph before continuing to the next sub-goal.
async fn prove_conjunction(
    cx: &ProofCtx,
    goals: Vec<Goal>,
    env: &Env,
) -> GoalResult {
    struct PendingGoal {
        original: Goal,
        last_substituted: Option<Goal>,
    }

    let mut pending: Vec<PendingGoal> = goals
        .into_iter()
        .map(|goal| PendingGoal {
            original: goal,
            last_substituted: None,
        })
        .collect();

    loop {
        let mut changed = false;
        let mut next_pending = Vec::new();

        for mut pending_goal in pending {
            // Substitute known values from the egraph before proving.
            let substituted = cx.substitute(pending_goal.original);

            if pending_goal.last_substituted.as_ref() == Some(&substituted) {
                next_pending.push(pending_goal);
                continue;
            }

            changed = true;
            pending_goal.last_substituted = Some(substituted);

            let result = prove_goal(cx, substituted, env).await;

            match result {
                GoalResult::Yes { modulo, .. } if modulo.is_trivially_true() => {
                    // Fully proven — remove from pending.
                }
                GoalResult::Yes { modulo, .. } => {
                    // Partially proven — prove the residual on the next pass.
                    next_pending.push(PendingGoal {
                        original: modulo,
                        last_substituted: None,
                    });
                }
                GoalResult::No => {
                    // One conjunct disproven — whole conjunction fails.
                    return GoalResult::No;
                }
                GoalResult::Maybe => {
                    // Hints/equalities, if any, are already in the egraph from
                    // prove_atom. Keep the substituted goal as a residual and
                    // retry only if later substitution changes it.
                    next_pending.push(pending_goal);
                }
            }
        }

        pending = next_pending;

        if pending.is_empty() {
            return GoalResult::Yes {
                value: ProofValue::True,
                modulo: Goal::All(&[]),
            };
        }

        if !changed {
            let residuals = pending
                .into_iter()
                .map(|pending_goal| {
                    pending_goal
                        .last_substituted
                        .unwrap_or(pending_goal.original)
                })
                .collect();
            return GoalResult::Yes {
                value: ProofValue::True,
                modulo: Goal::All(&residuals),
            };
        }
    }
}
```

## Candidate assembly

```rust
/// Assemble all clauses that could potentially prove this atom.
fn assemble_candidates(cx: &ProofCtx, atom: Atom, env: &Env) -> Vec<Clause> {
    let mut candidates = Vec::new();

    // 1. Impls from the clause database (indexed by trait + self-type head).
    candidates.extend(cx.db.impls_for_trait(atom.trait_sym()));

    // 2. Assumptions from the environment.
    for assumption in env.clauses_matching(atom) {
        candidates.push(assumption.as_clause());
    }

    candidates
}
```
