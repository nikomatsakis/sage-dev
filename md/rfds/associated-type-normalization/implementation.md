# Implementation plan and status

This RFD is a draft. Checked items represent work which has landed with its
tests and documentation. Each step below leaves the repository building and
keeps unsupported behavior conservative.

## Step 1: Alias type family

- [ ] Add `AliasTy::{Named, Associated, Opaque}` and `Ty::Alias` with stable
  definition identity and arguments.
- [ ] Extend stash allocation/copying, folding, substitution, inference
  skeletons, occurs/universe checks, canonicalization, response handling,
  display, and emitters exhaustively.
- [ ] Keep all alias reveal operations conservative; no variant falls back to
  `Ty::Adt`, a debug string, or an invented hidden type.
- [ ] Add structural and round-trip tests for all three identities.

## Step 2: External ADT signatures and generic defaults

- [ ] Add owned `TcxDb` data and tracked lowering for external ADT generics,
  trailing type defaults, ordinary predicates, and explicit completeness.
- [ ] Apply defaults in declaration order during source type lowering.
- [ ] Instantiate represented external ADT predicates through the ordinary
  obligation path.
- [ ] Test `IntoIter<Frame>` becoming `IntoIter<Frame, Global>` and test a
  default which references an earlier parameter.
- [ ] Add semantic trace assertions for the narrow ADT-signature dependency.

## Step 3: External relevant impls and headers

- [ ] Land the trait-keyed external relevant-impl operation from the Trait Impl
  Candidate Discovery RFD, including completeness and conservative self-head
  refinement.
- [ ] Add a separate owned external impl-signature operation and lower external
  headers into the same checked binder shape as local impls.
- [ ] Extend candidate assembly with external impl candidates without a second
  proof algorithm.
- [ ] Prove `IntoIter<Frame, Global>: Iterator` and its nested
  `Global: Allocator` condition.
- [ ] Verify a truth proof reads headers but no associated values or bodies.
- [ ] Land cold/warm and relevant/unrelated query-trace tests.

## Step 4: Associated values and normalization goals

- [ ] Extend `RawTy` and checked external signatures with associated
  projections.
- [ ] Add local impl-item and external metadata associated-value operations
  keyed by impl and associated type identity, separate from impl headers and
  item enumeration.
- [ ] Add canonical `NormalizesTo` and alias-relation operations using fresh
  output variables, isolated candidates, and ordinary answer merging.
- [ ] Represent normalization assumptions distinctly from bare trait facts;
  the latter never invent an associated value.
- [ ] Preserve uncertainty, residual goals, and explicit exhaustion without
  manufacturing `No` from incomplete sources.
- [ ] Test local and external associated values, `Iterator::Item = Frame`,
  nested projections, identical structural aliases, ambiguity independent of
  expected output, and no unrequested associated-value reads.

## Step 5: `Iterator::next` elaboration

- [ ] Admit represented projection-bearing external method signatures.
- [ ] Instantiate `Iterator::next` for
  `IntoIter<Frame, Global>: Iterator` and discharge the result projection.
- [ ] Consume `&mut self` into an explicit mutable `Dummy` borrow.
- [ ] Register normalization residuals in the body obligation lifecycle and
  require a terminal result before completed IR is returned.
- [ ] Add a focused one-call fixture whose completed result is
  `Option<Frame>`.

## Step 6: External inherent `Option::ok_or`

- [ ] Add complete external inherent-method discovery keyed by rigid receiver
  head and method name.
- [ ] Preserve owner-generic and method-generic scopes in external function
  metadata and selected-call instantiation.
- [ ] Bind `Option`'s `T = Frame` from the receiver and `ok_or`'s
  `E = ParseError` from its argument/result constraints.
- [ ] Classify rustc const-only host-effect predicates separately from the
  ordinary call contract; do not discard unknown predicates.
- [ ] Test lookup priority, completeness, generic rollback, and absence of
  dependencies on other `Option` methods or callee bodies.

## Step 7: Completed IR and exact oracle

- [ ] Retain selected function identity, dispatch, trait reference, and owner
  and method substitutions in resolved calls.
- [ ] Extend the shared reference schema and both independent emitters with the
  same semantic call information.
- [ ] Add an isolated exact `Parse::next` JSON snapshot and byte-identity test.
- [ ] Assert that successful output contains no unresolved method, required
  projection, inference variable, unsupported placeholder, or debug type.

## Step 8: Pinned mini-redis checkpoint

- [ ] Force the real `Parse::next` body under the pinned library target's
  edition, features, cfg values, and dependency world.
- [ ] Assert the documented cold semantic trace and unchanged warm reuse.
- [ ] Confirm no other mini-redis or selected external body is queried.
- [ ] Update the implementation checkpoint, destination architecture pages,
  and RFD status after the complete slice lands.
