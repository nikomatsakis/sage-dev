# Representations and Infrastructure

These chapters define the semantic values and storage mechanisms shared across
the compilation pipeline. They are not sequential phases.

- Symbols provide stable identities for local and external definitions.
- [Typed IR](../typed-ir.md) is the output contract for completed body checking.
- [Stash](../stash.md) owns compact hash-consed trees at query boundaries.
- [Spans](../spans.md) retain source provenance without making unrelated text
  movement invalidate semantic content.
- Salsa memoizes symbol-keyed computations and propagates changes through the
  dependencies actually observed.

The dedicated symbols and incrementality chapters are added by the accepted
architecture-guide RFD.
