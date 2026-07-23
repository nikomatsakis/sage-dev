# Implementation plan and status

This RFD is a draft. Checked items represent work which has landed with its
tests and documentation. Each step below leaves the repository building and
keeps unsupported behavior conservative.

## Step 1: Alias type family

- [x] Add `AliasTy::{Named, Associated, Opaque}` and `Ty::Alias` with stable
  definition identity and arguments.
- [x] Extend stash allocation/copying, folding, substitution, inference
  skeletons, occurs/universe checks, canonicalization, response handling,
  display, and emitters exhaustively.
- [x] Keep all alias reveal operations conservative; no variant falls back to
  `Ty::Adt`, a debug string, or an invented hidden type.
- [x] Add structural and round-trip tests for all three identities.

## Step 2: External ADT signatures and generic defaults

- [x] Add owned `TcxDb` data and tracked lowering for external ADT generics,
  trailing type defaults, ordinary predicates, and explicit completeness.
- [x] Apply defaults in declaration order during source type lowering.
- [x] Instantiate represented external ADT predicates through the ordinary
  obligation path.
- [x] Test `IntoIter<Frame>` becoming `IntoIter<Frame, Global>` and test a
  default which references an earlier parameter.
- [x] Add semantic trace assertions for the narrow ADT-signature dependency.

## Step 3: External relevant impls and headers

- [x] Land the trait-keyed external relevant-impl operation from the Trait Impl
  Candidate Discovery RFD, including completeness and conservative self-head
  refinement.
- [x] Add a separate owned external impl-signature operation and lower external
  headers into the same checked binder shape as local impls.
- [x] Extend candidate assembly with external impl candidates without a second
  proof algorithm.
- [x] Prove `IntoIter<Frame, Global>: Iterator` and its nested
  `Global: Allocator` condition.
- [x] Verify a truth proof reads headers but no associated values or callee
  bodies.
- [x] Land a cold/warm query trace which reads only the relevant external impl
  headers; relevant-versus-unrelated edit invalidation remains in the broader
  Trait Impl Candidate Discovery RFD.

The external metadata encountered while proving this checkpoint also carries
the compiler-built-in `MetaSized` lang-item obligation. It is modeled beside
the existing structural `Sized` candidate, not as an explicit impl and not as
a rustc-solved goal. The conservative evaluator proves represented rigid
cases and reports uncertainty when an external unsized tail is unavailable.

## Step 4: Associated values and normalization operations

- [x] Extend `RawTy` and checked external signatures with associated
  projections.
- [x] Add local impl-item and external metadata associated-value operations
  keyed by impl and associated type identity, separate from impl headers and
  item enumeration.
- [x] Separate value-producing `SolverGoal` operations from residual
  `ProofGoal`, and add canonical `GoalOutput::{Proven, Type}` responses.
- [x] Add input-only `Normalize(alias) -> Type` and alias-relation operations
  using isolated candidates and output-aware answer merging; no expected type
  or output inference variable participates in normalization candidate
  selection.
- [x] Bind, copy, occurs/universe-check, cache, and import response-local
  variables which occur in a goal output.
- [x] Represent normalization assumptions distinctly from bare trait facts;
  the latter never invent an associated value.
- [x] Preserve uncertainty, residual goals, and explicit exhaustion without
  manufacturing `No` from incomplete sources.
- [x] Test local and external associated values, `Iterator::Item = Frame`,
  nested projections, identical structural aliases, ambiguity independent of
  expected output, and no unrequested associated-value reads.

The caller-side alias relation is transactional: it solves and imports the
input-only normalization result before relating that result to the expected
type. Step 5 connects the same operation to the retained body-obligation
lifecycle and drives it to a terminal result.

## Step 5: `Iterator::next` elaboration

- [x] Introduce complete name-keyed external inherent-method discovery and
  use its identity-only result to audit trait-method shadowing for an external
  rigid receiver. Sound trait fallback requires proving that no inherent
  method shadows `Iterator::next`; candidate selection remains Step 6.
- [x] Admit represented projection-bearing external method signatures.
- [x] Instantiate `Iterator::next` for
  `IntoIter<Frame, Global>: Iterator` and discharge the result projection.
- [x] Consume `&mut self` into an explicit mutable `Dummy` borrow.
- [x] Register normalization residuals in the body obligation lifecycle and
  require a terminal result before completed IR is returned.
- [x] Add a focused one-call fixture whose completed result is
  `Option<Frame>`.

## Step 6: External inherent `Option::ok_or`

- [x] Select and instantiate candidates from the complete external
  inherent-method discovery keyed by rigid receiver head and method name.
- [x] Preserve owner-generic and method-generic scopes in external function
  metadata and selected-call instantiation.
- [x] Bind `Option`'s `T = Frame` from the receiver and `ok_or`'s
  `E = ParseError` from its argument/result constraints.
- [x] Classify rustc const-only host-effect predicates separately from the
  ordinary call contract; do not discard unknown predicates.
- [x] Test lookup priority, completeness, generic rollback, and absence of
  dependencies on other `Option` methods or callee bodies.

External inherent selection precedes final shared-IR retention because the
completed `Iterator::next` shadow audit already depends on the same external
provider boundary. The focused fixture includes an applicable local trait
fallback named `ok_or`; the external inherent method wins without reading the
fallback body.
Absent external inherent metadata remains explicitly incomplete, and selected
owner/method inference runs in a discardable child version before obligations
are published. Rustc's `Destruct` lang item is recorded as const-only
incompleteness; every unknown ordinary clause still makes the signature
ineligible.

## Step 7: Completed IR and exact oracle

- [x] Retain selected function identity, dispatch, trait reference, and owner
  and method substitutions in resolved calls.
- [x] Extend the shared reference schema and both independent emitters with the
  same semantic call information.
- [x] Add an isolated exact `Parse::next` JSON snapshot and byte-identity test.
- [x] Assert that successful output contains no unresolved method, required
  projection, inference variable, unsupported placeholder, or debug type.

The shared call form now separates direct and static-trait dispatch, records
the trait `Self` and trait arguments for static dispatch, and keeps owner and
method type substitutions in distinct lists. The isolated `Parse::next`
fixture structurally checks those fields before both independent outputs are
compared with one checked-in exact snapshot. No pairwise adapter or comparator
normalization is involved.

## Step 8: Pinned mini-redis checkpoint

- [ ] Force the real `Parse::next` body under the pinned library target's
  edition, features, cfg values, and dependency world.
- [ ] Assert the documented cold semantic trace and unchanged warm reuse.
- [ ] Confirm no other mini-redis or selected external body is queried.
- [ ] Update the implementation checkpoint, destination architecture pages,
  and RFD status after the complete slice lands.
