# Implementation plan and status

The source-neutral domain substrate and the numeric expression-literal work in
checkpoints 1 and 2 are implemented, together with the first trait-first
operator vertical slice in checkpoint 3. Unsuffixed expression literals now
create `Integer` or `Float` variables, suffixes lower directly, and failed
required domain relations report and recover without exposing inference
indices. Cached typed queries resolve all represented operator traits and
methods. Every non-lazy binary token now instantiates that method's real
signature, retains its proof and result-normalization obligations, and builds
a provisional resolved call whose failure is tied to diagnosed recovery;
`&&` and `||` remain explicit lazy boolean operations. Ground primitive,
comparison, and local user-defined cases are covered. Generic operator bodies,
unknown-`Self` candidate hints, explicit let-annotation enforcement, and the
full propagation/operator matrix remain later work. Each step must leave the
workspace building and keep unsupported operator paths diagnostic.

## Landing strategy

Land the work in dependency order rather than as one source-visible change:

1. **Domain substrate.** Add versioned domains, fallible domain-aware equality,
   and canonical round trips. Existing inference-variable constructors continue
   to allocate `Any`, so this checkpoint deliberately changes no source
   behavior.
2. **Literal behavior.** Construct domain-constrained variables for unsuffixed
   literals, lower suffixes directly, and add terminal diagnostics and recovery.
   This is the first checkpoint that closes the unsound literal-to-`bool`
   unification independently of operator elaboration.
3. **Operator proof and elaboration.** Add cached well-known-item lookups for
   the operator traits and methods, retain the trait obligation, reuse normal
   impl discovery, and emit resolved trait calls. Keep lazy boolean operators
   on their existing short-circuit path.
4. **Closure evidence.** Complete the source matrix, oracle comparisons,
   query-trace assertions, and living-document evidence before closing KD-7.

Each checkpoint is independently reviewable, keeps successful completed IR
free of inference-domain metadata, and updates the checkboxes below as it
lands. Checkpoint 1 remains a separate substrate boundary: it excludes operator
lookup and literal lowering from the equality machinery.

## Implementation seams

The current implementation and remaining operator work map to these seams:

| Responsibility | Current seam | Implementation state and planned change |
|---|---|---|
| Domain declared at variable creation | `check/infer/version.rs` (`VarInfo`, `VersionTree::alloc_var`) and `check/infer_ctx.rs` (`fresh_ty_var`) | Add `InferDomain` and a domain-aware constructor; preserve the existing constructor as `Any`. |
| Versioned class state | `check/infer/egraph.rs` (`VersionState`, `union`, `set_bound_in`) | Store effective class domains, intersect them transactionally, and include committed narrowing in revisions and wakes. |
| Equality enforcement and diagnostics | `check/infer/unify.rs` (`try_unify`, `try_unify_required`, `unify_in_probe`, `bind_variable`) and `check/infer_ctx.rs` (`require_eq`) | Reject an empty domain intersection or an out-of-domain concrete type with a structured `DomainConflict`; keep probe rejection silent and render required source relations without exposing inference indices. |
| Solver cache boundary | `check/solve/goal.rs` (`CanonicalVarInfo`), `canonical.rs`, and `boundary.rs` (`apply_query_response`) | Put effective input domains in the canonical key and recreate/recheck them while importing a response through the existing child transaction. Response-local variables begin as `Any` unless their binder metadata says otherwise. |
| Literal and binary-expression checking | `check/expr.rs` (`check_literal`) and `check/operator.rs` | Suffix lowering and `Integer`/`Float` expression-literal variables are implemented. Non-lazy binary expressions now map to the cached operator identity, instantiate its metadata signature, retain proof/normalization work, and build a provisional resolved call; lazy boolean expressions stay on the direct boolean path. |
| Operator semantic identity | `well_known.rs` (`operator_trait`, `operator_method`), `check/resolve/mod.rs`, and `TraitSymbol::items` | Cached queries resolve each fixed `core` path through ordinary symbol lookup and each method through ordinary associated-item lookup. They return typed `TraitSymbol`/`FnSymbol` identities; missing, incomplete, ambiguous, or wrong-kind results are unavailable rather than proof. Body checking consumes these identities without adding a second path/name table. |
| Ordinary primitive/user impl discovery | `external_syms.rs` (`external_relevant_impls`) and `check/solve/clauses.rs` | The first operator slice reuses the selected trait identity with the existing relevant-impl, impl-signature, and associated-value queries. Primitive `core` impls and local user impls use the same candidate path; there is no primitive operator candidate or signature matrix. A coarse `LocalAdt` self-head keeps blanket external impls while excluding rigid upstream nominal heads which cannot match a local type. Width-specific float heads likewise prove rustc's unrepresented `f16`/`f128` headers disjoint from `f32`/`f64` queries instead of letting unsupported metadata weaken completeness. |
| Successful operator IR | `check/operator.rs`, `check/infer/obligations.rs`, `check/finalize.rs`, and `tytree/mod.rs` | A non-lazy expression receives a provisional static `ResolvedCall`; proof and normalization obligations retain its recovery slot, and finalization admits only the settled call or an error node. Source-shaped `Binary` is reserved for lazy `&&`/`||`; future codegen performs primitive recognition. Generic operator bodies are not yet supported. |

The well-known-item queries share the ordinary resolver and trait-item
enumeration, while their enum keys cache every trait and method identity
separately. The fixed path and associated-item name are an accepted bootstrap
mechanism for now. Keeping them behind these queries lets a future first-class
lang-item source replace the lookup without changing body checking or trait
solving. Focused tests cover all twelve traits and sixteen methods, external
export identity, missing/incomplete/ambiguous/wrong-kind results, narrow
metadata demand, and cache reuse.

## First landing checkpoint

Checkpoint 1 is complete only when all of the following hold without changing
literal or operator behavior:

- every fresh variable has an explicit effective domain, with legacy call
  sites receiving `Any`;
- variable union and exact concrete binding share one fallible membership
  path, including exact-bound installation;
- either union order preserves the domain intersection, failed work rolls
  back, and committed narrowing publishes the expected revision and wakes;
- canonicalization distinguishes otherwise equal goals whose existential
  inputs have different domains;
- canonical response application recreates the caller constraints and rejects
  an incompatible substitution without leaking child state; and
- focused unit tests cover `Any ∩ Integer`, `Integer ∩ Float`, an integer
  variable unified with `f32`, representative integer/float members, branch
  collapse, and branch discard.

The checkpoint gate is a formatted workspace build plus the focused inference
and canonical-response tests. Source-level numeric tests belong to checkpoint
2, not this substrate commit.

## Step 1: Versioned inference domains

- [x] Add `InferDomain::{Any, Integer, Float}` to inference-variable
  allocation and class-domain lookup.
- [x] Implement transactional class-domain intersection, concrete membership
  checks, semantic revisions, wakes, collapse, and discard ([NIV-A1]).
- [x] Make every exact class-changing path—including variable union, concrete
  binding, and exact-bound installation—use one fallible domain-compatibility
  check; keep unchecked egraph mutation private to validated operations.
- [x] Preserve effective domains through both union orders and representative
  changes.
- [x] Carry effective domains through canonical goal/response round trips
  without conflating cache keys, and reject an incompatible imported response
  transactionally ([NIV-A5]).

## Step 2: Numeric literal domains

- [x] Lower suffixed numeric literals directly to their declared primitive
  types and unsuffixed literals to domain-constrained variables.
- [ ] Propagate domain-constrained variables through annotations, generic
  arguments, selected impls, where-clauses, and ordinary egraph wakes.
- [x] Preserve unresolved numeric classes through inference without choosing
  `i32` or `f64`; apply explicit terminal diagnostics and recovery ([NIV-A2]).
- [x] Render the structured unification domain-conflict error at required
  source relations using expected and actual source categories, report no
  internal variable index, and install `Ty::Error(ErrorReported)` recovery
  without a follow-on unresolved error. Candidate probes already treat the
  structured conflict as silent rejection.
- [x] Add completed-Typed-IR assertions for constrained concrete literals and
  diagnostic-recovery assertions for unconstrained literals.

Ordinary equality propagation is now covered through generic calls, nested ADT
assignment, and a retained rigid-head impl obligation which observes later
domain narrowing. The broader propagation item remains open because explicit
let type annotations are not yet related to their pattern or initializer, and
the full where-clause, wake, and isolation matrix has not been assembled. After
a required relation fails, its source-only transaction retains the unresolved
classes represented by the conflicting types before rollback, including class
members joined provisionally during that transaction. Recovery does not poison
unvisited sibling components of the larger structural relation; it replays
compatible sibling relations after installing the primary error.

## Step 3: Trait-first operator checking and elaboration

- [x] Replace blanket operand equality with a retained operator-trait goal and
  normalized method result for every non-short-circuit operator ([NIV-A3]).
- [x] Add cached well-known-item queries that resolve each operator trait from
  its fixed `core` path and each method through ordinary associated-item
  lookup. Return typed `TraitSymbol`/`FnSymbol` identities; unavailable,
  ambiguous, or wrong-kind lookup is not proof.
- [x] Map binary tokens to those identities in body checking, then discover
  primitive implementations as ordinary `core` impls through external
  metadata and the normal trait-candidate path, with no synthetic primitive
  candidate or hardcoded signature matrix.
- [ ] Prove that operator syntax alone neither narrows operand domains nor
  equates them; import only constraints from selected impls and where-clauses.
- [ ] After selection and type finalization, emit a resolved static trait call
  for every proven non-short-circuit operation, including concrete primitive
  and generic operands; diagnose failure or ambiguity ([NIV-A4]). Ground
  primitive and local user-defined cases now use this path, including
  obligation-linked recovery, but generic bodies remain open.
- [x] Add table-driven identity-lookup coverage for all twelve operator traits
  and sixteen methods, supported external exports, unavailable and wrong-kind
  results, narrow metadata demand, and cache reuse.
- [ ] Add operator-checking coverage for result types, mixed-integer shift,
  generic operator bodies, nonnumeric user-defined shift, and a user-defined
  operator with distinct `Rhs` and `Output`. Assert that overloadable operators
  do not complete as `Binary` and that lazy boolean operators retain their
  short-circuit form.
- [x] Add query-trace coverage proving primitive arithmetic and shift goals
  load the relevant external `core` impl identities and headers rather than a
  compiler-builtin operator rule.

The current source-driven checkpoint covers `u32 + u32`, `f32`/`f64`
addition, `u8 << i16`, `==` and `<` with signature-required shared borrows, a
local `Left + Right -> Sum` impl, and an unsuffixed right operand narrowed by
the selected `u32` impl. It also checks one-diagnostic recovery for
`bool + bool`, `u32 + bool`, and an unresolved comparison proof; recovery
isolation between equivalent calls; current unknown-`Self` ambiguity; both
lazy boolean tokens; and cold/warm `core` metadata demand. An independent
oracle comparison matches the representative successful forms exactly, with
additional oracle coverage for a comparison operand carrying `NeverToAny`.
This establishes a reviewable ground-operation slice of [NIV-A3] and
[NIV-A4], not their generic or full-matrix closure.

## Step 4: Source and oracle closure evidence

- [ ] Add source-driven positive cases for constrained integer/float literals,
  annotations, selected-impl and where-clause inference, boolean operators,
  comparisons, arithmetic, bitwise operations, and mixed-integer shifts.
- [ ] Add negative cases for `1 + true`, integer/float mixing, non-boolean lazy
  operators, invalid bitwise operands, invalid shifts, and unconstrained
  numeric literals which this RFD deliberately does not default.
- [ ] Compare representative accepted and rejected cases with the independent
  rustc oracle's fixed projection into Sage's trait-call schema, and inspect
  successful/recovery Typed IR ([NIV-A2], [NIV-A3], [NIV-A4], [NIV-A5]).
- [ ] Close KD-7 only after its complete closure evidence passes.

## Step 5: Documentation and completion

- [ ] Promote every living NIV anchor and its required verification into the
  Type Inference and Body Checking architecture chapters.
- [ ] Update both chapters' Current Status evidence and remove the implemented
  limitations.
- [ ] Mark the roadmap slice complete, move this RFD to Completed, and record
  any accepted design revisions.

## Completion checklist

- [ ] Every living RFD design anchor and its required verification is promoted
  into the relevant Architecture & Design chapter.
- [ ] The chapters' Current Status sections link the evidence which establishes
  each implemented anchor and state any remaining gap.
- [ ] Preserve [D20], which places overloadable-operator specialization after
  Typed IR at the future codegen boundary.
- [ ] The affected roadmap slice and RFD status indexes are updated.

[NIV-A1]: ./README.md#niv-a1
[NIV-A2]: ./README.md#niv-a2
[NIV-A3]: ./README.md#niv-a3
[NIV-A4]: ./README.md#niv-a4
[NIV-A5]: ./README.md#niv-a5
[D20]: ../../design/decisions.md#d20-overloadable-operators-remain-trait-calls-through-typed-ir
