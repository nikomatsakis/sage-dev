# Representations and Infrastructure

These chapters define the semantic values and storage mechanisms shared across
the compilation pipeline. They are not sequential phases.

- [Symbols](symbols-guide.md) provide stable identities for local and external
  definitions.
- [Typed IR](typed-ir-guide.md) is the output contract for completed body checking.
- [Stash](../sage_stash/storage-guide.md) owns compact hash-consed trees at query boundaries.
- [Spans](spans-guide.md) retain source provenance without making unrelated text
  movement invalidate semantic content.
- [Incrementality and Query Boundaries](incrementality-guide.md) explains how
  Salsa identities, query keys, backdating, and execution traces isolate
  changes.

Together these pages define the reusable data and computation boundaries used
by every pipeline phase.
