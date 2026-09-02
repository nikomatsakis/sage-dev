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
- The [Semantic Inspector](./semantic-inspector.md) is the
  web-backed local and external symbol browser for concrete IR, signatures,
  Typed IR, and other semantic results. Source-driven tests exercise its live
  service and JSON boundary; persistent edits and incremental query traces make
  query boundaries reviewable. An LSP adapter can reuse the inspection service
  as a future client.

Human-readable inspection supplements exact oracle conformance; it does not
replace or relax it.

<a id="val-a1"></a>
> **VAL-A1 — Evidence is attached to one architectural claim.** A positive
> Current Status statement names an observable artifact which establishes that
> particular behavior, while a negative or partial artifact may pin a known
> limitation. A passing aggregate suite is not evidence for an unstated local
> guarantee.
>
> **Required verification:** Documentation review checks every implemented
> capability against a focused test, snapshot, normalized trace, edit
> experiment, exact oracle result, inspector scenario, or source anchor whose
> asserted behavior matches the claim.

<a id="val-a2"></a>
> **VAL-A2 — Human inspection and exact conformance remain independent.**
> Readable Sage-only views explain semantic values and incremental work; the
> Oracle independently decides compatibility from Sage and rustc emissions.
> Neither facility consumes the other's output to repair or reinterpret a
> result.
>
> **Required verification:** Architecture and dependency tests keep inspector
> adapters out of oracle comparison and rustc-oracle values out of inspector
> rendering, while one pinned slice supplies both independently produced forms
> of evidence.

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

<a id="val-a3"></a>
> **VAL-A3 — A review packet connects result, dependency, edit, and code
> evidence.** For one semantic target, the packet identifies the Rust input,
> readable result, diagnostic/completeness outcome, cold and warm dependencies,
> relevant or unrelated edit behavior, focused or oracle verification, and
> anchored implementation entry points.
>
> **Required verification:** Each designated review packet contains or links
> all seven elements, and its reproduction commands regenerate the checked-in
> artifacts without relying on unrecorded setup.

## Current status

### Current frontier and evidence

- **[VAL-A1](#val-a1)/[VAL-A3](#val-a3):** The [module-expansion review
  packet](../pipeline/module-expansion.md#review-packet) and
  [oracle-checked body review
  packet](../examples/oracle-checked-method.md#review-packet) are the initial
  claim-specific review trails.
- **[VAL-A2](#val-a2):** The [Oracle Test Harness](../oracle-test-harness.md) documents and
  tests exact comparison independently from human-oriented examples and query
  traces.
- **[VAL-A1](#val-a1)/[VAL-A3](#val-a3):** The
  [Semantic Inspector](./semantic-inspector.md) now supplies source-driven API
  snapshots, readable real semantic products, structured cold/warm/edit
  traces, revision comparisons, and direct source anchors for
  `DbDropGuard::db`.

### Current limitations

- Review packets are established for selected expansion and body slices, not
  every positive capability claimed across the architecture guide.
- No automated documentation audit yet proves VAL-A1 or the seven-element
  completeness requirement in VAL-A3 across all chapters.

### Related roadmap slices

The implemented [Semantic Inspector and persistent edit-testing
slice](../../implementation/roadmap.md#implemented-slice-semantic-inspector-and-persistent-edit-testing)
establishes the common interactive and source-driven evidence surface.
