# Implementation plan and status

No implementation step in this accepted RFD has landed yet. Each step must
leave the workspace building and keep unsupported operator paths diagnostic.

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
- [ ] Map the token only to its operator language-item trait/method; discover
  primitive implementations as ordinary `core` impls through external
  metadata and the normal trait-candidate path, with no synthetic primitive
  candidate or hardcoded signature matrix.
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
