# Temporary Sage patch

This directory is Salsa 0.26.1 with focused observability extensions for
Sage's Semantic Inspector and architecture contracts. Every tracked-function invocation emits a balanced
`WillRequest`/`DidRequest` event pair around memo lookup and execution.
`DidRequest` reports `Executed`, `Validated`, `Reused`, or `Cancelled`.

The patch is intentionally confined to `src/event.rs`,
`src/function/fetch.rs`, `src/function/execute.rs`,
`src/function/accumulated.rs`, `src/tracing.rs`, the
revision accessor in `src/database.rs` and `src/revision.rs`, and public
re-exports in `src/lib.rs`, plus the macro path dependency in `Cargo.toml`.
Sage queries require no annotations. The cancellation guard also balances the
event pair during unwinding. Ordinary fetch and accumulator graph traversal
share the same traced memo-refresh path, so the accumulator side channel cannot
bypass observation. The intended destination is an upstream Salsa API;
the workspace `[patch.crates-io]` entry can be removed once an equivalent
release is available.

Except for those enumerated files and focused changes, the vendored source and
comments are preserved verbatim from Salsa 0.26.1. This includes upstream issue
references, historical rationale, and its crate-level unsafe-documentation
suppressions. They are inherited fork material rather than Sage-authored safety
claims; Sage's patch adds no unsafe operation. Upstream comment and safety-doc
cleanup should be contributed upstream rather than creating unrelated drift in
this temporary fork.

## Architecture contract observation

The contract prototype additionally attaches `QuerySource { file, line, name }`
to request/execution events, using the existing query configuration's location.
Matching uses file/line, never a parsed debug name. `RequestCause` distinguishes
source fetches from accumulator dependency-graph traversal. `DidExecute` and an
RAII guard in `src/function/execute.rs` balance the existing execution event
when incremental validation reexecutes a dependency without a source request.
Callbacks must not panic; consumers retain diagnostics and fail after the
session. Query panic/cancellation still balances these event scopes.

The runtime's Cargo dependency selects `../salsa-macros`, whose narrowly scoped
provenance fix is documented in its `SAGE_FORK.md`. No query wrapper or Sage
attribute processing is added to Salsa. The isolated real-Salsa fixture in
`crates/sage-meta/tests/fixtures/contracts` covers cold/warm/validated queries,
forbidden cached requests, accumulator traversal, and unwind cleanup.
