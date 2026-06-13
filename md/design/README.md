# Design

This section documents sage's architecture and key design decisions.
Start with the [overview](./overview.md) for the conceptual model
and shared vocabulary; the other pages drill into specific areas.

- [Overview](./overview.md) — vocabulary (`Symbol`, `ModSymbol`,
  `ItemAst`, `ModAst`, ext leaves), the wrapper-of-enum pattern,
  and how the layered phases stack from input to resolved body.
- [Architecture](./arch.md) — the pipeline from source files to
  analysis results, crate layout, the `TcxDb` interface to rustc,
  testing strategy.
- [IR](./ir.md) — field-level reference: every item kind, body
  representation, MEM-map data model, span tracking, display.
- [Checking](./checking.md) — design tenets for the CST checking
  layer: code organization, query design, two-stash data flow,
  resolution model.
