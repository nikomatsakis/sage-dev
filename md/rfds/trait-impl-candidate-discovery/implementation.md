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
- [x] Specify stable local and external impl identities.

## Phase 2: Query and index design

- [x] Define the mandatory trait-keyed candidate API.
- [x] Define the conservative simplified-self-type key and fallback bucket.
- [x] Select a stable tracked index with a private tracked map and keyed
  backdated lookup methods for local impl indexing.
- [x] Extend `TcxDb` with trait signatures, impl signatures, and relevant-impl
  enumeration using owned metadata values.
- [ ] Define deterministic ordering and deduplication across local, external,
  and fallback sources.

## Phase 3: Semantic implementation

- [ ] Index local impl signatures by trait without reading unrelated trait
  impls during lookup.
- [x] Import represented external trait signatures, relevant explicit-impl
  identities, and impl headers through separate `TcxDb` operations.
- [x] Expose local impls of external traits once defining predicates are
  complete and merge them with the reachable external candidate source.
- [x] Add no-false-negative simplified-self-type filtering to external
  metadata discovery; local indexing remains open.
- [x] Replace the solver's direct `local_impls` scan with the accepted
  candidate API.
- [x] Preserve incomplete-source behavior for unavailable candidate sources
  and unsupported individual headers.

## Phase 4: Required tests

- [ ] Expose a structured query recorder from the test harness while retaining
  Salsa `WillExecute` and typed `TcxDb` events.
- [ ] Normalize semantic keys and internal Salsa IDs for stable assertions.
- [ ] Add cold, warm-cache, relevant-edit, and unrelated-edit goal traces.
- [ ] Land every semantic discovery test listed in the RFD.
- [ ] Compare indexed and exhaustive results over generated fixtures.
- [ ] Land Salsa event tests proving unrelated-trait edits stop at an equal
  keyed lookup result.
- [ ] Land signature-versus-body invalidation tests.
- [ ] Land self-head bucket invalidation tests when that index is enabled.
- [ ] Land stable external-metadata identity and reuse tests.
- [ ] Update `md/design/trait-solver.md`, the Trait System architecture, and
  the MVP limitation notes after the indexed global path lands.
