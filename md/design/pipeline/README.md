# Rust Compilation Pipeline

These chapters explain the phases that turn Rust source into semantic output.
Each phase is demand-driven and memoized at a stated granularity rather than a
mandatory whole-crate batch pass.

A phase chapter begins with its contract: input, output, guarantees, and
entry queries. It then explains construction, failure and terminal
incompleteness, incremental dependencies, and a worked example grounded in
source anchors. A final **Current Status** section records limitations and
evidence without weakening the destination contract in the main text.

The [Architecture](../architecture.md) page gives the maximally zoomed-out
pipeline. The existing [Type Checking](../checking.md) chapter covers
signature and body checking while the phase-specific chapters are built out.

- [Module and Macro Expansion](./module-expansion.md) documents the
  module-level represented-symbol result, recursive output expansion, the
  narrower same-module fixed point, and terminal incompleteness.
- [Parsing and Stable Symbol Creation](./parsing.md) explains how source and
  generated text become stable local identities with self-contained CST.
- [Signature Checking](./signature-checking.md) defines the item interface
  boundary that other items may depend upon.
- [Body Checking and Typed-IR Elaboration](./body-checking.md) defines the
  completed per-body semantic output and its dependency rules.
