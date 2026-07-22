# Implementation plan and status

This RFD is a draft. Checked items represent design or implementation work
which has landed together with tests and documentation.

## Phase 1: Candidate-universe contract

- [ ] Define precisely which local and upstream crates belong to the visible
  impl universe.
- [ ] Define how builtin, auto, negative, and specialization candidates join
  user-defined impl discovery.
- [ ] Define candidate-source completeness in the presence of unavailable or
  unsupported metadata.
- [ ] Specify stable local and external impl identities.

## Phase 2: Query and index design

- [ ] Define the mandatory trait-keyed candidate API.
- [ ] Define the conservative simplified-self-type key and fallback bucket.
- [ ] Select a fine-grained Salsa representation for local impl indexing.
- [ ] Extend `TcxDb` with trait signatures, impl signatures, and relevant-impl
  enumeration using owned metadata values.
- [ ] Define deterministic ordering and deduplication across local, external,
  and fallback sources.

## Phase 3: Semantic implementation

- [ ] Index local impl signatures by trait without reading unrelated trait
  impls during lookup.
- [x] Import represented external trait signatures through `TcxDb`; external
  impl signatures and enumeration remain open.
- [x] Expose local impls of external traits once defining predicates are
  complete, while retaining source incompleteness until external relevant-impl
  enumeration exists.
- [ ] Add simplified-self-type filtering with no false negatives.
- [ ] Replace the solver's direct `local_impls` scan with the accepted
  candidate API.
- [ ] Preserve incomplete-source behavior until every relevant source exists.

## Phase 4: Required tests

- [ ] Expose a structured query recorder from the test harness while retaining
  Salsa `WillExecute` and typed `TcxDb` events.
- [ ] Normalize semantic keys and internal Salsa IDs for stable assertions.
- [ ] Add cold, warm-cache, relevant-edit, and unrelated-edit goal traces.
- [ ] Land every semantic discovery test listed in the RFD.
- [ ] Compare indexed and exhaustive results over generated fixtures.
- [ ] Land Salsa event tests for unrelated-trait invalidation.
- [ ] Land signature-versus-body invalidation tests.
- [ ] Land self-head bucket invalidation tests when that index is enabled.
- [ ] Land stable external-metadata identity and reuse tests.
- [ ] Update `md/design/trait-solver.md`, the Trait System architecture, and
  the MVP limitation notes after the indexed global path lands.
