# Validation and Inspection

Validation answers whether Sage satisfies its architectural and semantic
contracts; inspection makes those contracts reviewable without first reading
every implementation detail.

- The [Oracle Test Harness](../crates/sage_oracle_harness/oracle-guide.md) compares independently
  emitted Sage and rustc reference IR by exact textual identity.
- [Examples](../examples.md) walk from small Rust programs into anchored code
  excerpts and semantic output.
- Focused tests, readable snapshots, and query traces provide evidence for the
  claims on their generated anchor pages.
- [`sage-meta`](language.md) checks correspondence among named
  Markdown specification rules, `.sage` models, and Rust declarations/tests.
  Its library supplies both build-time validation and runtime call checking;
  CI and workspace tests use the shared validation entry point.
- The [Semantic Inspector](../crates/sage_inspector/inspector-guide.md) is the
  web-backed local and external symbol browser for concrete IR, signatures,
  Typed IR, and other semantic results. Source-driven tests exercise its live
  service and JSON boundary; persistent edits and incremental query traces make
  query boundaries reviewable. An LSP adapter can reuse the inspection service
  as a future client.

Human-readable inspection supplements exact oracle conformance; it does not
replace or relax it.

{anchor}`claim_specific_evidence`
Evidence is attached to one architectural claim. A statement that a behavior is established identifies evidence exercising that
particular behavior, while a negative or partial artifact may pin a known
limitation. A passing aggregate suite is not evidence for an unstated local
guarantee.

Verification scenarios: Documentation review checks every implemented
capability against a focused test, snapshot, normalized trace, edit
experiment, exact oracle result, inspector scenario, or source anchor whose
asserted behavior matches the claim.

{anchor}`independent_inspection`
Human inspection and exact conformance remain independent.
Readable Sage-only views explain semantic values and incremental work; the
Oracle independently decides compatibility from Sage and rustc emissions.
Neither facility consumes the other's output to repair or reinterpret a
result.

Verification scenarios: Architecture and dependency tests keep inspector
adapters out of oracle comparison and rustc-oracle values out of inspector
rendering, while one pinned slice supplies both independently produced forms
of evidence.

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
that the destination is already implemented. In that case the rule's explicit TODO implementation/test entries describe
the discrepancy, and the test pins the observed frontier. Its generated anchor
page exposes both the limitation and the associated evidence.

{anchor}`review_packets`
A review packet connects result, dependency, edit, and code
evidence. For one semantic target, the packet identifies the Rust input,
readable result, diagnostic/completeness outcome, cold and warm dependencies,
relevant or unrelated edit behavior, focused or oracle verification, and
anchored implementation entry points.

Verification scenarios: Each designated review packet contains or links
all seven elements, and its reproduction commands regenerate the checked-in
artifacts without relying on unrecorded setup.

## Evidence navigation

The generated anchor pages show implementation excerpts, complete annotated
tests, explicit gaps, and saved adequacy reviews. Source associations and reviews
do not themselves establish that a test ran. The [worked examples](../examples.md)
remain explanatory review trails; their coverage is accounted for by the
corresponding rules rather than a second manually maintained status table.
