# Validation and Inspection

Validation answers whether Sage satisfies its architectural and semantic
contracts; inspection makes those contracts reviewable without first reading
every implementation detail.

- The [Oracle Test Harness](../oracle-test-harness.md) compares independently
  emitted Sage and rustc reference IR by exact textual identity.
- [Examples](../examples.md) walk from small Rust programs into anchored code
  excerpts and semantic output.
- Focused tests, readable snapshots, and query traces provide evidence for the
  claims in each architecture chapter's **Current Status** section.
- The draft [Semantic Inspector
  RFD](../../rfds/semantic-inspector/README.md) proposes a persistent CLI and
  future LSP-backed interface for typed IR and incremental query traces.

Human-readable inspection supplements exact oracle conformance; it does not
replace or relax it.
