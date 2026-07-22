# Implementation plan and status

No implementation step is complete. This RFD remains a draft and is not yet
eligible for implementation.

### Step 1: Completed-IR data model and finalization checks

- [ ] Introduce resolved call, field, coercion, borrow, dereference, dispatch,
  substitution, and explicit error forms needed by the first vertical slice.
- [ ] Represent rigid types separately from the named, associated, and opaque
  `AliasTy` family without requiring aliases to be eagerly normalized.
- [ ] Introduce `Lifetime::Dummy` as the only lifetime produced by checking.
- [ ] Make successful body finalization reject unresolved names, source method
  calls, inference variables, adjustment recipes, and unsupported nodes.
- [ ] Add focused structural tests for completed-body invariants.

### Step 2: Associated methods and `DbDropGuard::db`

- [ ] Give impl functions independent symbol, signature, and body queries.
- [ ] Complete the derive/item path needed to represent the local `Db: Clone`
  impl.
- [ ] Resolve `Clone::clone`, prove `Db: Clone`, and consume receiver
  adjustments into explicit tree nodes.
- [ ] Assert that call checking does not query the selected callee body.

### Step 3: Oracle and coverage boundary

- [ ] Extend the shared reference model with the completed forms from Step 1.
- [ ] Normalize rustc definitions, substitutions, and adjustments into those
  forms.
- [ ] Enumerate associated bodies and reject unsupported successful output.
- [ ] Compare `DbDropGuard::db` and assert a stable semantic query trace.

### Step 4: Trait-directed expression families

- [ ] Land the separate normalization design needed to represent and solve
  associated-type projections while preserving unnormalized alias bounds.
- [ ] Elaborate overloaded operators, indexing, callable values, `for`, and
  `?` into their resolved semantic operations.
- [ ] Add one focused fixture and query-dependency assertion per family.

### Step 5: Closures and async

- [ ] Represent closure captures and nested typed bodies.
- [ ] Represent async bodies and high-level `await` without state-machine
  lowering.
- [ ] Complete the `Shutdown::recv` vertical slice.

### Step 6: Full mini-redis library coverage

- [ ] Account for every item and body in the default-feature library target.
- [ ] Reach zero unsupported successful nodes and zero unexpected diagnostics.
- [ ] Pass normalized oracle comparison and focused incremental-dependency
  tests.
