# Design

This section is a reference and guide to Sage's architecture. Start with the
[tenets](./tenets.md), then use the [architecture overview](./architecture.md)
to see the whole compilation pipeline and the concepts shared across it.

The detailed chapters are organized in four groups:

- [Rust Compilation Pipeline](./pipeline/README.md) — the phases that transform
  Rust source into checked signatures and elaborated typed bodies. Phase
  chapters lead with their input, output, guarantees, granularity, and entry
  queries.
- [Semantic Subsystems](./subsystems/README.md) — services such as name
  resolution, type inference, trait solving, and external metadata that are
  used from more than one phase.
- [Representations and Infrastructure](./infrastructure/README.md) — the
  symbols, Typed IR, spans, Stash storage, and incremental identities that
  connect the phases.
- [Validation and Inspection](./validation/README.md) — oracle conformance,
  worked examples, snapshots, query traces, and interactive inspection.

Architecture chapters describe the destination in their main text. Their
**Current Status** sections state what is implemented now, the current
limitations, and inspectable evidence. The [Build-Out
Roadmap](../implementation/roadmap.md) instead organizes future work into
cross-cutting implementation slices.

## Reading design anchors

New specification paragraphs have meaningful names such as
`meta.calls.declared`, introduced by the custom Markdown role
<code>{anchor}&#96;meta.calls.declared&#96;</code>. Each paragraph states one testable rule.
Rust tests link to those names with `#[sage::test(meta.calls.declared)]`, and
`sage-meta check` requires a test for every named specification paragraph.
The [architectural pseudocode chapter](./validation/architecture-language.md)
is the first converted specification. Existing numbered anchors such as
`EXP-A1` retain their destination rule and **Required verification** until
their chapters are converted.

An anchor does not by itself claim that Sage already satisfies the rule. The
chapter's **Current Status** section links the evidence which exists today and
names the remaining gap. Cross-cutting choices and rationale use `D<n>` entries
in [Architecture Decisions](./decisions.md); chapter anchors express the local,
auditable consequences of those choices.

When an RFD completes, its destination anchors and verification requirements
are promoted into these chapters. The architecture guide is therefore the
living reference; the RFD remains the historical design and implementation
record.

This organization was shaped in part by Kari Wilhelm's ideas for **Anchor
Driven Development**: identify the small, load-bearing claims from which the
rest of a design follows, give them stable names, and connect them to
inspectable evidence. Those ideas helped guide the restructuring of this book.
Many thanks to Kari for articulating the approach.

The [architectural pseudocode](./validation/architecture-language.md) provides
a complementary method-oriented review surface in `sage-src/`, parsed and
checked by `sage-meta`.
