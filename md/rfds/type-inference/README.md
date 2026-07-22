# RFD: Type Inference

**Status:** Completed

**Depends on:**
- [Type Signatures](../type-signatures/README.md) — `Ty`, `Binder`, `TyFolder`, stash-allocated types
- [Per-kind symbol data](../per-kind-symbol-data/README.md) — `FnSymbol`, `StructSymbol`, etc.
- [Generic params as symbols](../generic-params-as-symbols/README.md) — `GenericParam`, stable identities replacing de Bruijn indices

## Goal

Design the **type elaboration** engine for sage. This covers inference variables, constraint propagation, trait solving, and the versioned data structures that support speculative exploration.

**Elaboration vs. verification:** This RFD covers *elaboration* — fully inferring types, resolving all inference variables, and choosing coercions to produce a fully-typed IR. The goal is to always produce output, including error-recovery nodes when inference remains ambiguous. Correctness checking (subtyping validity, lifetime soundness, borrow checking) happens in a separate verification pass over the elaborated IR. Elaboration may recover after an error, but it must not commit a merely likely trait candidate or heuristic equality and rely on verification to undo that choice.

**Intentionally simplified model.** We are starting with a pure, simplified view of Rust's type system — ignoring or approximating many edge cases (complex coercion chains, higher-ranked trait bounds, auto traits, etc.). The architecture is designed to accommodate the full complexity incrementally as we go, but the initial implementation will handle the common paths and leave exotic corners for later. We expect to iterate on and refine the type rules as we encounter real-world code that needs them.

## Design overview

The system has three major components:

1. **Inference variables with streaming bounds** — each inference variable `?X` carries a bound that tightens over time, modeled as an async stream of `Bound<Ty>` values.
2. **A versioned union-find** — supports speculative branching during trait resolution (try one impl candidate, backtrack if it fails, try another).
3. **Trait solving through canonical obligations** — rigid caller parameters and
   flexible inference variables cross a cached query boundary with distinct
   metadata; conditional answers return residual obligations which must later
   be discharged.

### Inference variables and index spaces

Signatures already use `Ty::Param(GenericParam)` to reference generic parameters — these are stable symbol identities that need no shifting. During type inference, we add a new variant for unknowns:

```rust
enum TyData<'db> {
    // ... existing variants (Bool, Adt, Ref, Param, etc.) ...
    InferVar(InferVarIndex),     // used during inference — fresh unknowns
}
```

`GenericParam` values (universals from the function's signature) participate in inference as-is — they're already stable, hash-consable, and can appear freely in types at any depth. The `InferVar` variant represents existential unknowns that the inference engine must resolve.

**Opening/instantiating a signature** creates one coherent `Substitute` mapping
for its binder, then folds the signature with each `GenericParam` replaced by a
concrete type, rigid parameter, or fresh `InferVar`. Every component under the
same binder must reuse that one opening.

#### Newtyped index spaces

```rust
/// Stash allocation index — identifies a Ty node *structurally*.
/// Hash-consed: same TyData content = same TyIndex.
/// This is the egraph key: works for variables, concrete types, and applications alike.
struct TyIndex(u32);  // derived from Ptr<Ty>

/// Sequential counter for inference variables. Dense, monotonically increasing.
/// Indexes into the variable metadata table. Lives inside Ty::InferVar.
struct InferVarIndex(u32);

/// Version tree node identifier. Stable for the egraph lifetime (or paired
/// with a generation if storage slots are recycled).
struct Version(usize);
```

#### Variable metadata (globally identified, version-owned)

Every inference variable has a globally unique `InferVarIndex` within one
egraph. Its metadata records the version which created it:

```rust
struct VarInfo {
    owner: Version,                 // branch which created this variable
    created_universe: Universe,     // immutable allocation scope
    // span, debug name, etc.
}

/// Universe is just a scope depth counter. A variable at universe U can be
/// equated to types containing only variables at universe <= U.
struct Universe(u32);
```

Universal parameters (from function/impl generics) don't need `VarInfo` — they already have identity and scope via `GenericParam`. Only existential inference variables are tracked here.

The egraph stores a single append-only metadata table. A variable is visible
only from its owning version and descendants:

```rust
fn get_variable(egraph: &VersionedEGraph, v: Version, idx: InferVarIndex) -> &VarInfo {
    let info = &egraph.variables[idx.0 as usize];
    assert!(egraph.versions.is_ancestor_of(info.owner, v));
    info
}
```

Sibling versions never reuse an index:

```
Root:     variables=[?0, ?1, ?2, ?3]
Branch A: variables=[?4, ?6]
Branch B: variables=[?5, ?7, ?8]
```

This gives every `Ty::InferVar` an unambiguous identity even if a type pointer
is accidentally presented to the wrong concurrent branch. Explicit-version
operations still enforce visibility. The tradeoff is less cross-branch stash
sharing, which is preferable to allowing one branch's type to be reinterpreted
as a sibling's variable.

**Setting up a function** (at the root version):

```
impl<X> Foo<X> {
    fn bar<Y>(self) { ... }
}

// GenericParams X and Y already exist as symbols.
// Instantiating the signature: X → Ty::Param(X), Y → Ty::Param(Y)
// These are universal — they appear in types but are NOT InferVars.

// Checking the body creates existentials:
InferVar(0) = ?0, universe=1  (for an unannotated let binding)
InferVar(1) = ?1, universe=1  (for a method return type)
// Entering a closure:
InferVar(2) = ?c0, universe=2
```

The creation universe records origin. A separate versioned current ceiling may
only move outward: an equality such as `?X@U1 = Vec<?Y@U2>` lowers `?Y`'s
ceiling to `U1`, preventing a later indirect leak. `GenericParam` values from
the enclosing signature are rigid placeholders (effectively universe 0 in
this example), not `VarInfo` entries.

A committed ceiling decrease is a semantic bound change: it advances the
variable's revision and wakes dependents so canonical queries are rebuilt with
the narrower ceiling. Speculative lowering remains buffered in its version and
vanishes without a wake when discarded.

Since each `InferVarIndex` appears inside a distinct `Ty::InferVar(i)`, each gets a distinct `TyIndex` after hash-consing. This is what makes `TyIndex` viable as the egraph key for both variables and concrete types.

### Inference variable bounds

Each inference variable tracks a *bound* that narrows monotonically:

```rust
enum Bound<T> {
    None,        // BOTTOM — no information yet
    AtLeast(T),  // >= this, more updates may come
    Exactly(T),  // = this, final — stream is done
}
```

A variable's stream produces a sequence:

```
None → AtLeast(Foo<'a>) → AtLeast(Foo<'static>) → Exactly(Foo<'static>)
```

**Key invariant: bounds never contain inference variables.** Bounds are always concrete types (may contain `GenericParam` universals, but never `InferVar`). This restriction is specific to the streaming-bound lattice: egraph equality classes may still relate an inference variable to a structured type containing other inference variables, subject to occurs and universe checks. For bounds, the invariant means:

- Bounds are always directly printable, comparable, and hashable
- No transitive closure needed to inspect what a bound resolves to
- Bounds cannot directly hide an inner inference variable; egraph equalities
  separately enforce versioned universe-ceiling lowering for nested flexible
  variables

**Bounds are versioned** — a speculative branch might set `?X >= Foo` based on a candidate that turns out to be wrong. Discarding the branch rolls back the bound. (See versioned data structure below.)

Every bound write also checks the target variable's current versioned universe
ceiling. Bounds contain no inference variables, but a concrete bound may still
contain rigid type/lifetime/const placeholders; one above the ceiling is a leak
and the `AtLeast`/`Exactly` update is rejected without a wake.

**Relationships between variables** are maintained by two mechanisms:
1. **Equality** (`?X = ?Y`) — goes into the versioned egraph as a union. Congruence propagates.
2. **Ordering/subtyping** (`?X >= ?Y`) — maintained by running async tasks ("constraint watchers") that observe both sides' streams. When one side becomes concrete, the watcher propagates to the other.

Inter-variable constraints are *live tasks* in the executor, not data stored in the bounds. The async model is a natural fit: each constraint is a tiny task awaiting one variable's stream and updating another.

The promotion from `AtLeast` to `Exactly` is a key event: it finalizes the stream, enables congruence propagation, and potentially triggers cascading promotions via constraint watchers.

**Cycle detection.** Two watchers observing each other (`?X` depends on `?Y`, `?Y` depends on `?X`) would deadlock in the naive case. The egraph handles the equality case (just union them). Subtler cycles through subtyping or projections will need detection — the exact mechanism is TBD as we prototype.

### The egraph layer

The egraph stores equality facts settled within a particular version. A
speculative version may grow while a probe or candidate is running, but those
facts reach an ancestor only through an explicit successful collapse; a
discarded branch publishes none of them.

Projections like `<T as Trait>::Assoc` are operator applications in the egraph: `Proj(T, Trait, Assoc)`. When the solver definitively resolves `<T as Trait>::Assoc = U`, we union the projection node with `U` in the egraph. Congruence then propagates automatically: if `T = S` is later established, `<T as Trait>::Assoc = <S as Trait>::Assoc` follows for free.

The egraph is versioned — each speculative branch has its own view. Edges added in a discarded branch disappear with it.

### Versioned inference state

Based on the versioned e-graphs paper (2026). The inference state generalizes a union-find to also store versioned bounds.

#### The core data structure

```rust
struct VersionedEGraph {
    /// The stash — types are allocated here. Append-only, hash-consing.
    /// TyIndex = Ptr<Ty> index into this stash. NOT versioned.
    stash: Stash,
    
    /// Version nodes, indexed by Version. Each node contains all
    /// per-version state.
    versions: Vec<VersionNode>,

    /// Globally unique inference-variable metadata. Each record names the
    /// version which owns the variable.
    variables: Vec<VarInfo>,
}

struct VersionNode {
    /// Parent in the version tree.
    parent: Version,
    /// Child versions.
    subversions: Vec<Version>,
    
    // --- Mutable inference state (sparse diffs from ancestry) ---
    
    /// Equality edges (union-find parents), keyed by TyIndex.
    parents: FxHashMap<TyIndex, TyIndex>,
    
    /// Bounds on inference variables, keyed by TyIndex of the InferVar node.
    bounds: FxHashMap<TyIndex, Bound<TyIndex>>,

    /// Current maximum accessible universe for variables whose ceiling was
    /// lowered in this version. Absent entries inherit from ancestry, falling
    /// back to `VarInfo::created_universe`.
    universe_ceilings: FxHashMap<InferVarIndex, Universe>,
    
    /// Dependents for congruence closure: who references this node as an arg.
    dependents: FxHashMap<TyIndex, Set<TyIndex>>,
    
    /// Worklist for deferred congruence repair.
    worklist: Vec<TyIndex>,
}
```

**Key insight:** Both equalities AND bounds are versioned (per-node sparse diffs). A speculative branch can freely add equalities, tighten bounds, and allocate new variables; if the branch is discarded, its `VersionNode` is simply dropped — everything rolls back automatically.

The stash and globally unique variable metadata are append-only. Types and
variable records allocated in a discarded branch remain as harmless,
inaccessible entries; the mutable inference facts which could affect a parent
remain versioned.

#### Version tree

A tree of versions where each version inherits from its parent. `branch_from(v)`
creates a new child version. `remove(v)` discards a version and its subtree.
The identity is not reused during the egraph lifetime unless the handle carries
a generation, because inference-variable metadata retains owning versions.

Sparse inheritance is snapshot-stable because per-version mutation is
leaf-only. Creating a child freezes its parent against path compression,
variable allocation, equality or bound writes, rebuild/revision publication,
and inference wakes until every child is discarded or the sole child is
collapsed atomically. Otherwise a child's ancestry walk could observe parent
facts created after the branch. Read-only lookup and append-only global stash
allocation do not change the snapshot.

#### Sparse diff semantics

Each version only records edges/bounds that differ from its ancestry. To look up the parent (equality) or bound of a node at version `v`, walk up the version tree:

```
fn get_parent(version, x: TyIndex) -> TyIndex:
    let v = version
    loop:
        if parents[v] contains x: return parents[v][x]
        v = version_tree.parent(v)
    // hits root version which has self-loops for all canonical nodes

fn get_bound(version, x: TyIndex) -> Bound<TyIndex>:
    let v = version
    loop:
        if bounds[v] contains x: return bounds[v][x]
        v = version_tree.parent(v)
    return Bound::None  // no bound recorded anywhere in ancestry
```

The full `find(x)` at version `v` is the standard union-find chase, but each parent lookup walks the version ancestry.

**Path compression.** When `find_mut` discovers the canonical representative, it inserts shortcut edges into `parents[version]`. This makes future queries at the same version faster without affecting other versions. The "half path compression" strategy (compress every other node on the path) provides a good tradeoff between map insertions and query speed.

#### Variable allocation and versioning

The egraph allocates inference-variable IDs from one monotonic counter and
records their owner versions:

```
Root version: variables=[?0, ?1, ?2]
  ├─ Branch A: variables=[?3, ?5, ?7]
  └─ Branch B: variables=[?4, ?6]
```

`InferVar(3)` always means Branch A's first variable. Branch B cannot resolve or
bind it because Branch A is not in Branch B's ancestry. Its own first variable
is `InferVar(4)` and therefore has a distinct stash node.

Note: universal parameters (`GenericParam` values from the function/impl signature) are not allocated in the version tree at all — they already have stable identities. Only existential inference variables live here.

If Branch B is discarded, its mutable version state is dropped and its unique
variable records become inaccessible. A short-lived transactional child may
collapse into its direct parent only under the exclusive-child rule; variables
owned by that child are re-owned by the parent as part of the atomic commit.

#### Branching workflow for trait resolution

1. Encounter a choice point (multiple candidate impls)
2. `branch_from(parent_version)` for each candidate — cheap (creates empty maps)
3. Explore each candidate in its own version (add equalities, tighten bounds, allocate new vars)
4. Extract a canonical response from every still-possible candidate
5. Discard every candidate branch after its task and descendants finish
6. Merge the responses outside the branches and transactionally apply only the
   chosen aggregate response to the caller

#### Stash interaction

The stash is part of the egraph structure — types are allocated into it during inference (e.g., when constructing `Vec<?X>` you allocate `Adt(Vec, [InferVar(3)])` into the stash). The stash is append-only and hash-consing: allocating the same `TyData` twice returns the same `TyIndex`. This is fine — structurally identical types *should* be the same egraph node.

The stash is *not* versioned. Types allocated in a speculative branch that gets
discarded remain in the stash (they are harmless and inaccessible from valid
results). Globally unique inference-variable IDs prevent one branch's leftover
type node from being reinterpreted as a sibling's variable. Only mutable state
such as parents, bounds, worklists, and buffered wakes is committed or
discarded with a version.

### Trait solving (sketch — separate RFD)

The accepted [Trait Solving RFD](../trait-solving/README.md) defines the query
boundary. The caller canonicalizes rigid generic parameters and flexible
inference variables with distinct roles and relative universes. It does not
turn an unknown inference variable into a universal placeholder.

The cached solver result is one of:

- **`Yes`** — definite equalities plus residual obligations;
- **`Maybe`** — proof remains possible, with only hard equalities common to
  every still-possible alternative;
- **`No`** — candidate sources were exhaustive and every candidate was
  definitively disproven.

Applying a response is transactional. A `Yes` is not final while it has a
nontrivial residual, and a `Maybe` never authorizes committing a heuristic
near-miss. The body checker retains and re-runs residual obligations when
relevant inference state changes; finalization must discharge or diagnose all
of them.

### Interaction between bounds and the egraph

The two constraint mechanisms are complementary:

| Mechanism | When it fires | What it produces |
|-----------|---------------|------------------|
| Bounds (`>=`) | Always, as information arrives | Narrowed bound on the stream |
| Egraph (`=`) | Only on `Exactly` promotion | Congruence propagation, settled facts |

The bounds lattice drives exploration; the egraph records conclusions. The transition from `>=` to `=` is the bridge between them.

### Handling `BOTTOM` and unbounded variables

An inference variable with no bound remains a flexible existential input when
canonicalized. For the MVP, a trait goal whose self type is a bare unbounded
variable flounders to `Maybe` and records that variable as a retry dependency;
the solver does not enumerate the crate to guess a receiver type. Structured
goals such as `Vec<?X>: Trait` may still match impl heads and derive hard
equalities for `?X`.

Candidate-specific suggestions such as “this would work if `?X` were `u32`”
are heuristic near-misses, not proof consequences. They may be exposed later
for diagnostics or ranking, but are not applied to inference state by the MVP.

### Congruence closure (the egraph machinery)

On top of the versioned union-find, the egraph adds:

- **Dependents tracking** — for each class, which other classes reference it as an argument. When a class is merged, its dependents need re-canonicalization.
- **Worklist** — per-version list of merged classes pending congruence repair.
- **Deferred rebuild** — congruence restoration is batched. Multiple unions can happen before paying the cost; the worklist accumulates and `rebuild()` drains it.

This ensures that `?X = ?Y` automatically propagates to `Vec<?X> = Vec<?Y>`, `<? X as Trait>::Assoc = <?Y as Trait>::Assoc`, etc., without the solver re-deriving these equalities.

## Surface desugarings

Some Rust syntax desugars into trait-mediated operations (`?`, `for`, `.await`, operators). Rather than always expanding these into their full desugared form (which would require the trait solver to type-check), we use a hybrid approach:

**In the IR:** A `Desugar(kind, body)` node where `kind` identifies the surface syntax and `body` is the desugared expression tree.

```rust
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, AllocStashData)]
enum DesugarKind {
    QuestionMark,  // ? operator
    ForLoop,       // for x in iter { ... }
    Await,         // expr.await
    // ...
}
```

The desugared `body` is always present in the IR — it represents the "real" semantics.

**In the type checker:** The type checker can either:
1. Apply a **special-case typing rule** for the `DesugarKind` (e.g., `QuestionMark` → "unwrap Result, coerce error type") — used when trait solving isn't available or isn't needed.
2. **Type-check the desugared body** directly — used once the trait solver is mature enough.

This means we don't need to choose upfront whether something is "fully desugared" or "special-cased" — the same IR supports both, and we can migrate incrementally.

**Decision criterion:** Desugar into the IR if the desugared form is straightforward (if-let → match, ranges → struct construction). Keep a `Desugar` wrapper when the desugared form involves trait dispatch that the type checker may want to bypass.

| Syntax | Approach | Rationale |
|--------|----------|-----------|
| `if let` / `while let` | Direct desugar (no wrapper) | Pure syntax → match, no traits |
| `..` ranges | Direct desugar | Struct construction |
| `?` | `Desugar(QuestionMark, ...)` | Involves `Try` + `FromResidual`; special-case for Result/Option |
| `for` loops | `Desugar(ForLoop, ...)` | Involves `IntoIterator`; special-case for known iterators |
| `.await` | `Desugar(Await, ...)` | Involves `Future::poll`; special-case to just yield `Output` type |
| Operators on primitives | Intrinsic typing rules | No desugaring needed; trait dispatch only for custom types |

## Execution model: async type checking

The type checker is implemented as an **async function running on a custom executor**. This gives us two things:

### 1. Awaiting inference variable updates

Inference variables expose a stream interface:

```rust
impl InferenceVariable {
    /// Yields the next bound update, or None when the variable is finalized (Exactly).
    async fn next_bound(&self) -> Option<Bound<..>>;
}
```

When a piece of the type checker needs information from an inference variable that isn't yet determined, it `.await`s the next bound. The custom executor manages the wakeups — when a variable's bound tightens, all tasks awaiting it are woken.

This replaces the traditional "iterate to fixpoint" approach with a reactive one: work only happens when new information arrives.

The current `InferVarIndex` and solver MVP are type-only. A future generalized
bound-stream interface may also represent lifetime or const unknowns, but the
trait plan must not assume those variable kinds exist today.

### 2. Parallel type checking within a function body

Independent parts of the IR can be type-checked concurrently as separate async tasks. For example:

- Two independent `let` statements can be checked in parallel
- The arms of a match can be checked in parallel (they share the scrutinee type but are otherwise independent)
- Function arguments can be checked in parallel

The custom executor interleaves these tasks on one thread. The async structure
expresses the dependency graph; `RefCell` state and `!Send + !Sync` proof
contexts deliberately rule out cross-thread scheduling in this design.

When tasks produce type equalities or bound updates, those propagate through the inference variable wakeup mechanism to any other tasks that depend on them.

### Why a custom executor

A standard async runtime (tokio, etc.) is designed for I/O-bound workloads with external event sources. Our workload is different:

- **No I/O** — all data is local
- **Fine-grained tasks** — potentially one per expression or statement
- **Structured concurrency** — the task graph mirrors the IR tree
- **Custom scheduling** — we may want priority based on how much information a task would unlock (e.g., tasks that would pin an inference variable are high-priority)
- **Integration with versioning** — tasks in a speculative branch should be cancelled when the branch is discarded

### Executor design sketch

The executor is a single-threaded, cooperative scheduler:

```rust
struct Runtime {
    /// Tasks ready to be polled.
    ready: VecDeque<Task>,
    /// Per-inference-variable waker lists.
    waiting: HashMap<InferVarId, Vec<Waker>>,
}
```

**Core loop:**

```rust
impl Runtime {
    fn drain(&mut self) {
        while let Some(task) = self.ready.pop_front() {
            task.poll(self);  // may register wakers in self.waiting
        }
    }
}
```

**Wakeup mechanism:** When an inference variable's bound tightens, all wakers in `waiting[var]` are drained and their tasks re-queued into `ready`. This is the sole event source — no I/O, no timers, just constraint propagation.

**Inference variable polling:** The core primitive for awaiting a bound update:

```rust
impl InferenceVariable {
    /// Polls the variable. Returns Some(bound) if the bound has changed since
    /// last poll, None if inference is finalized with no further updates.
    /// Suspends (registers waker) if no new information is available yet.
    async fn next_bound(&self) -> Option<Bound<..>> { ... }
}
```

Internally this is `poll_fn`: read the variable's current state, compare to what the caller last saw, return `Poll::Ready` if changed or `Poll::Pending` + register waker if not.

**Parallelism within a task:** Use standard `futures` combinators:
- `futures::future::join(check_lhs, check_rhs)` — independent sub-expressions
- `futures::future::join_all(args.map(|a| a.check(ctx, ...)))` — function arguments

These are cooperative within a single task's poll. "Parallel" means interleaved when sub-futures block on different inference variables.

**Structured concurrency (moro-style):** For blocks with multiple statements, use a scoped spawn pattern (inspired by the `moro` crate). Spawned tasks within a scope share `&InferCtx` and are guaranteed to complete before the scope exits:

```rust
// Variable bindings introduced sequentially; initializer checks spawned concurrently
for stmt in stmts {
    if let Stmt::Let { pattern, initializer, .. } = stmt {
        ctx.push_bindings(pattern.check(ctx, declared_ty).await);
        scope.spawn(async move { initializer.check(ctx, ...).await; });
    }
}
```

**`&self` convention:** Since `InferCtx` is shared across concurrent tasks within a scope, all methods take `&self`. Internal mutation (egraph state, variable allocation, bound updates) is guarded by `RefCell`. This is safe because the executor is single-threaded and cooperative — no two tasks are polled simultaneously.

### Finalization

The root expression task may suspend before all statements have been visited,
and inference can still have variables whose bounds will never improve. Body
completion therefore cannot wait for a fully built IR and only then call a
one-shot finalizer.

**Finalization protocol:**

1. Keep the root runtime scope open. Drain runnable expression/constraint
   tasks, committed wakes, and ready trait obligations together until no
   component makes semantic progress.
2. Stable suspension is a quiescence barrier, not success or an immediate
   deadlock. Promote each still-unresolved variable from `AtLeast(B)` to
   `Exactly(B)`, or to `Exactly(Error)` from `BOTTOM`, and publish its wakes.
3. Resume the joint drain. A woken expression task may visit more source,
   allocate more variables, or submit more obligations, so repeat steps 1 and
   2.
4. When all variables are final but stable obligations or lookup waits remain,
   re-run them once, turn every remaining `Maybe`, `No`, or conditional
   residual into a diagnostic/error outcome, and wake their waiting expression
   tasks. Then return to step 1; this handles unsupported obligations with no
   inference dependency.
5. Finish only after the root expression and all scoped tasks join and every
   obligation is terminal. A pending task with no registered variable or
   obligation wait is an internal runtime deadlock. Assert that no waiter or
   speculative branch remains.

Finalization is a recovery transition made only at global stable quiescence.
It deliberately wakes the tasks which could not finish source traversal; those
tasks are then allowed to produce their remaining constraints before body
completion is declared.

### Error representation

Compilation always produces useful output — the IR is always built, even in the presence of errors.

**Separation of IR and diagnostics:** The type-checked result has two independent stashes:

```rust
#[salsa::tracked]
struct TypeCheckedBody<'db> {
    /// The typed IR — only changes when semantics change.
    #[tracked] #[returns(ref)]
    ir: Stashed<TypedBody<'db>>,

    /// Diagnostics — can change independently without invalidating IR consumers.
    #[tracked] #[returns(ref)]
    diagnostics: Stashed<Diagnostics<'db>>,
}
```

This gives good incrementality: downstream consumers that only read the IR (e.g., codegen, further analysis) are not invalidated when error messages improve or diagnostics are reordered. The two stashes are tracked independently by salsa.

**In the IR:** Errors are marked with lightweight sentinels — `Ty::Error` for types, and possibly an `ExprKind::Error` variant for expressions. These indicate "something went wrong" but carry no diagnostic detail. The IR remains valid and evaluable (hitting an error at runtime produces a failure).

**In the diagnostics stash:** Full diagnostic information — span, message, expected/actual types, candidate lists, suggestions. Keyed by span or node position so they can be correlated with the IR when needed (e.g., for IDE display).

**Inference variables that never resolve:** Promoted to `Exactly(Ty::Error)` during finalization. The diagnostic stash records "type annotations needed" with the relevant span.

## Pseudocode sketch: type-checking a function

This section sketches the high-level flow of type-checking a function body, showing how the components fit together.

### Setup: creating the inference context

```rust
fn type_check_body(db: &dyn Db, fn_sym: FnSymbol, body: &ResolvedBody) -> TypeCheckedBody {
    // 1. Create the versioned egraph (which owns the stash and variable table).
    let mut egraph = VersionedEGraph::new();
    
    // 2. Import the function's signature into the egraph's stash.
    //    GenericParams (X, Y) stay as Ty::Param — they're already stable symbols.
    //    The signature's types are cloned from the external stash into the
    //    egraph's stash in one pass (TyFolder), with GenericParams preserved as-is.
    let sig = egraph.import_signature(fn_sym.signature(db));
    //    sig.params and sig.ret now live in the egraph's stash,
    //    containing Ty::Param for generic parameters.
    
    // 3. Build the runtime + context.
    let mut ctx = InferCtx {
        egraph,
        runtime: Runtime::new(),
        current_universe: Universe(1),  // fn-level
    };
    
    // 4. Keep the root scope open while jointly driving source tasks,
    //    inference wakes, obligations, quiescence recovery, and finalization.
    let typed_body = ctx.drive_body_to_completion(async {
        check_block(&ctx, body.root_block, sig.ret).await
    });
    
    // 5. Package results only after every scope and obligation is terminal.
    TypeCheckedBody { ir: ctx.egraph.stash.seal(typed_body), diagnostics: ... }
}
```

### Creating inference variables

```rust
impl InferCtx {
    /// Allocate a fresh inference variable at the current universe.
    /// Returns the TyIndex (stash Ptr) for the InferVar node.
    /// Note: &self because InferCtx is shared across concurrent tasks;
    /// internal mutation guarded by RefCell.
    fn fresh_ty_var(&self) -> TyIndex {
        // Allocates one globally unique ID owned by the explicit context
        // version.
        let idx = self.egraph.alloc_var(self.version, VarInfo {
            owner: self.version,
            created_universe: self.current_universe,
        });
        
        // Allocate InferVar(idx) in the stash — unique TyData → unique TyIndex
        self.egraph.stash.alloc(Ty { data: Ty::InferVar(idx) })
    }
}
```

### Importing external signatures (substitution + clone)

When type-checking a call like `foo.bar(x)` where `bar` is from another item:

```rust
impl InferCtx {
    /// Import a foreign signature into the egraph's stash, substituting its
    /// GenericParams with the given type arguments (concrete types or InferVars).
    fn instantiate_sig(&self, sig: &Stashed<Binder<FnSig>>, args: &[TyIndex]) -> FnSig {
        // Uses the Substitute folder (TyFolder machinery).
        // Clones types from the foreign stash into our stash, replacing
        // each GenericParam listed in the binder's `generics` with the
        // corresponding arg. If a type arg is an InferVar, the resulting
        // type contains that InferVar.
        let subst: FxHashMap<GenericParam, SubstTarget> = sig.value.generics
            .iter()
            .zip(args.iter())
            .map(|(param, arg)| (*param, SubstTarget::Ty(*arg)))
            .collect();
        let mut folder = Substitute {
            source: sig.stash(),
            target: &mut self.egraph.stash,
            subst,
        };
        folder.fold_fn_sig(sig.root())
    }
}
```

### Checking expressions (the core recursive walk)

```rust
impl RExpr {
    /// Check this expression, returning its type.
    /// `expected` is the type we expect this expression to produce (if known).
    async fn check(&self, ctx: &InferCtx, expected: Option<TyIndex>) -> TyIndex {
        let expr = self;
    match &expr.kind {
        // --- Literals ---
        RExprKind::IntLiteral(_) => {
            // If expected type is a specific integer type, use it.
            // Otherwise create a fresh inference var (will be pinned later).
            match expected {
                Some(ty) if ty.is_integer() => ty,
                _ => ctx.fresh_ty_var(),
            }
        }
        
        // --- Local variable reference ---
        RExprKind::Local(local_id) => {
            ctx.local_type(*local_id)  // already assigned during let-binding
        }
        
        // --- Struct literal ---
        RExprKind::StructLit { path, fields } => {
            let (struct_sym, type_args) = resolve_struct_path(ctx, path);
            let sig = ctx.instantiate_sig(&struct_sym.signature(db), &type_args);
            
            // Check each field value against the expected field type
            for (field_expr, field_sig) in fields.iter().zip(sig.fields.iter()) {
                let field_ty = field_expr.check(ctx, Some(field_sig.ty)).await;
                ctx.require_coerce(field_ty, field_sig.ty);
            }
            
            // The struct literal's type is the ADT with the given args
            ctx.alloc_ty(Ty::Adt(struct_sym.into(), type_args))
        }
        
        // --- Method call ---
        RExprKind::MethodCall { receiver, method_name, args } => {
            let receiver_ty = receiver.check(ctx, None).await;
            
            // Resolve the method — may need to await receiver_ty if it's
            // still an inference var. For inherent methods on known types,
            // this is immediate.
            let method = ctx.resolve_method(receiver_ty, method_name).await;
            let fn_sig = ctx.instantiate_sig(&method.signature(db), &method_args);
            
            // Check arguments in parallel
            let arg_checks = args.iter().zip(fn_sig.params.iter()).map(|(arg, param_ty)| {
                arg.check(ctx, Some(*param_ty))
            });
            let arg_types = futures::future::join_all(arg_checks).await;
            
            // Equate each arg with its param
            for (arg_ty, param_ty) in arg_types.iter().zip(fn_sig.params.iter()) {
                ctx.require_coerce(*arg_ty, *param_ty);
            }
            
            fn_sig.ret
        }
        
        // --- If/else ---
        RExprKind::If { condition, then_branch, else_branch } => {
            // Condition must be bool
            let cond_ty = condition.check(ctx, Some(BOOL)).await;
            ctx.require_eq(cond_ty, BOOL);
            
            // Both branches checked in parallel, must produce same type
            let result_ty = expected.unwrap_or_else(|| ctx.fresh_ty_var());
            let (then_ty, else_ty) = futures::future::join(
                then_branch.check(ctx, Some(result_ty)),
                else_branch.check_maybe(ctx, Some(result_ty)),
            ).await;
            
            ctx.require_coerce(then_ty, result_ty);
            ctx.require_coerce(else_ty, result_ty);
            result_ty
        }
        
        // --- Match ---
        RExprKind::Match { scrutinee, arms } => {
            let scrutinee_ty = scrutinee.check(ctx, None).await;
            let result_ty = expected.unwrap_or_else(|| ctx.fresh_ty_var());
            
            // Arms checked in parallel — each binds pattern vars and checks body
            let arm_checks = arms.iter().map(|arm| async {
                let bindings = arm.pattern.check(ctx, scrutinee_ty).await;
                ctx.with_bindings(bindings, || {
                    arm.body.check(ctx, Some(result_ty))
                }).await
            });
            let arm_types = futures::future::join_all(arm_checks).await;
            
            for arm_ty in &arm_types {
                ctx.require_coerce(*arm_ty, result_ty);
            }
            result_ty
        }
        
        // --- Block ---
        RExprKind::Block(stmts, tail) => {
            block.check(ctx, expected).await
        }
        
        // ... other expression kinds ...
    }
  }
}
```

### Checking blocks (structured concurrency)

Blocks use a moro-style structured concurrency pattern (see `~/dev/moro`). The key idea: variable bindings are introduced sequentially (they define the typing context for subsequent statements), but initializer checking and expression statements can be spawned as concurrent tasks.

```rust
impl Block {
    async fn check(
        &self,
        ctx: &InferCtx,
        scope: &TaskScope,
        expected_ret: Option<TyIndex>,
    ) -> TyIndex {
        // Process statements — bindings are sequential, checks are spawned
        for stmt in &self.stmts {
            match stmt {
                Stmt::Let { pattern, ty_annot, initializer } => {
                    // Determine the declared type (annotation or fresh var)
                    let declared_ty = match ty_annot {
                        Some(ty) => *ty,
                        None => ctx.fresh_ty_var(),
                    };
                    
                    // Bind pattern variables immediately — subsequent
                    // statements can reference them right away
                    let bindings = pattern.check(ctx, declared_ty).await;
                    ctx.push_bindings(bindings);
                    
                    // Spawn initializer check as a concurrent task —
                    // it will equate with declared_ty when it completes
                    if let Some(init) = initializer {
                        scope.spawn(async move {
                            let init_ty = init.check(ctx, Some(declared_ty)).await;
                            ctx.require_coerce(init_ty, declared_ty);
                        });
                    }
                }
                
                Stmt::Expr(expr) => {
                    scope.spawn(async move {
                        expr.check(ctx, None).await;
                    });
                }
            }
        }
        
        // Tail expression determines the block's type (not spawned — we need the result)
        match &self.tail {
            Some(expr) => expr.check(ctx, expected_ret).await,
            None => UNIT_TY,
        }
    }
}
```

The structured concurrency scope ensures all spawned tasks within a block complete before the block's parent can observe their effects. The typing context grows sequentially (let bindings are visible to later statements), but the *validation* of each binding's initializer runs concurrently.

### Core constraint operations

This is the original pre-solver sketch for the three constraint operations,
plus a coercion placeholder. It is illustrative rather than the current API
contract: the Trait Solving RFD replaces direct speculative mutation with
explicit-version transactions and routes trait predicates through obligations.

```rust
impl InferCtx {
    /// Record an equality which has already been proven by a trusted rule.
    /// Source where-clauses are not unconditional equality facts.
    fn assume_eq(&self, a: Ptr<Ty>, b: Ptr<Ty>) {
        self.egraph.union(a, b);
        // Triggers congruence propagation via the worklist
    }
    
    /// Check that `a` and `b` can be equal. Structural descent;
    /// sets bounds on inference vars; errors on mismatch.
    /// Used for invariant positions (generic type args, pattern bindings).
    fn require_eq(&self, a: Ptr<Ty>, b: Ptr<Ty>) {
        let a = self.egraph.find(a);
        let b = self.egraph.find(b);
        if a == b { return; }
        
        let a_data = self.egraph.stash[a];
        let b_data = self.egraph.stash[b];
        
        match (a_data.data, b_data.data) {
            // Two inference vars: union in egraph + spawn watcher
            (Ty::InferVar(_), Ty::InferVar(_)) => {
                self.egraph.union(a, b);
                self.runtime.spawn(watch_equality(a, b));
            }
            
            // Inference var meets concrete type: set bound + union
            (Ty::InferVar(_), _) => {
                self.egraph.set_bound(a, Bound::Exactly(b));
                self.egraph.union(a, b);
            }
            (_, Ty::InferVar(_)) => {
                self.egraph.set_bound(b, Bound::Exactly(a));
                self.egraph.union(a, b);
            }
            
            // Two concrete types: structural descent into args
            (Ty::Adt(sym_a, args_a), Ty::Adt(sym_b, args_b))
                if sym_a == sym_b =>
            {
                for (ai, bi) in args_a.iter().zip(args_b.iter()) {
                    self.require_eq(*ai, *bi);
                }
            }
            (Ty::Ref(inner_a, m_a, _), Ty::Ref(inner_b, m_b, _))
                if m_a == m_b =>
            {
                self.require_eq(inner_a, inner_b);
            }
            
            // Incompatible → error
            _ => {
                self.report_type_mismatch(a, b);
            }
        }
    }
    
    /// Check that `a <: b` (a is a subtype of b).
    /// Handles lifetime variance in references, `never <: anything`.
    /// Falls back to require_eq for invariant positions.
    /// Used for: argument passing, assignment, return values, match arms.
    fn require_sub(&self, a: Ptr<Ty>, b: Ptr<Ty>) {
        let a = self.egraph.find(a);
        let b = self.egraph.find(b);
        if a == b { return; }
        
        let a_data = self.egraph.stash[a];
        let b_data = self.egraph.stash[b];
        
        match (a_data.data, b_data.data) {
            // `never` is a subtype of anything
            (Ty::Never, _) => {}
            
            // Inference vars: same as require_eq for now
            (Ty::InferVar(_), _) | (_, Ty::InferVar(_)) => {
                self.require_eq(a, b);
            }
            
            // References: covariant in the referent, contravariant in lifetime
            (Ty::Ref(inner_a, m_a, _lt_a), Ty::Ref(inner_b, m_b, _lt_b))
                if m_a == m_b =>
            {
                self.require_sub(inner_a, inner_b);
                // lifetime: lt_a outlives lt_b (deferred — lifetimes are a non-goal)
            }
            
            // Everything else: fall back to equality
            _ => self.require_eq(a, b),
        }
    }
    
    /// Check that `a` can coerce to `b`.
    /// For now, just delegates to subtyping. Later this will handle
    /// &mut T → &T, deref coercions, unsizing, etc.
    fn require_coerce(&self, a: Ptr<Ty>, b: Ptr<Ty>) {
        self.require_sub(a, b);
    }
}
```

**Call site mapping:**

| Context | Operation |
|---------|-----------|
| Trait where-clause | Solver assumption/obligation; associated bindings are deferred |
| Generic type args (`Vec<T>` vs `Vec<U>`) | `require_eq` |
| Function argument passing | `require_coerce` |
| Assignment `let x: T = expr` | `require_coerce` |
| Return value vs declared return type | `require_coerce` |
| Match arms (all must produce compatible type) | `require_coerce` |

### Method resolution (awaiting concrete types)

```rust
/// Resolve a method on a receiver type. If the receiver is still an
/// inference variable, await its bound before looking up methods.
async fn resolve_method(ctx: &InferCtx, receiver_ty: Ty, name: &str) -> FnSymbol {
    match receiver_ty.data {
        Ty::InferVar(v) => {
            // Wait until the receiver has a concrete type
            loop {
                match ctx.runtime.next_bound(v).await {
                    Some(Bound::AtLeast(ty)) => {
                        // Try to resolve with what we have
                        if let Some(method) = try_resolve_inherent(ctx, ty, name) {
                            break method;
                        }
                        // Not enough info yet, keep waiting
                    }
                    Some(Bound::Exactly(ty)) => {
                        break resolve_inherent(ctx, ty, name);
                    }
                    None => {
                        // Variable finalized without a bound — error
                        return ctx.error_method(name);
                    }
                }
            }
        }
        _ => resolve_inherent(ctx, receiver_ty, name),
    }
}
```

### Closures: new universe level

```rust
RExprKind::Closure { params, body } => {
    // Enter a new universe — variables created here can't escape
    let outer_universe = ctx.current_universe;
    ctx.current_universe = Universe(outer_universe.0 + 1);
    
    // Create inference variables for closure params at the new universe
    let param_types: Vec<TyIndex> = params.iter().map(|p| {
        match &p.ty_annot {
            Some(ty) => *ty,
            None => ctx.fresh_ty_var(),  // universe = current (inner)
        }
    }).collect();
    
    let ret_ty = ctx.fresh_ty_var();
    
    // Type-check the closure body
    let body_ty = check_expr(ctx, body, Some(ret_ty)).await;
    ctx.require_coerce(body_ty, ret_ty);
    
    // Restore universe — inference vars at the inner universe remain in the
    // egraph but universe checking prevents them from appearing in
    // bounds/equalities of outer variables.
    ctx.current_universe = outer_universe;
    
    // The closure's type references the param/ret InferVars directly.
    // They have stable TyIndex values regardless of universe nesting.
    let fn_ty = ctx.egraph.stash.alloc(Ty {
        data: Ty::FnPtr(param_types, ret_ty)
    });
    fn_ty
}
```

### Key observations from this sketch

1. **Importing signatures is the natural instantiation point.** When you clone a foreign signature into the local stash, you simultaneously apply substitutions (replacing that item's `GenericParam` references with concrete types or fresh inference vars). One operation, no separate "substitution pass."

2. **`equate` is the workhorse.** Almost every type-checking step ends with `equate(actual, expected)`. This either immediately resolves (concrete vs concrete), sets a bound (concrete vs inference var), or creates an egraph edge (inference var vs inference var).

3. **Async appears at method resolution and statement sequencing.** Most expressions check synchronously (if their sub-expressions are concrete). The await points are: (a) waiting for an inference variable to become concrete enough for method resolution, (b) joining parallel sub-expression checks.

4. **Ordinary equalities propagate incrementally, but body completion is an
   explicit phase.** The runtime, committed wakes, and trait obligations are
   driven jointly to quiescence; stalled variables are then finalized for
   recovery, tasks resume, and retained obligations receive a mandatory final
   pass.

## Implementation status

The inference modules under `crates/sage-ir/src/check/` implement the initial
infrastructure sections of this RFD. What's done and what's next:

**Done:**
- Versioned union-find with sparse per-version diffs (`egraph.rs`)
- Monotonic bounds: `None` → `AtLeast` → `Exactly` (`bound.rs`)
- Version tree with branching/discard for speculative exploration (`version.rs`)
- Skeleton decompose/recompose for generic structural type operations (`skeleton.rs`)
- Constraint operations: `require_eq`, `require_sub`, `require_coerce` (`infer_ctx.rs`)
- Universe metadata scaffolding (`Universe`, `VarInfo`); full leak enforcement is
  not implemented yet
- Finalization: promote unresolved → Error, AtLeast → Exactly
- Async runtime scaffold (`runtime.rs`)
- Async body checker (`check/expr.rs`) for the currently supported expression
  forms
- In-memory test support and integration tests under `crates/sage-ir/tests/`

**Design decisions made during implementation:**
- `TyData` compound variants store `Slice<Ptr<Ty<'db>>>` (pointer-slices), not `Slice<Ty<'db>>`. This enables zero-copy skeleton decomposition — `decompose` takes `&Stash` (read-only).
- Congruence closure currently uses explicit dependents and a rebuild worklist.
  A lazy recanon-on-find design was considered but is not the implemented
  behavior.
- No `TyIdx` newtype — uses `Ptr<Ty<'db>>` directly throughout.
- Block tail heuristic: tree-sitter wraps if/else in `expression_statement` even without a semicolon, so the checker treats the last `Expr` statement as the tail when there is no explicit tail expression.

**Follow-on work owned by dependent RFDs:**
- Globally unique inference-variable IDs with owning-version metadata
- Explicit-version egraph operations and exclusive child-to-parent collapse
- Transactional unification with occurs and universe-leak checks
- Scoped runtime tasks, real wakers, joining, and version-aware cancellation
- Kind-aware folding of lifetime and const parameters embedded in types for
  canonical query copying (they remain rigid in the solver MVP)
- Async task spawning for structured concurrency in blocks
- Surface desugarings (`?`, `for`, `.await`)
- Function calls (resolving callee signature, instantiating type args)
- Method resolution on known types

The first five items are prerequisites in the Trait Solving implementation
plan; structured body completion is coordinated with the Async Type Checker;
method lookup belongs to the Method Resolution RFD. They do not reopen this
completed foundation RFD.

## Remaining follow-ons

1. **Promotion conditions (MVP rule).** A proven exact equality may promote a
   variable early. Candidate uniqueness or a heuristic bound may not. The
   recovery promotion from `AtLeast(B)` to `Exactly(B)` happens only at the
   global stable-quiescence barrier described above; later refinements can add
   earlier sound promotion rules without changing solver results.

2. **Variance and subtyping.** How does `>=` interact with variance? `?X >= Foo<'a>` where `Foo` is covariant in its lifetime — does this mean `?X` could be `Foo<'static>`?

3. **Higher-ranked types.** How do `for<'a>` types interact with the bounds/streams model? E.g., `fn(&u32, &u32)` as a higher-ranked function type — can we avoid instantiating the binder eagerly?

4. **Cycles.** Equality cycles are rejected by transactional occurs checking.
   The MVP trait solver treats an ancestor proof cycle as inductive failure;
   coinductive auto-trait cycles are deferred to a later solver design.

5. ~~**Egraph vs. explicit propagation.** Is the congruence closure machinery
   worth its complexity, or should structural equalities be propagated only by
   solver code?~~ **Resolved:** keep congruence in the egraph. The current
   implementation uses dependents plus an explicit rebuild worklist; changing
   that representation is independent of the solver contract.

6. **Memory management.** Inference variables and egraph nodes accumulate during type checking of a function body. When is it safe to GC? Version removal handles speculative branches, but the "successful" path grows monotonically.

## Trait solving

Trait solving remains a separate subsystem. Its canonical roles, versioned
probes, result algebra, residual-obligation lifecycle, and Salsa caching
boundary are specified in the [Trait Solving RFD](../trait-solving/README.md).
This RFD supplies the inference state on which that design operates.

## Target examples

The mini-redis codebase is heavily async + trait-dependent. For the initial type inference milestone, we want functions that exercise core inference without needing trait resolution. Here are representative patterns from mini-redis (slightly simplified) and standalone examples:

### Pattern 1: Struct construction + field access + if/else

```rust
// From frame.rs (simplified)
fn array() -> Frame {
    Frame::Array(vec![])
}

fn push_bulk(&mut self, bytes: Bytes) {
    match self {
        Frame::Array(vec) => {
            vec.push(Frame::Bulk(bytes));
        }
        _ => panic!("not an array frame"),
    }
}
```

Exercises: enum variant construction, match arms producing same type, method calls on known types (`Vec::push`), `self` type propagation through match.

### Pattern 2: Option/Result matching + local type propagation

```rust
// From get.rs (simplified, no async/traits)
fn parse_frames(parse: &mut Parse) -> Result<Get, ParseError> {
    let key = parse.next_string()?;
    Ok(Get { key })
}

fn apply(self, db: &Db) -> Option<Frame> {
    let response = if let Some(value) = db.get(&self.key) {
        Frame::Bulk(value)
    } else {
        Frame::Null
    };
    Some(response)
}
```

Exercises: `?` operator (requires `From` trait — may need to stub), `if let` binding, `let` with inferred type from if/else, struct literal construction.

### Pattern 3: Numeric operations + type coercion

```rust
// From frame.rs (simplified)
fn get_line<'a>(src: &mut Cursor<&'a [u8]>) -> Result<&'a [u8], Error> {
    let start = src.position() as usize;
    let end = src.get_ref().len() - 1;

    for i in start..end {
        if src.get_ref()[i] == b'\r' && src.get_ref()[i + 1] == b'\n' {
            src.set_position((i + 2) as u64);
            return Ok(&src.get_ref()[start..i]);
        }
    }

    Err(Error::Incomplete)
}
```

Exercises: `as` casts, indexing, arithmetic, range construction, early return, reference lifetimes flowing through.

### Pattern 4: Closures + iterators (basic, no complex trait bounds)

```rust
fn check_empty(parts: &mut vec::IntoIter<Frame>) -> Result<(), ParseError> {
    if parts.next().is_none() {
        Ok(())
    } else {
        Err("expected end of frame".into())
    }
}
```

Exercises: method call on generic type, boolean conditions, `Result` construction.

### Pattern 5: Standalone examples exercising inference without traits

```rust
fn fibonacci(n: u32) -> u32 {
    let mut a = 0;
    let mut b = 1;
    for _ in 0..n {
        let tmp = b;
        b = a + b;
        a = tmp;
    }
    a
}

fn longest<'a>(x: &'a str, y: &'a str) -> &'a str {
    if x.len() > y.len() {
        x
    } else {
        y
    }
}

fn build_vec(n: usize) -> Vec<i32> {
    let mut result = Vec::new();     // type inferred from push below
    for i in 0..n {
        result.push(i as i32);
    }
    result
}

fn nested_option(x: Option<Option<i32>>) -> i32 {
    match x {
        Some(Some(v)) => v,
        Some(None) => -1,
        None => 0,
    }
}
```

Exercises: mutable locals with type narrowing, `for` loops, `as` casts, nested patterns, lifetime annotations without trait bounds, `Vec::new()` with deferred type parameter (requires knowing the method call `push(i32)` pins the type parameter).

### What's needed without trait solving

For these patterns, the inference engine needs:
1. **Forward propagation** — struct field types, enum variant payloads, function return types flow into locals.
2. **Backward propagation (limited)** — `Vec::new()` gets its type parameter
   from later usage through ordinary equality/deferred resolution; this does
   not require committing trait-solver near-misses.
3. **Match exhaustiveness isn't needed** — just the type-level: all arms produce the same type.
4. **Method resolution on known types** — `vec.push(x)` where `vec: Vec<T>` is already known. This is inherent impl lookup, not trait dispatch.
5. **`?` operator** — may need to be stubbed or handled as syntactic sugar for match + `From::from` (which is trait solving). Consider deferring.

## Non-goals (for now)

- Coercions (`&mut T → &T`, deref coercions, unsizing) — first pass uses subtyping only; coercion insertion is a later layer
- Lifetime inference and region solving (separate system, runs after type inference)
- Const generics evaluation
- Exhaustive trait coherence checking
- IDE-specific partial inference (building blocks are reusable, but the query interface is future work)
- Trait solving (separate RFD)
