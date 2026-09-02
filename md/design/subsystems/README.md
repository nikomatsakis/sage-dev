# Semantic Subsystems

Semantic subsystems are used by several compilation phases and therefore do
not form a simple linear pipeline. Name resolution participates in expansion,
signature checking, and body checking. Type inference and trait solving
cooperate during body checking. External metadata supplies authoritative facts
about dependency definitions without deciding Sage's semantic questions.

Each subsystem chapter explains its public semantic boundary, permitted
dependencies, important algorithms, and the phases that invoke it. Its final
**Current Status** section records implemented behavior, limitations, and
inspectable evidence. Stable chapter-local design anchors identify the
load-bearing invariants: each anchor states its destination rule and the
verification required to establish it, while Current Status records only the
evidence which exists today.

- [Name Resolution](./name-resolution.md) maps scoped Rust syntax to symbols
  and preserves ambiguity or incompleteness.
- [Type Inference](./type-inference.md) owns body-local constraints,
  speculative versions, obligations, and finalization.
- [Trait Solver](../trait-solver.md) proves canonical propositions and
  produces goal-specific outputs such as normalized types.
- [External Metadata](./external-metadata.md) imports narrow authoritative
  dependency facts without delegating Sage semantic decisions.
