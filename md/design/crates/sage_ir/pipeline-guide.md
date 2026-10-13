# Rust Compilation Pipeline

These chapters explain the phases that turn Rust source into semantic output.
Each phase is demand-driven and memoized at a stated granularity rather than a
mandatory whole-crate batch pass.

A phase chapter begins with its contract: input, output, guarantees, and
entry queries. It then explains construction, failure and terminal
incompleteness, incremental dependencies, and a worked example grounded in
source anchors. Generated anchor pages connect the destination contract to
implementation entries, tests, and explicit TODO/N/A gaps.

Load-bearing rules appear next to that explanation as stable design anchors.
Each anchor states a destination invariant and the verification required to
establish it. Its code/test associations and saved review expose the evidence
and remaining gaps without maintaining a separate chapter status table.

The [Architecture](architecture-guide.md) page gives the maximally zoomed-out
pipeline. The [Shared Checking Design](checking-guide.md) chapter records
mechanisms common to signature and body checking.

- [Module and Macro Expansion](expansion-guide.md) documents the
  module-level represented-symbol result, recursive output expansion, the
  narrower same-module fixed point, and terminal incompleteness.
- [Parsing and Stable Symbol Creation](parsing-guide.md) explains how source and
  generated text become stable local identities with self-contained CST.
- [Signature Checking](signatures-guide.md) defines the item interface
  boundary that other items may depend upon.
- [Body Checking and Typed-IR Elaboration](bodies-guide.md) defines the
  completed per-body semantic output and its dependency rules.
