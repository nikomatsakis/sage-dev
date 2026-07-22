# RFD: Trait Solving

**Status:** Accepted

**Depends on:**
- [Trait System](../trait-system/README.md) — `TraitRef`, `WherePredicate`,
  `ImplSignature`, and local impl discovery
- [Type Inference](../type-inference/README.md) — `VersionedEGraph`, inference
  variables, universes, and the async executor
- [Async Type Checker](../async-type-checker/README.md) — scoped task lifetime,
  real wakeups, joining, and cancellation

## Goal

Design the trait solver: given a goal such as `Vec<?X>: Debug`, determine
whether it follows from the assumptions at the proving site and the positive
trait impls in the current crate. The solver uses versioned inference state for
speculation and coordinates repeated atomic sub-proofs through a per-execution
whiteboard.

This RFD specifies the positive, inductive MVP and records the design used to
implement it. It is not the destination contract for candidate discovery,
scheduling, incremental progress, or recursive proof semantics. Those tenets
live in [Trait Solver Design](../../design/trait-solver.md); planned changes are
split across the
[Candidate Discovery](../trait-impl-candidate-discovery/README.md),
[Cycle Semantics](../trait-solver-cycle-semantics/README.md),
[Scheduling and Fairness](../trait-solver-scheduling/README.md), and
[Incremental Results](../incremental-trait-results/README.md) RFDs. The local
all-impl scan, parent-chain cycle cutoff, and depth-only overflow rule below are
MVP behavior, not commitments to the final query or search architecture.

## MVP contract

The first implementation is intentionally type-only, positive, and
inductive:

- **Atoms:** `TraitImpl { self_ty, trait_ref }` and `Equals(lhs, rhs)`, where
  every argument is a type.
- **Structural goals:** `All`, `Exists`, `Implies`, and `Maybe`.
- **Clauses:** assumptions at the proving site first, followed by positive local
  impl clauses whose trait signatures are available locally. The Trait System
  owns deterministic `local_impls(db, LocalCrateSymbol)` discovery; the solver
  linearly scans that list.
- **Cycles:** an atomic goal recurring in its own parent-frame chain is `No`.
  Reaching the fixed recursion limit of 64 is `Maybe`.
- **Alternatives:** all possible answers are considered. Only an unconditional
  `Yes` with no caller-variable bindings may terminate sibling alternatives
  early.
- **Equality:** structural unification is transactional. It runs in an egraph
  child version and collapses that child into its parent only after the entire
  operation, including occurs and universe-leak checks, succeeds.
- **Residuals:** unresolved but still-possible goals are returned in `modulo`,
  registered as obligations, and retried only when relevant inference state
  changes.

The following are explicitly outside the MVP: lifetime/outlives goals,
goal-level `ForAll`, associated-type normalization, higher-ranked bounds,
auto and other builtin traits, negative impls, external-crate impl discovery,
local impls of external traits whose defining predicates are unavailable,
coherence, specialization, and method resolution. These features require
additional data-model or proof-search rules; the MVP does not approximate them
as successes.

Generic impl declarations still have binders. Candidate assembly opens such a
binder with fresh existential variables for that candidate. This is clause
instantiation, not a `Goal::ForAll` proof rule.

## External interface

### Goal and query types

```rust
#[salsa::interned]
struct GoalQuery<'db> {
    data: Stashed<GoalQueryData<'db>>,
}

struct GoalQueryData<'db> {
    /// Selects the Trait System's deterministic local-impl list.
    local_crate: LocalCrateSymbol<'db>,
    /// The caller's current universe relative to this query's universe base.
    canonical_universe: u32,
    /// Metadata for every free alpha-equivalent input in `assumptions` and
    /// `goal`, in deterministic first-appearance order. Binder-local params
    /// are canonicalized in place but do not appear here.
    canonical_vars: Slice<CanonicalVarInfo<'db>>,
    /// First alpha-equivalent parameter index not occupied anywhere in the
    /// canonical query, including nested binder-local parameters.
    next_response_param: u32,
    /// False when unsupported/deferred source predicates were diagnosed and
    /// omitted from `assumptions`.
    assumptions_complete: bool,
    assumptions: Slice<Assumption<'db>>,
    goal: Goal<'db>,
}

struct CanonicalVarInfo<'db> {
    param: AlphaEquivParam<'db>,
    kind: GenericParamKind,
    role: CanonicalVarRole,
    /// Universe relative to the query's universe base.
    relative_universe: u32,
}

enum CanonicalVarRole {
    /// A caller generic parameter. It is rigid inside the query.
    RigidPlaceholder,
    /// A caller inference variable. The solver may constrain it.
    ExistentialInput,
}

enum Goal<'db> {
    Exists(Binder<'db, Ptr<Goal<'db>>>),
    Implies(Slice<Assumption<'db>>, Ptr<Goal<'db>>),
    All(Slice<Goal<'db>>),
    Atom(Atom<'db>),
    /// The goal is still possible but cannot currently be decided.
    Maybe,
}

enum Atom<'db> {
    TraitImpl {
        self_ty: Ptr<Ty<'db>>,
        trait_ref: TraitRef<'db>,
    },
    Equals(Ptr<Ty<'db>>, Ptr<Ty<'db>>),
}

enum Assumption<'db> {
    /// A trait fact with an empty body. Equality assumptions are not part of
    /// the positive MVP environment.
    TraitImpl {
        self_ty: Ptr<Ty<'db>>,
        trait_ref: TraitRef<'db>,
    },
    /// A positive Horn clause: `conditions => consequence`.
    Implies(Slice<Goal<'db>>, Ptr<WherePredicate<'db>>),
    /// Convenience representation flattened while building the environment.
    All(Slice<Assumption<'db>>),
}
```

Both MVP atoms prove only propositions, so their successful semantic output is
unit-like `Proven`. The MVP implementation omits that uniform output from its
response representation. The destination generalization is introduced with
normalization: solver operations carry goal-specific outputs, and input-only
`Normalize(alias)` returns a type. This later design is specified by
[Trait Solver Design](../../design/trait-solver.md#knowledge-returned-by-the-solver)
and the [Associated Type Normalization RFD](../associated-type-normalization/README.md).
`Equals` is goal-only: hypothetical equality environments need a scoped egraph
model and are deferred, so `Assumption` heads are statically trait-only.

### Result types

```rust
/// Variables created while extracting a response. All are existential and
/// use indices at or after `GoalQueryData::next_response_param`, so they cannot
/// collide with a free input or nested binder-local parameter.
struct ResponseVarInfo<'db> {
    param: AlphaEquivParam<'db>,
    kind: GenericParamKind,
    relative_universe: u32,
}

struct QueryResult<'db> {
    bound_vars: Slice<ResponseVarInfo<'db>>,
    value: QueryResultData<'db>,
}

enum QueryResultData<'db> {
    Yes {
        /// Definite equalities learned for existential query inputs.
        subst: Subst<'db>,
        /// Goals which must still be discharged under their captured
        /// environment.
        modulo: Goal<'db>,
    },
    /// At least one alternative remains possible, but no unique definite
    /// answer exists. Hints are necessary, not sufficient, conditions.
    Maybe {
        hints: Subst<'db>,
    },
    /// Candidate sources were exhaustive and every candidate was definitively
    /// disproven.
    No,
}

/// Keys are always type-kind canonical variables whose role is
/// `ExistentialInput`. Rigid type/const placeholders may occur inside
/// values but never as keys.
type Subst<'db> = Slice<(AlphaEquivParam<'db>, Ptr<Ty<'db>>)>;

/// Internal to one proof context. Equalities live in that context's explicit
/// egraph version, so they are not repeated here.
enum GoalResult<'db> {
    Yes { modulo: Goal<'db> },
    Maybe,
    No,
}
```

This is the implemented MVP response shape. Its `value` field names the
`Yes`/`Maybe`/`No` payload; it is not the semantic goal output. The planned
normalization extension adds a goal-specific output to `Yes`, while retaining
substitution and `modulo` as separate response knowledge. Variables appearing
in that output join the response binder and the existing validation boundary.

The result is stashed at every canonical/egraph boundary. Instantiation
allocates one caller inference variable for each `bound_vars` entry, preserving
its relative universe, and uses the same mapping across `subst` and `modulo`.
While importing `modulo`, a separate push/pop binder remapper freshens every
nested `Exists` declaration and its bound occurrences in the requester. Free
query inputs, response existentials, and residual-local binder parameters are
three disjoint namespaces; numeric alpha-parameter overlap in independently
canonicalized requester/response objects must never capture an occurrence.

A definite `Yes` response preserves sharing. If one fork-local variable occurs
twice, both occurrences refer to the same response variable. For example,
`?X = (T, T)` is not weakened to `?X = (?A, ?B)`. Only `Maybe` hints may use
the deliberately shallow, non-repeating anti-unification form.

`Maybe::hints` contains only hard necessary equalities common to every
still-possible alternative. A heuristic near-miss such as “this impl would
apply if `?X` were `u32`” is not a hard hint and must not be committed to the
caller. Ranking or exposing such suggestions is separate, deferred diagnostic
work rather than part of the MVP proof result.

The `bound_vars` on a `Maybe` bind its hints just as they bind a `Yes`
substitution and residual. Hint merging therefore operates on a binder-aware
value, not on a bare `Subst`. After anti-unification, entries of the form
`?Input = fresh ?Response` whose response variable occurs nowhere else are
tautologies and are removed, followed by pruning and renumbering unused
response variables. Applying an ambiguous hint must add real information; it
must not create a fresh alias and a semantic revision on every retry.

### Entry point and caching boundary

```rust
#[salsa::tracked]
impl<'db> GoalQuery<'db> {
    #[salsa::tracked]
    pub fn prove(self, db: &'db dyn Db) -> Stashed<QueryResult<'db>>;
}
```

`GoalQuery::prove` is the synchronous Salsa boundary. Salsa may reuse the
completed result of the same canonical query, including its crate and
environment. An execution that Salsa actually runs creates a fresh inference
context, runtime, and whiteboard. The whiteboard deduplicates in-progress work
only within that execution; it is not another global or cross-query cache.

## Canonicalization

The caller canonicalizes the goal and assumptions together. It chooses an
absolute universe base and retains that base in the caller-side mapping rather
than the cached key. With free inputs, the base is the lowest current ceiling
among the inputs and the caller's current universe; with no free inputs, the
base is the caller's current universe. `canonical_universe` records the current
universe relative to that base, so two otherwise-identical queries at
meaningfully different binder depths do not share an invalid cached result.

Canonicalization then proceeds as follows:

1. Walk both in deterministic order and assign one `AlphaEquivParam` to each
   distinct caller variable.
2. Record `kind`, `role`, and universe relative to the selected base in
   `CanonicalVarInfo`. For an existential inference input, this is its current
   versioned universe ceiling, not immutable creation universe; a lowered
   ceiling therefore participates in the canonical query/cache identity.
3. Map `Ty::Param` values to `RigidPlaceholder` and inference variables to
   `ExistentialInput`. An inference variable appearing in an assumption stays
   existential; canonicalization never generalizes it into a universal.
   Const parameters embedded in a type are visited and alpha-renamed as rigid
   placeholders. Checked lifetimes are already `Dummy`; the MVP has no flexible
   lifetime or const inference inputs.
4. On entering `Goal::Exists`, allocate deterministic canonical parameters for
   its declarations in binder order, push a binder mapping, fold the body, and
   pop it. Bound occurrences use that mapping and are excluded from
   `canonical_vars` and the caller reverse map. Nested binders are
   capture-avoiding; the type-only MVP rejects non-type `Exists` declarations.
5. Record a reverse mapping from each free canonical parameter to the original
   caller generic or egraph inference variable.
6. Record the first unused alpha-parameter index after both free and bound
   query parameters as `next_response_param`, preserve the source parameter
   environment's completeness bit, include `LocalCrateSymbol`, and allocate the
   complete query in a fresh stash.

When importing a query, the solver starts at `canonical_universe`, instantiates
rigid inputs as rigid placeholders and existential inputs as inference
variables in the declared relative universes, and opens nested binders with a
capture-avoiding binder stack. Rigid placeholders cannot become the left-hand
side of a substitution or be bound by unification. When applying a response,
only existential-input entries are mapped back into the caller egraph. The
caller adds its retained absolute universe base to every response-relative
universe; this is defined even for a closed query with no canonical inputs.

Response extraction records relative universes for variables created during
proof. Applying a response must run the universe-leak check before committing
it, so a response-local variable cannot escape into an older caller universe.

Universe validation also constrains flexible variables nested inside a binding.
If `?X@U0 = Vec<?T@U1>`, the transaction lowers `?T`'s current universe ceiling
to `U0` (or fails if that ceiling cannot be changed); checking only for an
immediate rigid placeholder would allow `?T` to bind a `U1` placeholder later
and leak indirectly. Universe ceilings are versioned sparse state, separate
from each variable's immutable creation universe. Unification computes a
fixed point: an eclass takes the minimum ceiling of its flexible variables,
nested flexible variables in structural members are lowered to that ceiling,
and a rigid placeholder above it rejects the transaction. All lowering rolls
back with the probe. Response variables record their lowered ceiling, and
reapplying a substitution reproduces any required lowering on caller inputs.
A committed ceiling decrease is a semantic variable update: it advances the
relevant revision and wakes obligations whose canonical identity/accessibility
depends on that input. A discarded decrease publishes neither revision nor
wake.

The same accessibility validator gates streaming bound writes. Although a
`Bound` contains no inference variables, its concrete type may contain a rigid
placeholder; `set_bound(?X@U0, AtLeast/Exactly(P@U1))` is rejected before
publication. Equality, substitution application, and bound tightening cannot
use separate leak rules.

Extraction projects each egraph class into a deterministic response normal
form before allocating response variables:

- a class containing only flexible variables and one or more existential query
  inputs uses the lowest canonical input as its representative;
  candidate-local variables fold to that input, and only the other query inputs
  need equality entries;
- a class containing a rigid placeholder, concrete type, or structured type
  uses its fully substituted, canonically ordered rigid/structural form, so an
  equality such as
  `?Input = Vec<?Local>` retains the meaningful `Vec` constructor;
- a class containing only proof-local variables and no query input gets one
  response existential if it is reachable from the substitution or residual.

The same projection folds `subst` and `modulo`, then prunes and renumbers unused
response variables. Thus `exists<T>. ?X = T` extracts no binding, two inputs
equated through the same candidate variable extract a deterministic
input-to-input equality, and `exists<T>. ?X = T && T: Bound` rewrites the
residual to `?X: Bound` instead of leaking a vacuous response alias. This
normalization must be independent of union-find root choice and candidate
execution order.

## Inference transactions and explicit versions

Every `ProofCtx` names an explicit egraph `Version`; no solver operation relies
on one mutable global `current` version. A candidate gets a child version of
its parent, and concurrent candidate futures retain independent version
handles. All reads, writes, variable allocation, rebuilds, and wake
registrations are performed against that explicit handle.

Per-version mutation is leaf-only. Once a version has a live child, its
inherited state is frozen until all children are discarded or its sole child is
atomically collapsed. Sparse ancestry lookup otherwise would let a later parent
write change the snapshot observed by every existing child. Frozen parents use
read-only lookup; path compression, variable allocation, equality/bound writes,
rebuild/revision changes, and inference-wake publication target child leaves.
Append-only global stash allocation remains safe because it does not alter an
existing version's inference facts.

Inference-variable IDs are globally unique within the egraph and record their
owning version. A version may access variables owned by itself or its
ancestors, never a sibling. Consequently, a `Ty::InferVar` allocated by one
branch cannot be reinterpreted in a sibling. Concurrent candidates additionally
own distinct egraphs, so their raw variable identities never share a stash or
proof context. Version identities are stable for one egraph lifetime or carry
a generation when storage slots are reused.

The egraph therefore needs these operations before concurrent alternatives can
be implemented:

- branch from a specified parent version;
- perform all inference operations against a specified version;
- collapse a successful child into its parent atomically;
- discard a failed/cancelled child and its descendants;
- buffer version-local inference wakeups, publishing them only on collapse;
- cancel tasks tied to a discarded version before recycling it.

Collapsing has a strict safety precondition: the child being collapsed is the
only live child of its parent. The MVP uses this simple rule so committing a
probe cannot change inference state underneath a live sibling. Concurrent
candidates use independent proof contexts; each extracts a response from its
local child transaction and then drops the context after joining.

Structural unification and response application use a nested child as a
transactional probe:

1. create a child of the context version;
2. apply every recursive equality or substitution entry in that child;
3. reject recursive types with an occurs check;
4. reject universe leaks;
5. preserve the inference engine's `Ty::Error` recovery behavior;
6. rebuild congruence and validate the complete result;
7. collapse the child on success, or discard it on any failure.

This makes a multi-entry substitution atomic and prevents a failed recursive
unification from leaving partial bindings. It also ensures inference-bound
wakeups become visible only for committed state. Variables created in a
committed probe keep their globally unique IDs and are atomically re-owned by
the parent; variables from discarded descendants cannot appear in parent
state.

Candidate contexts are not collapsed into a requester: different alternatives
may make incompatible choices. Each candidate extracts a canonical response
from its local child and then drops the isolated context. Applying the chosen
aggregate response to a caller is a separate transactional operation.

## Internal architecture

### The whiteboard

The whiteboard is an active proof tree. An atomic frame is keyed by its
canonical atom/environment/depth **and its parent frame**. Exact duplicates
under the same parent share a future; equal goals reached through different
parents remain separate frames. Parent links are also used for cycle detection.

Frame production is requester-independent. Each new frame imports its
canonical key into a fresh producer-owned proof stash and egraph and runs in a
query-owned producer scope. Candidate alternatives likewise get independent
proof contexts and perform matching in a local child transaction. A producer
therefore never inherits or borrows the first requester's candidate version.
After extracting and stashing its response, it joins all nested work and drops
its isolated proof context before publishing the result.

Each returned frame future owns a subscription allocated at lookup time.
Polling an incomplete frame updates that subscription with `cx.waker()`, and
dropping the future removes it. A creator's cancellation therefore cannot stop
a producer while another subscriber remains. If the last subscriber to an
incomplete frame disappears, the whiteboard removes the key, requests producer
cancellation, joins its nested work, and drops its proof context; a later
lookup starts a new frame rather than waiting on the cancelled one. Completion
takes and wakes every remaining subscriber. See the
[whiteboard implementation sketch](./impl-sketches/whiteboard.md).

For an atomic request at depth `D`:

1. If `D >= 64`, return `Maybe` without creating a frame.
2. Walk the parent-frame chain. If the same canonical atom and environment
   appears in an ancestor (depth is ignored for this comparison), return `No`.
3. Reuse or create the exact parent-keyed frame.
4. If newly created, start its isolated canonical frame context in the
   query-owned producer scope; otherwise await its future. The producer is not
   a child of the requesting candidate's scope or egraph version.
5. Instantiate the completed response transactionally in the requesting
   context.

The query-owned scope cannot exit until every frame producer has completed or
has been cancelled and drained. Cancellation drops all nested candidate tasks
and subscriptions before dropping the producer-owned proof context. This lets
a losing candidate drop its own isolated context immediately after releasing
its subscriptions, without invalidating a shared producer still needed by a
surviving candidate.

The MVP is inductive, so a cycle is never treated as success. Structural auto
trait recursion is deferred with auto traits rather than simulated by injecting
self-assumptions.

### Structural goals

`prove_goal` decomposes non-atomic structure:

- `Atom` uses the whiteboard.
- `All` runs the sequential conjunction fixpoint.
- `Exists` allocates fresh inference variables in the current explicit version
  and current universe, then proves its body. The existing `Binder` carries
  generic identities, not separate universe metadata.
- `Implies` extends the environment for the inner proof. If the inner proof
  returns residual `R`, the outer result stores
  `Implies(original_assumptions, R)` so retrying it cannot lose the environment
  on which it depended.
- `Maybe` returns `Maybe`.

### Atomic dispatch and candidates

Atomic orchestration dispatches by atom kind. `Equals(lhs, rhs)` directly runs
transactional structural unification; it does not attempt trait candidate
assembly. `TraitImpl { self_ty, trait_ref }` assembles alternatives in this
order:

1. flattened environment assumptions whose heads can match the atom;
2. positive impls found by linearly scanning
   `local_impls(db, GoalQueryData::local_crate)` from the Trait System.

Before assembly, a trait atom whose self type or trait arguments contain
`Ty::Error` returns an empty-substitution, trivial-residual recovery `Yes`.
The original diagnostic is already represented by the error sentinel; emitting
`No` here would create a misleading secondary trait failure. This recovery
answer adds no inference fact other than terminating the obligation.

When `self_ty` is a bare flexible variable, step 1 still runs: an explicit
environment assumption may prove the goal without guessing a receiver type.
The MVP then skips the impl scan and records that omitted search as an
unconstrained possible alternative. If no environment clause proves the goal
unconditionally, the result is `Maybe`, not `No`, and the retained obligation's
existential inputs provide its conservative retry set.

An incomplete source parameter environment follows the same rule. Known direct
assumptions remain usable and an unconditional proof still wins, but failure of
the represented subset cannot produce `No`; omitted unsupported facts
contribute an unconstrained possible alternative and therefore `Maybe`.

Each generic clause binder is opened with fresh variables in its own candidate
version. Head unification and the complete clause body are proved there. A
failed candidate is discarded with no parent mutation.

An impl clause is exposed only when both `ImplSignatureData` and its referenced
local `TraitSignatureData` are `SolverEligibility::Eligible`. A potentially
relevant `Unsupported` signature is an incomplete candidate source, not an
irrelevant impl: it contributes `Maybe` unless an environment or other
unconditional candidate already proves the goal. This prevents a diagnosed,
unrepresented const binder or predicate from becoming either an empty
unconditional clause or a false exhaustive `No`.

For an external trait, the MVP may still prove a goal directly from an
environment assumption. Otherwise its impl sources and defining predicates are
not exhaustive, so failure of the known environment candidates yields
`Maybe`/unsupported rather than definitive `No`. `No` is returned only when
the enabled candidate sources are complete for that atom.

### Alternatives

Applicable candidates run concurrently in separate proof contexts, each with a
local child transaction. The runtime provides scoped tasks, a join point, and
cancellation. Returning from an atomic proof is forbidden while a sibling
future can still access its context. If an unconditional answer wins early,
the scope cancels and drains every sibling before their proof states are
dropped.

Merging retains every `Yes` answer until all alternatives finish, then computes
the non-dominated set from the complete answer relation. This avoids making a
conservative, not-necessarily-transitively-recognized subsumption check depend
on arrival order. A directional check may remove a `Yes` only when another
answer is proven at least as general. `Maybe` is tracked separately and is
never treated as `No`.

Every non-`No` result also contributes a binder-aware hint input containing
both its response variables and its substitution. The accumulator represents
these as an optional nonempty collection: absence means no possible answer has
been seen, whereas a collection containing an empty substitution is a real,
maximally weak hint. At finalization, the collection is alpha-renamed,
canonically ordered, and anti-unified as one n-ary operation.

Each fresh witness introduced at a divergent hint occurrence receives the
relative universe of that occurrence's substitution key. Because shallow
anti-unification does not share such witnesses across keys, this is the most
general universe which remains bindable by that input. The merged value is
then leak-checked against the key; an inaccessible rigid placeholder cannot be
hidden behind a newly chosen witness universe.

Hint anti-unification is an intersection over substitution keys: a key absent
from any still-possible answer is unconstrained and is removed from the hard
hint. For keys present in every answer, their values are structurally
anti-unified. This is what makes the result a necessary condition rather than
a union of candidate-specific near-misses. Tautological bare fresh witnesses
are removed; useful shared outer structure such as `Vec<?A>` is retained.

After all alternatives complete:

- an unconditional `Yes { subst: [], modulo: All([]) }` proves the proposition
  regardless of any `Maybe` and wins;
- no `Yes` and no `Maybe` means `No`;
- exactly one non-dominated `Yes` and no `Maybe` means that `Yes`;
- otherwise, any `Maybe` or multiple incomparable `Yes` answers means `Maybe`
  with the anti-unification of every still-possible answer's
  hints/substitution.

Only `Yes { subst: [], modulo: All([]) }` may return before all alternatives
finish. Even an empty-hint `Maybe` must wait because an unconditional `Yes` may
still arrive. See the [solving implementation sketch](./impl-sketches/solving.md).

### Directional subsumption

`A.subsumes(B)` asks whether `Cond(B) => Cond(A)`. The MVP check is
directional:

- variables and caller bindings established by antecedent `B` are frozen
  rigid placeholders;
- only response-local existential witnesses belonging to consequent `A` may
  be bound;
- response variables use their lowered universe ceilings, and consequent
  witnesses may not cross a frozen antecedent's accessibility boundary;
- proof-condition equalities and residual matching may not refine the
  antecedent.

Thus an unconditional answer subsumes `?X = u32`, but `?X = u32` does not
subsume an unconditional answer by binding the antecedent's `?X`. Residual
conjunctions use a conservative canonical subset check, recognizing
`X && Y => X`; cases requiring trait implication remain incomparable and yield
`Maybe`.

### Conjunction fixpoint

The whole conjunction runs in an exclusive child version. A `No` result
discards that child, rolling back equalities committed by earlier conjuncts; a
possible or successful result collapses it only after all nested work has
joined and the child is again exclusive. Within that child, conjuncts run
sequentially. Each pending goal records the last pair of
`(fully substituted goal, semantic egraph
revision)` attempted. It is retried only when that pair changes. A partial
`Yes` replaces the pending goal with its normalized residual and records that
residual as already attempted at the post-proof revision. Thus even a rule
which produces a different, ever-growing residual on each proof attempt cannot
spin at one parent depth. A sibling's later semantic update changes the pair
and permits one useful retry.

Fully proven conjuncts are removed, `No` fails the conjunction, and remaining
stable goals become `All(residuals)`. Hints applied by one conjunct can advance
the semantic revision or change a sibling's substituted form, causing exactly
the required retry.

## Type-checker integration and obligation lifecycle

The body checker owns an obligation set. Each entry contains the canonical
goal, its canonical-to-caller mapping, the caller variables on which it is
stalled, and diagnostic provenance (originating span and reason).

The MVP derives a conservative wake set from every `ExistentialInput` occurring
in the retained goal and environment. Solver-reported fine-grained stall
reasons may narrow that set later, but correctness does not depend on that
optimization.

Obligation producers use the same path. Besides explicit trait checks, the call
checker instantiates every positive type predicate in an ordinary callee's
`CheckedParameterEnv`; the method checker does the same for the selected method
inside its selected-method transaction; and ADT construction/use instantiates
the ADT environment as well-formedness obligations. Each represented
`WherePredicate` lowers to a fixed `TraitImpl` goal. An ineligible environment
is diagnosed/retained as unsupported rather than treated as an empty set.

1. Canonicalize a requested goal and call `GoalQuery::prove`.
2. Apply a `Yes` substitution transactionally. Remove the obligation only when
   `modulo` is trivially true; otherwise register the residual. An `Implies`
   residual already contains the environment it needs. Record the residual as
   attempted at the current dependency revision so it is not immediately
   re-proved into an unbounded residual chain without new caller information.
3. Apply hard `Maybe` hints transactionally. Because they are necessary across
   every still-possible alternative, a conflict disproves the original goal;
   otherwise retain the original goal as an obligation. Heuristic near-misses
   are not applied here.
4. Deduplicate equivalent obligations and wake them only after a relevant
   caller variable receives a committed semantic update.
5. Re-canonicalize and re-prove a woken obligation. If neither its canonical
   form nor its dependency revision changed, do not busy-loop.
6. Body completion uses a quiescence loop while the root runtime scope remains
   open. Drain runnable expression/constraint tasks, publish committed wakes,
   and process ready obligations together until none makes semantic progress.
   A suspended expression task is not required to finish before this point; it
   may be waiting for exactly the inference or obligation update being driven.
7. At stable quiescence, finalize the still-unresolved inference variables
   (`AtLeast` to its final bound, otherwise `Ty::Error` recovery), publish those
   wakes, and return to step 6. Tasks resumed by recovery may visit more source,
   create more variables, or submit more obligations, so quiescence and
   finalization repeat.
8. If all inference variables are final but stable obligations or method-lookup
   waits remain, re-prove them once and complete them with a diagnostic/error
   outcome for `No`, `Maybe`, or a non-trivial residual. Wake their waiting
   expression tasks and return to step 6; a no-variable unsupported obligation
   must not deadlock the root task which is awaiting it.
9. Completion requires the root expression and every scoped task to join and
   every obligation to be terminal. Pending tasks with no variable or
   obligation wait are an internal runtime deadlock, not successful
   quiescence. No task, waiter, branch, or obligation may be silently dropped.

Method resolution is not part of this integration step. The
[Method Resolution RFD](../method-resolution/README.md) owns trait/method
candidate enumeration and submits a fixed post-deref `LookupSelfTy: Trait` goal
to the solver's proof operation. Its successful output is `Proven`; the solver
does not need to return selected impl evidence for that contract.

## Deferred work

- lifetime and outlives proving;
- hypothetical equality assumptions and scoped equality environments;
- goal-level universals and higher-ranked bounds;
- projection representation and value-producing associated-type normalization;
- auto/builtin traits and coinductive search;
- external impl discovery through compiler metadata;
- negative reasoning, coherence, overlap, and specialization;
- solver-backed implication for precise answer subsumption;
- impl indexing beyond the MVP linear local scan;
- cross-execution in-progress deduplication beyond Salsa's completed-result
  memoization;
- method-resolution enumeration and integration, owned by the
  [Method Resolution RFD](../method-resolution/README.md).
