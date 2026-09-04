# Implementation plan and status

No implementation step in this accepted RFD has landed yet. Each step must
leave the workspace building and keep unsupported operator paths diagnostic.

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
lands. The first implementation change should be checkpoint 1 only; it should
not mix operator lookup or literal lowering into the equality substrate.

## Implementation seams

The current code provides most of the required boundaries, but the cached
well-known-item lookups for operators are new. The expected code map is:

| Responsibility | Current seam | Planned change |
|---|---|---|
| Domain declared at variable creation | `check/infer/version.rs` (`VarInfo`, `VersionTree::alloc_var`) and `check/infer_ctx.rs` (`fresh_ty_var`) | Add `InferDomain` and a domain-aware constructor; preserve the existing constructor as `Any`. |
| Versioned class state | `check/infer/egraph.rs` (`VersionState`, `union`, `set_bound_in`) | Store effective class domains, intersect them transactionally, and include committed narrowing in revisions and wakes. |
| Equality enforcement and diagnostics | `check/infer/unify.rs` (`try_unify`, `unify_in_probe`, `bind_variable`) and `check/infer_ctx.rs` (`require_eq`) | Reject an empty domain intersection or an out-of-domain concrete type with a structured `DomainConflict`; keep probe rejection silent and render required source relations without exposing inference indices. |
| Solver cache boundary | `check/solve/goal.rs` (`CanonicalVarInfo`), `canonical.rs`, and `boundary.rs` (`apply_query_response`) | Put effective input domains in the canonical key and recreate/recheck them while importing a response through the existing child transaction. Response-local variables begin as `Any` unless their binder metadata says otherwise. |
| Literal and binary-expression checking | `check/expr.rs` (`check_literal_ty`, `check_binary_op_ty`) | Lower suffixes directly, allocate `Integer`/`Float` variables for unsuffixed literals, and replace blanket binary equality with the selected operator-trait operation. |
| Operator semantic identity | `check/resolve/mod.rs`, external module/item lookup, and a new Sage-owned well-known-item query module | Add individually cached queries such as the addition-trait query. Each resolves a fixed `core`/`std` path such as `core::ops::Add` through ordinary symbol lookup and finds the method through ordinary associated-item lookup. Return typed `TraitSymbol`/`FnSymbol` identities; missing or ambiguous results are incomplete, never proof. No new operator-specific `TcxDb` operation is required. |
| Ordinary primitive/user impl discovery | `external_syms.rs` (`external_relevant_impls`) and `check/solve/clauses.rs` | Reuse the selected trait identity with the existing relevant-impl and impl-signature queries. Do not add a primitive operator candidate or signature matrix. |
| Successful operator IR | `tytree/mod.rs` (`ResolvedCallTarget`, `CallDispatch`, `TyExprData`) | Reuse resolved static trait-call representation for overloadable operators. Reserve source-shaped `Binary` for non-overloadable/lazy forms still needing it; future codegen performs primitive recognition. |

The well-known-item queries may share an internal path-resolution helper, but
each public cached query has one semantic meaning and a typed result. The
fixed path and associated-item name are an accepted bootstrap mechanism for
now. Keeping them behind these queries lets a future first-class lang-item
source replace the lookup without changing body checking or trait solving.
Focused tests must cover successful resolution, reexports where supported,
wrong-kind results, missing/ambiguous paths, and cache reuse.

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

- [ ] Add `InferDomain::{Any, Integer, Float}` to inference-variable
  allocation and class-domain lookup.
- [ ] Implement transactional class-domain intersection, concrete membership
  checks, semantic revisions, wakes, collapse, and discard ([NIV-A1]).
- [ ] Make every exact class-changing path—including variable union, concrete
  binding, and exact-bound installation—use one fallible domain-compatibility
  check; keep unchecked egraph mutation private to validated operations.
- [ ] Preserve effective domains through both union orders and representative
  changes.
- [ ] Carry effective domains through canonical goal/response round trips
  without conflating cache keys, and reject an incompatible imported response
  transactionally ([NIV-A5]).

## Step 2: Numeric literal domains

- [ ] Lower suffixed numeric literals directly to their declared primitive
  types and unsuffixed literals to domain-constrained variables.
- [ ] Propagate domain-constrained variables through annotations, generic
  arguments, selected impls, where-clauses, and ordinary egraph wakes.
- [ ] Preserve unresolved numeric classes through inference without choosing
  `i32` or `f64`; apply explicit terminal diagnostics and recovery ([NIV-A2]).
- [ ] Add a structured unification domain-conflict error. Candidate probes
  treat it as silent rejection; required source relations render expected and
  actual source categories, report no internal variable index, and install
  `Ty::Error(ErrorReported)` recovery without a follow-on unresolved error.
- [ ] Add completed-Typed-IR assertions for constrained concrete literals and
  diagnostic-recovery assertions for unconstrained literals.

## Step 3: Trait-first operator checking and elaboration

- [ ] Replace blanket operand equality with a retained operator-trait goal and
  normalized method result for every non-short-circuit operator ([NIV-A3]).
- [ ] Add cached well-known-item queries that resolve each operator trait from
  its fixed `core`/`std` path and its method through ordinary associated-item
  lookup. Map tokens to those typed identities, then discover primitive
  implementations as ordinary `core` impls through external metadata and the
  normal trait-candidate path, with no synthetic primitive candidate or
  hardcoded signature matrix.
- [ ] Prove that operator syntax alone neither narrows operand domains nor
  equates them; import only constraints from selected impls and where-clauses.
- [ ] After selection and type finalization, emit a resolved static trait call
  for every proven non-short-circuit operation, including concrete primitive
  and generic operands; diagnose failure or ambiguity ([NIV-A4]).
- [ ] Add table-driven unit coverage for every operator-to-trait-method mapping,
  result type, mixed-integer shift, generic operator body, nonnumeric
  user-defined shift, and user-defined operator with distinct `Rhs` and
  `Output`. Assert that overloadable operators do not complete as `Binary` and
  that lazy boolean operators retain their short-circuit form.
- [ ] Add query-trace coverage proving primitive arithmetic and shift goals
  load the relevant external `core` impl identities and headers rather than a
  compiler-builtin operator rule.

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
