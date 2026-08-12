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
