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
  RFD](../../rfds/semantic-inspector/README.md) proposes a web-backed local and
  external symbol browser for concrete IR, signatures, Typed IR, and other
  semantic results. A reviewed exact JSON contract first drives frontend tests
  through a strict dummy server; independently constructed typed backend
  values and Axum snapshots meet it at the same bytes in the following slice.
  Persistent edits and incremental query traces follow after result
  inspection; an LSP adapter remains a future client.

Human-readable inspection supplements exact oracle conformance; it does not
replace or relax it.

## Review packets

A worked example is review-complete when it provides a compact trail for:

1. the Rust input and semantic target;
2. readable semantic output;
3. diagnostics or completeness status;
4. the cold/warm query and external-metadata dependencies;
5. behavior under at least one relevant or unrelated edit;
6. focused tests or exact oracle evidence; and
7. anchored entry points for deeper code inspection.

An artifact may expose a current limitation; evidence does not have to show
that the destination is already implemented. In that case the architecture
chapter's **Current Status** records the discrepancy and the test pins the
observed frontier.

The [module-expansion review
packet](../pipeline/module-expansion.md#review-packet) and [oracle-checked body
review packet](../examples/oracle-checked-method.md#review-packet) are the
initial examples. The Semantic Inspector will first replace ad hoc semantic
debug text with reproducible, navigable result views and later add structured
query traces and edit experiments. Exact Oracle comparison remains a separate
conformance decision.
