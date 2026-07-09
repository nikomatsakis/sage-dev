# Implementation plan and status

Each step should leave the codebase building and should add focused tests for the behavior it introduces. The first pass targets the MVP contract from the RFD, not the full future solver.

## Code organization

The implementation should fit into the existing `sage-ir/src/check/` split:

```text
crates/sage-ir/src/check/
  infer/
    egraph.rs       existing VersionedEGraph
    skeleton.rs     existing Ty decomposition/recomposition
    unify.rs        new diagnostic-free structural unification helper
    ...
  solve/
    mod.rs          public solver module surface
    goal.rs         Goal, Atom, Assumption, Clause, ProofValue
    result.rs       QueryResult, GoalResult, Subst, result binders
    canonical.rs    canonicalize/instantiate goals and results
    ctx.rs          ProofCtx: egraph, stash, env, depth, executor hooks
    whiteboard.rs   per-query dedup table and dependency-stack cycle checks
    prove.rs        prove_goal, prove_atom, solve_atom orchestration
    clauses.rs      environment and impl clause assembly
    merge.rs        alternative merging, Maybe/hint aggregation
    anti_unify.rs   anti-unification helpers for substitutions/hints
    subsume.rs      conservative MVP subsumption check
  infer_ctx.rs      existing body-checking context; later calls solver
  expr.rs           later emits solver obligations from body checking
```

`check/mod.rs` should grow `pub mod solve;` when the solver module is introduced. Keep the first implementation inside `sage-ir`; do not add a new crate unless compile times or dependency boundaries force it later.

### Module responsibilities

- **`check/infer/unify.rs`** owns structural type unification over `Ty`. It reuses `VersionedEGraph`, `skeleton::{decompose, recompose}`, and `Stash`. It does not know about goals, traits, clauses, diagnostics, or solver results. `InferCtx::require_eq` should become a diagnostic-producing wrapper around this helper.
- **`check/solve/goal.rs`** owns the logical vocabulary. It should be mostly data definitions and small constructors/helpers such as flattening `Goal::All`.
- **`check/solve/result.rs`** owns boundary vs internal result types and helpers for inspecting `subst`, `modulo`, and `ProofValue`.
- **`check/solve/canonical.rs`** owns alpha-equivalence boundaries: mapping caller variables to `AlphaEquivParam`, opening result binders, and copying/stashing results across contexts.
- **`check/solve/ctx.rs`** owns proof-local state. It should be the solver analogue of `InferCtx`, but narrower: database handle, working stash, local egraph, environment, whiteboard handle, recursion depth, and helper methods like `fresh_existential`.
- **`check/solve/whiteboard.rs`** owns the active proof tree for atomic proofs. For the MVP, do not make it a cache: include the parent proof frame in the entry key, walk the parent chain to detect cycles, deduplicate requests with the same key and parent, and keep entries until the enclosing `GoalQuery::prove` finishes.
- **`check/solve/prove.rs`** owns the recursive algorithm: structural goal decomposition, atomic goal proof, alternative spawning, and conjunction fixpoint. It should call into other modules rather than accumulating data-model code.
- **`check/solve/clauses.rs`** owns candidate assembly. Start with environment clauses; later add local impl clauses from trait-system signatures. Linear scan belongs here until indexing is needed.
- **`check/solve/anti_unify.rs`** owns hint construction. It should not know about whiteboards or candidate assembly.
- **`check/solve/subsume.rs`** owns `A.subsumes(B)` for `QueryResult`s. It uses `check/infer/unify.rs` and goal canonicalization, but not the full proof search in the MVP.
- **`check/solve/merge.rs`** owns disjunction policy: early winner detection, calling subsumption to keep the more general result, and calling anti-unification when ambiguity remains.

Tests should follow those boundaries. Put narrow unit tests next to the module they exercise, then add integration-style solver tests under `crates/sage-ir/tests/` once `GoalQuery::prove` exists.

### Step 1: Diagnostic-free structural unification

Factor the structural unification logic behind `InferCtx::require_eq` into a helper that can be used without reporting diagnostics. This is structural unification over the current `Ty` representation: no type-alias expansion, no associated type projection normalization, and no semantic equality beyond the egraph plus skeleton decomposition.

- [ ] Add a `try_unify`-style primitive over `VersionedEGraph` + `Stash`.
- [ ] Update `InferCtx::require_eq` to call the helper and convert mismatch into `TypeError`.
- [ ] Add tests covering:
  - [ ] equal concrete leaf types (`u32 == u32`)
  - [ ] mismatched concrete leaf types (`u32 != bool`)
  - [ ] inference variable binding (`?X == u32`)
  - [ ] inference variable coalescing (`?X == ?Y`)
  - [ ] structural child unification (`Vec<?X> == Vec<u32>` binds `?X = u32`)
  - [ ] nested structural unification (`Option<Vec<?X>> == Option<Vec<u32>>`)
  - [ ] arity/skeleton mismatch (`(u32, bool) != (u32,)`, `Vec<u32> != Option<u32>`)
  - [ ] egraph congruence after rebuild (`?X = u32` makes `Vec<?X>` equal to `Vec<u32>`)
  - [ ] version rollback: a failed/discarded branch does not leak equalities to its parent
  - [ ] failed structural unification leaves no partial child equalities behind, or documents/implements rollback around recursive unification
  - [ ] recursive/infinite type attempt (`?X == Vec<?X>`) is rejected or explicitly documented as deferred if occurs-checking is not part of this helper
  - [ ] `Ty::Error` behavior is preserved when routed through `require_eq`
  - [ ] lifetime and const fields in skeletons compare structurally (`&'a T` vs `&'static T`, `[T; N]` vs `[T; M]`)
  - [ ] diagnostic wrapper behavior: `require_eq` still reports the same mismatch after being refactored through `try_unify`
  - [ ] out-of-scope alias behavior: type aliases and associated type projections are not expanded or normalized by this helper

### Step 2: Goal, result, and clause data model

Add the solver-facing IR types without wiring them into body checking yet.

- [ ] Add MVP `Goal`, `Atom`, `Assumption`, `QueryResult`, `GoalResult`, `ProofValue`, `Subst`, and `Clause` types.
- [ ] Represent `QueryResult` as a binder-wrapped boundary result.
- [ ] Omit deferred cases like `Normalize` and `ForAll` from the initial Rust enums until their data models land.
- [ ] Add tests covering:
  - [ ] `Goal::All([])` is the trivially true residual
  - [ ] nested `Goal::All` flattening preserves conjunct order/canonical form
  - [ ] binder-wrapped `QueryResult` can reference its bound existentials from `value`, `subst`, and `modulo`
  - [ ] `Subst` cannot contain duplicate bindings for the same canonical variable, or duplicate handling is deterministic
  - [ ] `Clause` head/body construction preserves binder scope

### Step 3: Canonicalization and instantiation

Implement the boundary crossing between caller inference state and solver query state.

- [ ] Canonicalize caller generic params and inference vars into `AlphaEquivParam` indices.
- [ ] Preserve the reverse mapping needed to instantiate `QueryResult` back into the caller egraph.
- [ ] Open result binders by allocating fresh local inference variables.
- [ ] Add tests covering:
  - [ ] deterministic indexing by first encounter order
  - [ ] same caller variable appearing multiple times maps to one `AlphaEquivParam`
  - [ ] distinct caller variables with same printed name still map distinctly
  - [ ] generic params and inference vars of different kinds do not collide
  - [ ] assumptions participate in canonicalization, not just the goal atom
  - [ ] nested binders shadow/allocate independently from outer canonical variables
  - [ ] round-trip substitutions, residual goals, and bound result existentials
  - [ ] instantiated result binders allocate fresh variables on each open and do not reuse stale frame-local variables

### Step 4: Equality goals and conjunction fixpoint

Implement enough structural proof machinery to solve equality-only goals and conjunctions.

- [ ] Implement `Atom::Equals` using `try_unify`.
- [ ] Implement `Goal::Exists`, `Goal::Implies`, `Goal::Maybe`, and `Goal::All`.
- [ ] Implement the conjunction flounder loop with residual retention and retry after substitution changes.
- [ ] Add tests covering:
  - [ ] `All([])` succeeds with `modulo: All([])`
  - [ ] `Equals(?X, u32)` applies an egraph equality and succeeds
  - [ ] conjunction short-circuits to `No` when any conjunct fails
  - [ ] partial success replaces a conjunct with its residuals
  - [ ] `Maybe` keeps the substituted goal as a residual
  - [ ] `?X = u32, ?X: Clone`-style retry using a stub atom that first flounders
  - [ ] fixpoint termination when no residual changes between iterations
  - [ ] `Implies` assumptions are scoped to the inner goal and do not leak to siblings

### Step 5: Whiteboard and atomic proof orchestration

Add the per-query whiteboard and parent-chain cycle detection. The MVP uses the whiteboard primarily to prevent infinite recursion, not to maximize caching.

- [ ] Add one whiteboard per `GoalQuery::prove`.
- [ ] Represent each atomic proof as a `ProofFrame { key, parent }`.
- [ ] Key entries by `(canonical atom, canonical environment, depth, parent frame)`.
- [ ] Walk the parent chain before spawning; if the same canonical atom/environment appears in an ancestor, return `No`.
- [ ] Await in-progress duplicate entries only when they have the same full key, including parent frame.
- [ ] Add tests covering:
  - [ ] same-parent duplicate returns a future for the same frame
  - [ ] different-parent same atom allocates a distinct frame
  - [ ] direct self-recursion is rejected as a cycle
  - [ ] indirect recursion through multiple parents is rejected as a cycle
  - [ ] same atom at different recursion depth is a distinct frame unless the depth cap has fired
  - [ ] future returned before completion polls pending and then ready after `finish`
  - [ ] multiple waiters observe the same completed result
  - [ ] completed entries remain readable until the enclosing `GoalQuery::prove` finishes
  - [ ] `finish` rejects or asserts double completion of the same frame
  - [ ] cycle detection ignores completed non-ancestor frames and only walks the parent chain
  - [ ] frame ids remain stable after later frames are appended

See [Whiteboard implementation sketch](./impl-sketches/whiteboard.md) for the pseudocode and cycle-handling details.

### Step 6: Environment clauses

Prove trait goals from assumptions before scanning impls.

- [ ] Lower `Assumption::Atom` and `Assumption::Implies` into candidate clauses.
- [ ] Match candidate heads against atomic goals with `try_unify`.
- [ ] Prove candidate bodies as conjunctions.
- [ ] Add tests covering:
  - [ ] direct bounds like `T: Clone`
  - [ ] implication assumptions like `T: Copy => T: Clone`
  - [ ] assumption head unification can bind goal variables
  - [ ] assumption body residuals are returned when not fully proven
  - [ ] multiple matching assumptions produce alternatives
  - [ ] non-matching trait symbol or self type is ignored
  - [ ] assumptions introduced by `Implies` are visible only while proving the implied goal

### Step 7: Anti-unification for hints

Implement anti-unification as a standalone helper before using it in disjunction merging. The helper computes a conservative common shape for incompatible substitutions and introduces fresh existential witnesses for the parts that differ.

- [ ] Add an anti-unification helper for `GenericArg`/type values used in `Subst`.
- [ ] Preserve identical structure where possible (`Vec<u32>` and `Vec<i32>` -> `Vec<?T>`).
- [ ] Produce shallow, non-repeated fresh witnesses for diverging substructure (`(?A, ?B)`, not inferred equality like `(?A, ?A)` unless both sides are literally the same).
- [ ] Anti-unify whole substitutions by key: only caller variables present in all successful alternatives produce hints.
- [ ] Add tests covering:
  - [ ] identical substitutions remain unchanged
  - [ ] leaf mismatch (`u32` vs `i32`) produces one fresh witness
  - [ ] shared outer constructor (`Vec<u32>` vs `Vec<i32>`) preserves `Vec<_>`
  - [ ] different outer constructors (`Vec<u32>` vs `Option<u32>`) produce one fresh witness
  - [ ] tuple/component mismatch produces independent shallow witnesses
  - [ ] repeated existential structure may be approximated shallowly: `exists<T> Vec<(T, T)>` and `exists<U> Vec<(U, U)>` may produce `exists<A, B> Vec<(A, B)>`
  - [ ] document the more precise future answer for repeated structure: `exists<A> Vec<(A, A)>`
  - [ ] structurally more-general side wins: `exists<A> Vec<Vec<A>>` and `exists<B> Vec<B>` produce `exists<B> Vec<B>`
  - [ ] missing substitution key in one alternative drops that key from hints
  - [ ] conflicting hints for multiple caller variables anti-unify independently
  - [ ] existing bound variables in input substitutions are alpha-renamed apart before anti-unification
  - [ ] multi-alternative accumulation is order-independent after canonicalization

### Step 8: Conservative subsumption

Implement the MVP `A.subsumes(B)` check as a standalone helper before using it in disjunction merging.

- [ ] Open binder-wrapped `QueryResult`s into a fresh comparison egraph.
- [ ] Compare proof values using diagnostic-free structural unification.
- [ ] Compare substitutions using diagnostic-free structural unification without mutating antecedent caller bindings.
- [ ] Flatten residual `Goal::All` values into canonical conjunct sets.
- [ ] Recognize residual implication by subset (`conjuncts(MA) ⊆ conjuncts(MB)` for `A.subsumes(B)`).
- [ ] Recognize unconditional top (`subst: []`, `modulo: All([])`) as subsuming all successful answers with the same proof value.
- [ ] Add tests covering:
  - [ ] unconditional answer subsumes conditional answer
  - [ ] conditional answer does not subsume unconditional answer
  - [ ] identical substitutions and identical residuals are equivalent
  - [ ] incompatible substitutions (`?X = u32` vs `?X = i32`) do not subsume either direction
  - [ ] residual subset (`X` subsumes `X && Y`)
  - [ ] residual superset does not subsume subset
  - [ ] binder witness instantiation (`exists<T> ?X = Vec<T>` subsumes `?X = Vec<u32>`)
  - [ ] binder variables are alpha-renamed apart before comparison
  - [ ] proof-value type mismatch prevents subsumption
  - [ ] residual subset comparison ignores conjunct ordering after canonicalization
  - [ ] duplicate residual conjuncts do not affect subset comparison
  - [ ] cases needing real trait implication remain non-subsuming under the MVP approximation

### Step 9: Conservative alternative merging

Implement the MVP disjunction policy for multiple candidates.

- [ ] Early-return only for `Yes { subst: [], modulo: All([]) }`.
- [ ] Anti-unify substitutions into `Maybe` hints when alternatives are incompatible.
- [ ] Use the standalone conservative subsumption helper to keep the more general compatible answer.
- [ ] Add tests covering:
  - [ ] `?X = u32` vs `?X = i32` becomes `Maybe` with anti-unified hints
  - [ ] unconditional proof with empty substitution wins early
  - [ ] proof with non-empty substitution does not win early until siblings are considered
  - [ ] residual subset subsumption keeps the more general answer
  - [ ] equivalent answers in different arrival orders produce the same merged result
  - [ ] all `No` alternatives produce `No`
  - [ ] `Maybe` alternatives contribute hints but do not by themselves prove the goal

### Step 10: Impl clause assembly

Connect trait-system impl signatures to solver clauses.

- [ ] Assemble clauses from local trait impls.
- [ ] Start with a linear scan; defer indexing by trait/self-type head.
- [ ] Convert impl where-clauses into clause bodies.
- [ ] Add tests covering:
  - [ ] concrete impl proof
  - [ ] generic impl proof with inferred type parameter
  - [ ] generic impl proof with where-clause residuals
  - [ ] non-matching trait impl is ignored
  - [ ] non-matching self-type head is ignored
  - [ ] two applicable impls produce alternatives for merge policy
  - [ ] impl generics are fresh per candidate and do not leak between alternatives

### Step 11: Type-checker integration

Submit and manage solver obligations from body checking.

- [ ] Canonicalize goals from the type checker's inference context.
- [ ] Apply `Yes` substitutions and hold `modulo` as deferred obligations.
- [ ] Apply `Maybe` hints conservatively.
- [ ] Re-prove deferred obligations as inference variables become more known.
- [ ] Add tests covering:
  - [ ] deferred obligations converge after later type information
  - [ ] `Yes` substitution updates the caller egraph
  - [ ] residual goals are stored without losing their canonical-to-caller mapping
  - [ ] `Maybe` hints that conflict with caller state turn into failure or are ignored according to policy
  - [ ] repeated re-proving is idempotent when the caller state has not changed

### Step 12: Method-resolution integration

Use the solver for trait-method dispatch after inherent and where-clause-only lookup.

- [ ] Query candidate traits/impls for receiver types.
- [ ] Preserve ambiguity when multiple impl candidates produce incompatible substitutions.
- [ ] Add tests covering:
  - [ ] method resolution requiring trait impl dispatch
  - [ ] inherent method still wins before trait solving
  - [ ] where-clause-provided method works without scanning impls
  - [ ] ambiguous trait impl dispatch is reported/preserved
  - [ ] receiver inference variable defers method resolution until pinned

### Deferred beyond MVP

- [ ] Solver-backed subsumption instead of residual subset approximation.
- [ ] Associated type normalization.
- [ ] `ForAll` goals and higher-ranked lifetimes.
- [ ] Coherence, overlap, and specialization.
- [ ] Cross-query/global solver caching.
- [ ] Impl indexing by trait and self-type head.
