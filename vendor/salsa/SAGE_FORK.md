# Temporary Sage patch

This directory is Salsa 0.26.1 with one focused observability extension for
Sage's Semantic Inspector. Every tracked-function invocation emits a balanced
`WillRequest`/`DidRequest` event pair around memo lookup and execution.
`DidRequest` reports `Executed`, `Validated`, `Reused`, or `Cancelled`.

The patch is intentionally confined to `src/event.rs`,
`src/function/fetch.rs`, `src/function/accumulated.rs`, `src/tracing.rs`, the
revision accessor in `src/database.rs` and `src/revision.rs`, and public
re-exports in `src/lib.rs`.
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
