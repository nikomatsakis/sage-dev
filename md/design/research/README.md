# Research

Notes from investigating how rustc and other systems handle specific
subsystems.

- [Trait Solver Search](trait-solver-search.md) — miniKanren interleaving,
  SLG and Chalk tables, rustc's search graph, answer subsumption, and
  resource-bounded recursion.
- [rustc Binary Operator Typing and
  Lowering](rustc-binary-operator-typing.md) — trait-first operator checking,
  ordinary `core` impl proof plus rustc's scalar inference shortcuts, numeric
  fallback, writeback reclassification, and the THIR/MIR split between builtin
  operations and trait calls.
- [rustc Opaque Types and the New Trait Solver](rustc-opaque-types.md) —
  opaque identity, hidden-type inference, reveal modes, alias-bound candidates,
  RPITIT lowering, async returns, and implications for Sage.
