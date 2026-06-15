# Design

This section documents sage's architecture and key design decisions.
Start with the [tenets](./tenets.md) for the principles that guide
all new code; then read [architecture](./architecture.md) for the
structural overview.

- [Tenets](./tenets.md) — design principles: code organization,
  query design, two-stash data flow, resolution model, incrementality.
- [Architecture](./architecture.md) — crate layout, module map, data
  flow diagram, salsa layer, symbol system, testing strategy.
- [Examples](./examples.md) — three progressive walk-throughs: struct
  signature, function body with field access, macro-generated struct.
- [Checking](./checking.md) — detailed design of the CST checking
  layer: contexts, type lowering, body inference.
- [Stash](./stash.md) — `sage-stash` arena: `Stash`, `Ptr<T>`,
  `Slice<T>`, `Stashed<T>`, alloc vs intern, salsa integration.
- [Spans](./spans.md) — two-level span model: `AbsoluteSpan` for
  items, `RelativeSpan` within items, incremental reuse.
- [Subsetting](./subsetting.md) — language subsetting approach.
