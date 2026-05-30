---
name: next-trait-solver
description: "Guide to rustc's next-generation trait solver architecture (rustc_next_trait_solver + rustc_type_ir). Use when designing or implementing trait/impl resolution, type normalization, where-clause proving, or evaluating whether to integrate vs. reimplement the solver. Covers the full model: Interner trait, SolverDelegate, EvalCtxt, candidate assembly, canonicalization, search graph cycle detection, associated type normalization, and the NormalizesTo/AliasRelate split. Use whenever the user is working on trait solving, obligation discharge, or coherence checking — especially in the context of building an alternative Rust frontend or analysis tool."
---

# The Next-Generation Rust Trait Solver

## Overview

The "next trait solver" is rustc's rewrite of trait resolution, designed to be generic over type representations. It lives in three crates:

| Crate | Role | Reusable? |
|-------|------|-----------|
| `rustc_type_ir` (~30k LOC) | Type IR definitions, `Interner` trait, folding/visiting/relating | Yes |
| `rustc_next_trait_solver` (~15k LOC) | Solver engine: evaluation, assembly, normalization, search graph | Yes (generic over `Interner`) |
| `rustc_trait_selection` | Wires the solver to rustc's `TyCtxt`, error reporting, obligation fulfillment | No (rustc-specific) |

The solver is parameterized over an `Interner` trait. Any project can implement `Interner` and use the solver — but the trait surface is enormous (~100+ associated types and methods).

---

## Core Concepts

### Goals and Predicates

The solver proves **goals**. A goal is a predicate in a parameter environment:

```rust
struct Goal<I: Interner, P> {
    pub param_env: I::ParamEnv,   // where-clauses in scope
    pub predicate: P,             // what to prove
}
```

Predicates come in two layers:

```rust
enum ClauseKind<I: Interner> {
    Trait(TraitPredicate<I>),           // T: Trait
    Projection(ProjectionPredicate<I>), // <T as Trait>::Assoc == U
    TypeOutlives(...),                  // T: 'a
    RegionOutlives(...),                // 'a: 'b
    ConstArgHasType(...),
    WellFormed(...),
    ConstEvaluatable(...),
}

enum PredicateKind<I: Interner> {
    Clause(ClauseKind<I>),
    NormalizesTo(NormalizesTo<I>),      // internal: normalize <T as Trait>::Assoc
    AliasRelate(...),                   // internal: relate two aliases
    Subtype(SubtypePredicate<I>),
    Coerce(CoercePredicate<I>),
    Ambiguous,
}
```

`ClauseKind` represents "facts" that can appear in where-clauses. `PredicateKind` adds solver-internal operations.

### Certainty

Every evaluation returns a `Certainty`:

```rust
enum Certainty {
    Yes,                    // Definitely holds
    Maybe(MaybeInfo),       // Might hold — blocked on inference or overflow
}
```

`Maybe` carries a `MaybeCause` (either `Ambiguity` or `Overflow`) and tracking info for stalled variables.

---

## The Evaluation Loop

### Entry Point

```rust
trait SolverDelegateEvalExt: SolverDelegate {
    fn evaluate_root_goal(
        &self,
        goal: Goal<I, I::Predicate>,
        span: I::Span,
        stalled_on: Option<GoalStalledOn<I>>,
    ) -> Result<GoalEvaluation<I>, NoSolution>;
}
```

Called from outside the solver. Creates a fresh `SearchGraph` and `EvalCtxt`.

### EvalCtxt: The Solver's Working State

```rust
struct EvalCtxt<'a, D: SolverDelegate> {
    delegate: &'a D,                    // inference context
    search_graph: &'a mut SearchGraph,  // cycle detection + caching
    nested_goals: Vec<(GoalSource, Goal, Option<GoalStalledOn>)>,
    current_goal_kind: CurrentGoalKind, // Misc | CoinductiveTrait | NormalizesTo
    var_values: CanonicalVarValues<I>,  // current variable assignments
    max_input_universe: UniverseIndex,
    // ...
}
```

### Nested Goal Processing

The solver collects nested goals and processes them in a fixpoint loop:

1. `evaluate_added_goals_step()` iterates over `nested_goals`
2. Each goal is evaluated via `evaluate_goal_raw()`
3. Goals returning `Certainty::Yes` are removed
4. Goals returning `Certainty::Maybe` are re-queued
5. Loop repeats until no changes or `FIXPOINT_STEP_LIMIT` (8) is reached

**Stall optimization**: If a goal was stalled and the variables it depends on haven't changed, skip re-evaluation and return the cached `stalled_certainty`.

### Probes: Speculative Evaluation

The solver uses probes to try candidates without committing:

```rust
self.probe(|&result| ProbeKind::TraitCandidate { source, result })
    .enter(|ecx| {
        // Try candidate...
        // If Ok, constraints are kept
        // If Err(NoSolution), rolled back
    })
```

Probes snapshot inference state and roll back on failure.

---

## Candidate Assembly

When proving a trait goal `T: Trait`, the solver assembles candidates from multiple sources:

### Candidate Sources

```rust
enum CandidateSource<I: Interner> {
    Impl(I::ImplId),              // User-written impl
    BuiltinImpl(BuiltinImplSource), // Compiler-generated (auto traits, Sized, Fn*)
    ParamEnv(ParamEnvSource),     // Where-clause assumption
    AliasBound(AliasBoundKind),   // Bounds on alias types
    CoherenceUnknowable,          // Coherence: possible downstream impl
}
```

### Assembly Order

```
assemble_and_evaluate_candidates(goal)
├─ Normalize self type
├─ Assemble alias bound candidates (from alias type bounds)
├─ Assemble param env candidates (from where-clauses in scope)
├─ Assemble builtin impl candidates (auto traits, Sized, Copy, Fn*, etc.)
├─ Assemble user impl candidates (via for_each_relevant_impl)
└─ Assemble object bound candidates (for dyn types)
```

### Candidate Selection (Priority)

Candidates are merged with strict priority ordering:

1. **Trivial builtin impls** — `Certainty::Yes` with zero constraints (e.g., `(): Sized`)
2. **Non-nested alias bounds for marker traits** — preferred for auto traits to avoid lifetime extension
3. **Non-global where-bounds** — if any `ParamEnv(NonGlobal)` candidate exists, all `ParamEnv` candidates are merged and selected
4. **All alias bounds** — merged together
5. **Remaining candidates** — filtered for specialization, then merged

Merging succeeds if:
- An "always applicable" candidate exists (one with `Certainty::Yes` and no external constraints), OR
- All candidates produce identical responses

Otherwise: ambiguity.

### Builtin Candidates for Auto Traits

Auto traits (Send, Sync) use **structural decomposition**:
- `Tuple(A, B, C)` → check A, B, C individually
- `Adt(S, args)` → check all field types
- `Closure(args)` → check upvar types
- Etc.

---

## Canonicalization

Canonicalization separates a goal from its inference context, enabling global caching.

### What It Does

Replaces inference variables and placeholders with sequential canonical bound variables:

```
Before: Vec<?T> : Clone        (where ?T is inference var #42)
After:  Vec<^0> : Clone        (^0 = canonical var 0, kind = Ty)
        orig_values = [?T]     (mapping back to caller)
```

### Key Types

```rust
struct Canonical<I: Interner, V> {
    pub value: V,
    pub max_universe: UniverseIndex,
    pub var_kinds: I::CanonicalVarKinds,
}

type CanonicalInput<I> = CanonicalQueryInput<I, QueryInput<I, I::Predicate>>;
type CanonicalResponse<I> = Canonical<I, Response<I>>;

struct Response<I: Interner> {
    pub certainty: Certainty,
    pub var_values: CanonicalVarValues<I>,
    pub external_constraints: I::ExternalConstraints,
}
```

### The Full Flow

```
1. Raw goal + inference state
   ↓ canonicalize_goal()
2. CanonicalInput (context-free) + orig_values (back-mapping)
   ↓ search_graph.evaluate_goal() [cache lookup or compute]
3. CanonicalResponse (context-free constraints)
   ↓ instantiate_and_apply_query_response()
4. Constraints applied to caller's inference state
```

### Input vs Response Canonicalization

- **Input mode**: All inference vars → `UniverseIndex::ROOT`. This makes cache keys maximally general.
- **Response mode**: Variables from caller's universes → ROOT; variables created *inside* the query → keep their relative universe offsets. This communicates new universe info back.

---

## Search Graph and Cycle Detection

The search graph handles recursive goals (e.g., `T: Clone where T contains itself`).

### Structure

- **Stack**: `IndexVec<StackDepth, StackEntry>` — goals currently being evaluated
- **Provisional cache**: Results that depend on goals still on the stack
- **Global cache**: Fully-resolved results (cache key = `CanonicalInput`)

### Cycle Detection

When evaluating a goal that's already on the stack → cycle detected. The response depends on the **path kind**:

```rust
enum PathKind {
    Inductive,      // All steps inductive → initial result = NoSolution
    Unknown,        // → initial result = ambiguity
    Coinductive,    // At least one coinductive step → initial result = Yes
}
```

A step is **coinductive** if it's proving a coinductive trait (auto traits, `Sized`) or comes from a where-clause of such a trait.

### Fixpoint Iteration for Cycles

When a goal is a cycle head (other goals cycle back to it):

```
loop {
    result = compute_goal(input)
    if result == provisional_result → fixpoint reached, done
    if iterations >= FIXPOINT_STEP_LIMIT (8) → overflow
    provisional_result = result
    clear dependent provisional cache entries
    re-push goal and re-evaluate
}
```

### Provisional Cache Rebasing

When popping a cycle head, provisional entries depending on it are "rebased" — their cycle-head dependencies are updated to point through the popped entry's own dependencies. Entries with incompatible path-kind combinations are discarded.

---

## Associated Type Normalization

Normalization resolves `<T as Trait>::Assoc` to a concrete type. It uses a two-level architecture:

### AliasRelate: The Orchestrator

When the solver encounters `<T as Trait>::Assoc == U`:

1. Creates an `AliasRelate` goal
2. `AliasRelate` normalizes both sides by creating `NormalizesTo` goals with **fresh unconstrained inference variables**
3. After normalization, relates the results

```rust
// Pseudocode for AliasRelate
let lhs_var = fresh_infer_var();
add_goal(NormalizesTo { alias: lhs, term: lhs_var });
let rhs_var = fresh_infer_var();  // or just rhs if not an alias
add_goal(NormalizesTo { alias: rhs, term: rhs_var });
try_evaluate_added_goals();
relate(lhs_var, rhs_var);  // Now both should be concrete
```

### NormalizesTo: The Resolver

`NormalizesTo` goals are special-cased in three ways:

1. **Unconstrained RHS**: The solver replaces the expected term with a fresh inference var, so the RHS doesn't bias candidate selection.

2. **Nested goals returned to caller**: When `NormalizesTo` succeeds with `Certainty::Yes`, its remaining nested goals are returned to the parent `AliasRelate` goal (not resolved internally). This allows constraints from the actual RHS to propagate.

3. **CurrentGoalKind::NormalizesTo**: Changes how the response is constructed — nested normalization goals are packed into `ExternalConstraints` rather than being resolved inline.

### Normalization for Projection Types

For `<T as Trait>::Assoc`:

1. Prove `T: Trait` (the trait obligation)
2. Assemble normalization candidates (impl items, where-clause projections, builtin)
3. For an impl candidate:
   - Unify goal's trait ref with impl's trait ref
   - Verify impl where-clauses
   - Get `type_of(AssocItem)` from the impl
   - Instantiate with resolved args
   - Equate with the goal's expected term

### Rigid vs Non-Rigid Aliases

- **Rigid**: Cannot be normalized further (opaque outside defining scope, abstract projections in coherence mode). The solver treats them structurally — equating `<T as Tr>::A == <U as Tr>::A` requires `T == U`.
- **Non-rigid**: Can be normalized to a hidden type (opaque in defining scope, projections with known impls).

The `TypingMode` controls which aliases are rigid:
- `Coherence`: All aliases rigid (conservative)
- `Analysis`: Opaque types in defining scope are non-rigid; others rigid
- `PostAnalysis`: All opaques known, fully normalizable

---

## The Interner Trait (Integration Surface)

To use `rustc_next_trait_solver`, implement `Interner`. Key categories:

### DefId Types (~25 associated types)

```rust
type DefId: DefId<Self>;
type TraitId: SpecificDefId<Self>;
type ImplId: SpecificDefId<Self>;
type AdtId: SpecificDefId<Self>;
type TraitAssocTyId: SpecificDefId<Self>;
// ... ~20 more
```

### Core IR Types

```rust
type Ty: Ty<Self>;          // requires Copy + Hash + Eq + TypeFoldable + Relate + Flags
type Region: Region<Self>;  // lifetimes
type Const: Const<Self>;    // const generics
type GenericArgs: GenericArgs<Self>;
type ParamEnv: ParamEnv<Self>;
type Predicate: Predicate<Self>;
type Clause: Clause<Self>;
```

### Query Methods (the expensive part)

```rust
fn generics_of(self, def_id: Self::DefId) -> Self::GenericsOf;
fn type_of(self, def_id: Self::DefId) -> EarlyBinder<Self, Self::Ty>;
fn predicates_of(self, def_id: Self::DefId) -> EarlyBinder<Self, impl IntoIterator<Item = Self::Clause>>;
fn item_bounds(self, def_id: Self::DefId) -> EarlyBinder<Self, impl IntoIterator<Item = Self::Clause>>;
fn impl_trait_ref(self, impl_def_id: Self::ImplId) -> EarlyBinder<Self, TraitRef<Self>>;
fn for_each_relevant_impl(self, trait_def_id: Self::TraitId, self_ty: Self::Ty, f: impl FnMut(Self::ImplId));
fn for_each_blanket_impl(self, trait_def_id: Self::TraitId, f: impl FnMut(Self::ImplId));
fn explicit_super_predicates_of(self, def_id: Self::TraitId) -> EarlyBinder<Self, impl IntoIterator<...>>;
// ... ~60 more methods
```

### InferCtxtLike (Inference Context)

```rust
trait InferCtxtLike {
    fn typing_mode_raw(&self) -> TypingMode<I>;
    fn universe(&self) -> UniverseIndex;
    fn create_next_universe(&self) -> UniverseIndex;
    fn root_ty_var(&self, var: TyVid) -> TyVid;
    fn opportunistic_resolve_ty_var(&self, vid: TyVid) -> I::Ty;
    fn next_region_infer(&self) -> I::Region;
    fn sub_regions(&self, sub: I::Region, sup: I::Region, ...);
    // ... ~20 methods for managing inference state
}
```

### SolverDelegate (Solver Callbacks)

```rust
trait SolverDelegate: Deref<Target = Self::Infcx> {
    fn build_with_canonical<V>(cx: I, canonical: &CanonicalQueryInput<I, V>) -> (Self, V, CanonicalVarValues<I>);
    fn compute_goal_fast_path(...) -> Option<Certainty>;
    fn leak_check(&self, max_input_universe: UniverseIndex) -> Result<(), NoSolution>;
    fn evaluate_const(...) -> Option<I::Const>;
    fn well_formed_goals(...) -> Option<Vec<Goal<I, I::Predicate>>>;
    fn fetch_eligible_assoc_item(...) -> FetchEligibleAssocItemResponse<I>;
    fn is_transmutable(...) -> Result<Certainty, NoSolution>;
    // ~12 methods total
}
```

---

## Key Design Decisions and Their Rationale

### Why Canonicalization Instead of Salsa-Style Caching

The solver uses canonicalization + a global cache rather than Salsa's input-tracking approach because:
- Goals arise dynamically during inference (not from stable inputs)
- The same logical goal appears with different inference variable IDs across contexts
- Canonicalization normalizes these away, giving O(1) cache lookups
- Salsa would require stable goal identities, which don't exist during inference

### Why NormalizesTo Returns Nested Goals

Without this, the solver would lose inference information. If `<T as Iterator>::Item` normalizes to `u32`, and the caller is proving `<T as Iterator>::Item: Clone`, the `Clone` obligation needs to know it's actually `u32: Clone`. By returning nested goals to the parent `AliasRelate`, constraints flow both ways.

### Why Coinductive Cycles Default to Yes

For auto traits: `struct Foo { inner: Box<Foo> }` — proving `Foo: Send` requires `Box<Foo>: Send` requires `Foo: Send`. This is a coinductive cycle. The correct answer is "yes, assuming it holds" — which is sound because auto traits have no methods to call recursively.

### FIXPOINT_STEP_LIMIT = 8

Prevents infinite oscillation in pathological cycles. In practice, most cycles converge in 1-2 iterations. The limit is a safety net. When hit, the solver returns ambiguity rather than looping forever.

---

## Source Reference

| File | Contents |
|------|----------|
| `compiler/rustc_type_ir/src/interner.rs` | `Interner` trait (~470 lines) |
| `compiler/rustc_type_ir/src/infer_ctxt.rs` | `InferCtxtLike`, `TypingMode` |
| `compiler/rustc_type_ir/src/inherent.rs` | `Ty`, `Region`, `Const` helper traits |
| `compiler/rustc_type_ir/src/ty_kind.rs` | `TyKind<I>` enum |
| `compiler/rustc_type_ir/src/predicate_kind.rs` | `PredicateKind`, `ClauseKind` |
| `compiler/rustc_type_ir/src/canonical.rs` | `Canonical`, `CanonicalVarKind` types |
| `compiler/rustc_type_ir/src/search_graph/mod.rs` | Search graph: stack, caching, fixpoint |
| `compiler/rustc_type_ir/src/search_graph/stack.rs` | `Stack`, `StackEntry` |
| `compiler/rustc_type_ir/src/search_graph/global_cache.rs` | Global result cache |
| `compiler/rustc_next_trait_solver/src/delegate.rs` | `SolverDelegate` trait |
| `compiler/rustc_next_trait_solver/src/solve/mod.rs` | Solver entry, `FIXPOINT_STEP_LIMIT` |
| `compiler/rustc_next_trait_solver/src/solve/eval_ctxt/mod.rs` | `EvalCtxt`, `SolverDelegateEvalExt` |
| `compiler/rustc_next_trait_solver/src/solve/eval_ctxt/probe.rs` | Probe mechanism |
| `compiler/rustc_next_trait_solver/src/solve/assembly/mod.rs` | Candidate assembly |
| `compiler/rustc_next_trait_solver/src/solve/trait_goals.rs` | Trait goal selection/merging |
| `compiler/rustc_next_trait_solver/src/solve/normalizes_to/mod.rs` | NormalizesTo dispatch |
| `compiler/rustc_next_trait_solver/src/solve/alias_relate.rs` | AliasRelate orchestration |
| `compiler/rustc_next_trait_solver/src/canonical/mod.rs` | `canonicalize_goal`, response instantiation |
| `compiler/rustc_next_trait_solver/src/canonical/canonicalizer.rs` | Canonicalization folder |
