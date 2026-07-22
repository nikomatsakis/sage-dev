# Implementation plan and status

This RFD is a draft. Checked items represent design or implementation work
which has landed together with tests and documentation.

## Phase 1: Scheduler contract

- [ ] Define a polling round for root, producer, and scoped-task queues.
- [ ] Define fairness, priority, and starvation guarantees.
- [ ] Inventory every existing solver await point and proposed intentional
  yield point.
- [ ] Define cancellation and deadlock behavior at every scheduler layer.
- [ ] Specify schedule-independent resource charging.

## Phase 2: Alternatives and prototype

- [ ] Compare bounded custom ready queues with `FuturesUnordered`.
- [ ] Prototype a self-waking `yield_now()` under both approaches.
- [ ] Measure candidate-heavy, deep-recursive, and mostly-synchronous proofs.
- [ ] Select a design which preserves scoped borrowing and query-owned
  producer lifetime.

## Phase 3: Runtime implementation

- [ ] Make newly queued work wait for a later polling round.
- [ ] Add an intentional solver yield primitive.
- [ ] Add selected candidate and producer yield points.
- [ ] Preserve idempotent real wakers and structured cancellation.
- [ ] Keep debugging traces outside semantic query results.

## Phase 4: Verification

- [ ] Prove a repeatedly self-waking future cannot spin inside one round.
- [ ] Prove finite candidate work is not starved by a yielding infinite
  sibling before its configured resource limit.
- [ ] Randomize or systematically permute runnable ordering and assert
  identical canonical query results.
- [ ] Test absorbing `Yes` and `No` behavior around overflow and cancellation.
- [ ] Test that every cancelled borrowed future is dropped before its proof
  context.
- [ ] Update `md/design/trait-solver.md` with the accepted scheduler.
