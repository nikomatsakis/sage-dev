# Implementation plan and status

No implementation step is complete. The first `DbDropGuard::db` vertical
slice is in progress; resolved local fields, explicit built-in field
autodereference, and independently queryable associated function bodies are
the first landed pieces.

### Step 1: Completed-IR data model and finalization checks

- [x] Introduce resolved local-field identity and explicit dereference nodes,
  and materialize built-in reference dereferences used by field access.
- [ ] Introduce resolved call, coercion, borrow, dispatch, substitution, and
  the remaining explicit error forms needed by the first vertical slice.
- [ ] Represent rigid types separately from the named, associated, and opaque
  `AliasTy` family without requiring aliases to be eagerly normalized.
- [x] Introduce `Lifetime::Dummy` as the only lifetime produced by checking.
- [ ] Make successful body finalization reject unresolved names, source method
  calls, inference variables, adjustment recipes, and unsupported nodes.
- [ ] Add focused structural tests for completed-body invariants.

### Step 2: Associated methods and `DbDropGuard::db`

- [x] Give impl functions independent symbol, signature, and body queries.
- [x] Complete the derive/item path needed to represent the local `Db: Clone`
  impl.
  - [x] Preserve the source item and append a parsed `Clone` impl for the
    concrete non-generic struct shape used by `Db`.
  - [x] Give generated derive text a distinct parse-source identity linked to
    the source item.
  - [x] Resolve the generated impl's external `Clone` trait and expose it
    through trait-keyed candidate discovery.
- [ ] Supply the external `Clone` trait contract needed for the solver to
  accept and prove the generated candidate.
- [ ] Resolve `Clone::clone`, prove `Db: Clone`, and consume receiver
  adjustments into explicit tree nodes.
- [ ] Assert that call checking does not query the selected callee body.

### Step 3: Oracle and coverage boundary

- [x] Extend the shared reference model with resolved field owner/index
  identity and explicit dereference expressions.
- [x] Adapt rustc field definitions directly into shared owner/index identity
  without consulting or rewriting Sage output.
- [ ] Adapt rustc substitutions and adjustments directly into the remaining
  completed forms without consulting or rewriting Sage output.
- [x] Remove paired output normalization and make pass/fail depend on
  byte-for-byte identity of deterministic serialized output.
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
- [ ] Produce byte-identical deterministic oracle output and pass focused
  incremental-dependency tests.
