# Implementation plan and status

This RFD is a draft. Checked items represent design or implementation work
which has landed together with tests and documentation.

## Phase 1: Progress algebra

- [ ] Define distinct sufficient-answer and necessary-envelope types.
- [ ] Define the envelope implication and monotone-strengthening laws.
- [ ] Define aggregation across live, completed, failed, ambiguous, incomplete,
  and overflowed alternatives.
- [ ] Decide whether envelopes carry substitutions only or general residual
  conditions.
- [ ] Specify universe handling and canonical response-variable normalization.

## Phase 2: Incremental reduction

- [ ] Define stable candidate identities and revisioned progress slots.
- [ ] Select an order-independent strategy for online subsumption and cancelled
  answer evidence.
- [ ] Specify which progress may become a caller hard hint.
- [ ] Specify frame-level provisional summaries and subscriber wake behavior.
- [ ] Prove that recursive consumers cannot observe unsoundly subsumed
  provisional answers.

## Phase 3: Implementation

- [ ] Add producer-owned candidate progress slots.
- [ ] Publish an initial envelope after candidate head matching.
- [ ] Publish stronger envelopes after useful nested progress.
- [ ] Derive aggregate frame progress and wake interested subscribers.
- [ ] Cancel branches only with a retained dominance certificate.
- [ ] Keep the final frame response single-shot and canonical.

## Phase 4: Verification

- [ ] Test monotone candidate and aggregate revisions.
- [ ] Test incomplete sources force the aggregate envelope to `true`.
- [ ] Test shared constructors survive anti-unification while divergent leaves
  are generalized.
- [ ] Test conditional answers cancel only fully covered pending branches.
- [ ] Test speculative candidate substitutions never leak.
- [ ] Permute candidate progress and completion order and assert identical
  final query results.
- [ ] Add recursive-answer-subsumption tests required by the accepted cycle
  model.
- [ ] Update `md/design/trait-solver.md` with the accepted publication model.
