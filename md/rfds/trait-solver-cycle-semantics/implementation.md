# Implementation plan and status

This RFD is a draft. Checked items represent design or implementation work
which has landed together with tests and documentation.

## Phase 1: Semantic model

- [ ] Define the exact groundness predicate at the canonical query boundary.
- [ ] Define inductive, coinductive, unknown, and mixed-path semantics.
- [ ] Specify the result order and convergence rule for substitutions,
  residuals, ambiguity, and overflow.
- [ ] Specify deterministic combination of multiple overflow causes.
- [ ] Add worked examples for variant cycles, productive cycles, mixed cycles,
  and strictly growing ground goals.

## Phase 2: Search-graph design

- [ ] Compare provisional-stack and SCC/table-completion designs against the
  active whiteboard ownership model.
- [ ] Define stable goal-node and dependency-edge identities independent of
  requester lifetime.
- [ ] Define invalidation, rebasing or SCC completion, and producer
  cancellation behavior.
- [ ] Prove that polling order cannot change the canonical result.

## Phase 3: Resource model

- [ ] Add explicit ambiguity and overflow causes to solver results.
- [ ] Define and implement structural term-size measurement.
- [ ] Define deterministic depth, work, and fixpoint limits.
- [ ] Include configurable limits in the effective cache configuration.
- [ ] Preserve absorbing `Yes` and `No` results when siblings overflow.

## Phase 4: Implementation and verification

- [ ] Replace parent-chain-only cycle decisions with the accepted graph model.
- [ ] Add inductive and coinductive fixpoint tests.
- [ ] Add non-ground sound-incompleteness tests.
- [ ] Add ground exactness tests which do not hit a resource limit.
- [ ] Add growing-term overflow tests for every configured limit.
- [ ] Perturb polling order and assert identical canonical results and overflow
  causes.
- [ ] Update `md/design/trait-solver.md` and retire superseded MVP wording.
