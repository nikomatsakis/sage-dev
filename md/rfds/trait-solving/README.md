# RFD: Trait Solving

**Status:** Draft

**Depends on:**
- [Trait System](./trait-system.md) — `TraitRef`, `WherePredicate`, `ImplSignature`, data model
- [Type Inference](./type-inference.md) — `VersionedEGraph`, inference variables, async executor

## Goal

Design the trait solver: given a goal like `Vec<?X>: Debug`, prove it (or report failure). The solver operates inside the same async executor as type checking, uses the versioned egraph for speculative exploration, and coordinates sub-proofs via a shared whiteboard.

## External interface

### Goal and query types

```rust
#[salsa::interned]
struct GoalQuery<'db> {
    goal: Stashed<GoalQueryData<'db, Goal<'db>>>,
}

struct GoalQueryData<'db, G> {
    for_all: Slice<GenericParam<'db>>,   // canonical variables (AlphaEquivParam)
    assumptions: Slice<Assumption<'db>>, // environment at the proving site
    goal: G,
}

enum Goal<'db> {
    Exists(Binder<'db, Ptr<Goal<'db>>>),                // Output: from subgoal
    ForAll(Binder<'db, Ptr<Goal<'db>>>),                // Output: from subgoal
    Implies(Slice<Assumption<'db>>, Ptr<Goal<'db>>),    // Output: from subgoal
    All(Slice<Goal<'db>>),                              // Output: ProofValue::True
    Atom(Atom<'db>),
    Maybe,                                              // Output: n/a, cannot be proven
}

enum Atom<'db> {
    TraitImpl(TraitSym<'db>, Slice<GenericArg<'db>>),   // Output: ProofValue::True
    Equals(GenericArg<'db>, GenericArg<'db>),           // Output: ProofValue::True
    Normalize(AliasSym<'db>, Slice<GenericArg<'db>>),   // Output: ProofValue::Ty
    Outlives(GenericArg<'db>, Lifetime<'db>),           // Output: ProofValue::True
}

enum Assumption<'db> {
    ForAll(Binder<'db, Ptr<Assumption<'db>>>),
    Implies(Slice<Goal<'db>>, Ptr<Atom<'db>>),
    All(Slice<Assumption<'db>>),
    Atom(Atom<'db>),
}
```

Goals have an **output type** determined by their atom kind. `Normalize` produces a type; everything else produces `True` on success.

### Result type

```rust
/// The result of proving a goal. Lives in its own stash (a Stashed value)
/// so that it can be returned across canonicalization boundaries.
enum ProofResult<'db> {
    /// Proven, modulo these residual goals.
    Yes {
        value: ProofValue<'db>,
        modulo: Goal<'db>,
    },

    /// Ambiguous — multiple alternatives apply with incompatible residuals.
    /// Can only transition to Yes { modulo: All([]) } (fully proven).
    Maybe,

    /// Disproven. Terminal — once No, always No.
    No,
}

enum ProofValue<'db> {
    True,
    Ty(Ty<'db>),
}
```

The `ProofResult` is stashed — each whiteboard entry and each `GoalQuery::prove` return value is a `Stashed<ProofResult<'db>>`. The caller instantiates it into their own stash via substitution.

**Transition lattice:**
```
No                                          (terminal)
Yes { modulo: G } → Yes { modulo: G' }     where G' ⊆ G (residuals shrink)
Yes { modulo: G } → Maybe                  (discovered incompatible alternative)
Maybe → Yes { modulo: All([]) }            (fully proven — ambiguity becomes moot)
```

Key constraint: `Maybe` can only transition to `Yes` when `modulo` is trivially true (`All([])`). This is because once we've seen multiple incompatible alternatives, we can never know which residual set is correct — the only escape is proving the goal unconditionally.

### Entry point

```rust
#[salsa::tracked]
impl<'db> GoalQuery<'db> {
    #[salsa::tracked]
    pub fn prove(self, db: &'db dyn Db) -> Stashed<ProofResult<'db>>;
}
```

This is the synchronous salsa boundary. Internally it creates a local inference context, whiteboard, and async executor, then blocks on the result. Eventually we'll have more sophisticated caching, but this is the starting point.

## Canonicalization

Before entering the solver, the caller **canonicalizes** its goal:

1. Walk the goal in a deterministic order.
2. When encountering a `GenericParam` (universal from the caller's context) or an `InferVar` (existential), map it to a fresh `AlphaEquivParam { kind, index }`, starting at index 0 and incrementing.
3. Record the reverse mapping: `AlphaEquivParam(i) → original variable in caller's egraph`.
4. Allocate the canonicalized goal into a fresh stash → produces a `GoalQuery` (salsa-interned, so structurally-identical queries share identity).

The `GoalQueryData` has a flat structure: one level of `for_all` (binding the canonical variables), assumptions (the environment), and the goal. Nested quantifiers inside the goal (`Goal::Exists`, `Goal::ForAll`, `Goal::Implies`) are handled internally by the solver during proof.

When results come back, the caller **instantiates** them: fold the result from the entry's stash into the caller's stash, substituting each `AlphaEquivParam(i)` with `mapping[i]`. Universe constraints are respected because the mapped-back variables already carry the correct universe in the caller's egraph.

## Internal architecture

### The whiteboard

The internal proof machinery coordinates via a **whiteboard** — a shared table of in-progress atomic goal proofs. Each entry is a write-once future: it is resolved when the atomic goal's proof completes, and not before.

```rust
struct Whiteboard<'db> {
    entries: RefCell<FxHashMap<WhiteboardKey<'db>, WhiteboardEntry<'db>>>,
}

struct WhiteboardKey<'db> {
    /// Canonical form of the atomic goal (in its own stash).
    query: Stashed<GoalQueryData<'db, Atom<'db>>>,
    /// Recursion depth — ensures termination.
    depth: u32,
}

struct WhiteboardEntry<'db> {
    /// Write-once: None while in progress, Some when complete.
    result: Option<Stashed<ProofResult<'db>>>,
    /// Wakers of tasks blocked waiting for completion.
    wakers: Vec<Waker>,
}
```

Each whiteboard entry produces its result in a **distinct stash** (the `Stashed` wrapper). This isolation gives clean canonical boundaries and will enable global caching later.

### Proving an atomic goal

When a task needs to prove an atomic goal:

1. **Canonicalize** the goal from the current egraph → produces a `WhiteboardKey` + a reverse mapping.
2. **Look up** the whiteboard:
   - **Entry exists, complete:** read the result immediately.
   - **Entry exists, in progress:** this is a cycle → return `No`.
   - **No entry:** create one, spawn an async task to prove it.
3. When the spawned task completes, it writes the result into the entry and wakes all waiting tasks.
4. **Instantiate** the result back into the caller's stash/egraph via the reverse mapping.

This gives natural **goal deduplication** — if multiple parts of the proof tree need the same atomic goal at the same depth, the second one waits for the first to finish.

**Cycles:** Finding an in-progress entry for your own goal means you've recursed into yourself. The result is `No` (unprovable). For coinductive cases (auto traits), the caller should add an explicit assumption to the environment before recursing, rather than relying on cycle-as-success.

### Recursion and termination

**Cycles are disallowed.** Finding an in-progress entry on the whiteboard for the same goal is always `No`. There is no coinductive cycle-as-success mechanism in the solver itself.

**Auto traits (Send, Sync, etc.)** are handled by adding an explicit assumption before recursing into fields. For example, when proving `Foo<?T>: Send`, we add `Foo<?T>: Send` to the environment before proving each field is `Send`. If a field has type `Foo<?T>`, it matches the assumption (just another alternative that unifies against the goal). The assumption may contain inference variables — that's fine, they get canonicalized normally (becoming universals in the canonical form, meaning "this holds for any value of this variable").

**Recursion depth** is part of the whiteboard key. When a task spawns a sub-goal at depth D, the sub-goal's entry is keyed at depth D+1. When the maximum depth is reached, the result is forced to `Maybe`.

This means the same canonical goal at different depths can yield different results (one might be fully proven, the other forced to ambiguity at the depth limit). This is not ideal but ensures termination. We can explore better strategies later (e.g., type size limits).

### Structural goal decomposition

The async function `prove_goal` handles non-atomic goal structure:

```rust
async fn prove_goal<'db>(
    goal: &Goal<'db>,
    ctx: &ProofCtx<'db>,  // owns egraph version, stash, env, depth
) -> ProofResult<'db>;
```

Decomposition rules:
- **`Goal::Atom(atom)`** → check whiteboard, subscribe or spawn.
- **`Goal::All(goals)`** → prove conjunctively (flounder loop, shared egraph version).
- **`Goal::Exists(binder)`** → open the binder by allocating fresh inference variables in the egraph, then prove the inner goal.
- **`Goal::ForAll(binder)`** → open the binder by allocating fresh universals (higher universe), then prove the inner goal.
- **`Goal::Implies(assumptions, goal)`** → extend the environment with the assumptions, then prove the inner goal.
- **`Goal::Maybe`** → return `Maybe` immediately.

### Alternatives (disjunction)

When proving an atomic goal, we assemble all candidate clauses whose head could unify with the goal. Each candidate is an **alternative**:

1. **Fork the egraph** (cheap — new version node with empty sparse diffs).
2. **Unify** the clause head with the goal in that fork.
3. If unification succeeds, **prove the clause body** as a conjunction in that fork.
4. Report the result back to the parent.

Alternatives run as **scoped async tasks**. The parent starts with `No` and incorporates results as they arrive:

- **First `Yes`** → becomes the current answer.
- **Another `Yes` with compatible residuals** (one subsumes the other) → upgrade to the tighter one.
- **Another `Yes` with incompatible residuals** → retract to `Maybe`.
- **`No`** → ignored (doesn't change the current answer).
- **All alternatives complete** → the current answer is final.

**Early termination:** If any alternative yields `Yes { modulo: All([]) }` (fully proven, no residuals), it's a clear winner — drop sibling futures immediately. This is the common case for concrete types where only one impl applies.

### Conjunctions (the flounder loop)

When proving a conjunction (`Goal::All` or a clause body), sub-goals share the same egraph version and communicate through it:

```
loop {
    for each pending sub-goal:
        fully substitute known values from the egraph
        if the goal changed from last iteration:
            re-attempt proving it (recursive prove_goal call)
        match result:
            Yes { modulo: All([]) } → remove from pending (fully proven)
            Yes { modulo: residuals } → replace with residuals
            No → the whole conjunction fails, return No
            Maybe → keep in pending (floundered)

    if nothing changed this iteration:
        break — remaining pending goals are residuals
}
```

**Termination:** fixpoint — when no sub-goal's substituted form changes between iterations. Remaining floundered goals become the conjunction's residuals.

**Communication between siblings:** Because conjuncts share an egraph version, proving one sub-goal may unify variables that appear in another. On the next loop iteration, the second sub-goal's substituted form changes, triggering a retry. This is how `?X: Clone` gets resolved after a sibling pins `?X = u32`.

### Associated types (normalization)

`Atom::Normalize(alias, args)` goals produce a `ProofValue::Ty` — the normalized type.

Strategy:
- The proof selects an impl, extracts the associated type definition.
- The result is a single shallow normalization step.
- The result type is added to the caller's egraph (unified with the projection node).
- Further normalization (if the result is itself a projection) happens via the egraph's lazy recanon-on-find.
- Cycles are naturally handled: the egraph represents them as self-referential nodes.

## Integration with the type checker

The type checker creates goals and submits them:

1. **Canonicalize** the goal from the type checker's inference context (mapping inference variables + generic params to `AlphaEquivParam` indices, allocating into a fresh stash).
2. Call `GoalQuery::prove(db)` at the salsa boundary.
3. **Instantiate** the `Stashed<ProofResult>` back into the type checker's stash/egraph via the reverse mapping.
4. If the result has residuals, hold them as deferred obligations. As inference progresses, re-canonicalize and re-prove (salsa may cache the previous result if inputs haven't changed).

For method resolution specifically: submit `ReceiverTy: Trait`, and once `Yes` comes back, the method is resolved. The `modulo` residuals become background obligations.

## Deferred

- **Sophisticated caching** — currently each `GoalQuery::prove` recomputes. Later: exploit salsa memoization, cross-query deduplication beyond the whiteboard.
- **Cycles (coinductive)** — auto traits. Currently cycles → `No`. Coinductive cases are handled by explicit assumptions in the environment.
- **Higher-ranked lifetimes** — `for<'a> &'a T: Trait`. Defer.
- **Coherence / overlap** — separate concern.
- **Specialization** — selecting more specific impls.
- **Streaming / incremental updates** — the current model runs each `GoalQuery::prove` to completion. If needed later, we could add generation-based streaming for intra-solver communication.

## Open questions

1. **Scoped task semantics** — alternatives need structured concurrency (all must complete or be cancelled before the parent proceeds). Does the runtime need explicit scope support?

2. **Whiteboard lifetime** — one whiteboard per `GoalQuery::prove` invocation? Or shared across the type-checking session for cross-function deduplication?

3. **Environment representation** — currently threaded through the `GoalQueryData`. Should assumptions also be interned/canonicalized for better sharing?

4. **Ambiguity policy** — when the type checker gets `Maybe`, does it wait (hoping inference will narrow things), or proceed optimistically? Probably context-dependent.

5. **Recursion depth default** — what's a reasonable cap? Rust code rarely needs deep recursion in trait solving. 64? 128?

6. **Stash for results** — each whiteboard entry produces its result in its own stash. Is there value in sharing a stash across entries within a single `prove` invocation? Probably not — isolation is more important than the small allocation savings.
