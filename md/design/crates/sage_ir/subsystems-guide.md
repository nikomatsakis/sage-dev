# Semantic Subsystems

Semantic subsystems are used by several compilation phases and therefore do
not form a simple linear pipeline. Name resolution participates in expansion,
signature checking, and body checking. Type inference and trait solving
cooperate during body checking. External metadata supplies authoritative facts
about dependency definitions without deciding Sage's semantic questions.

Each subsystem chapter explains its public semantic boundary, permitted
dependencies, important algorithms, and the phases that invoke it. Claims
beside the modeled entities identify the load-bearing invariants. Their
generated anchor pages display implementation entries, tests, saved reviews,
and explicit TODO/N/A gaps; these sources supply coverage without a separate
subsystem status table.

- [Name Resolution](resolution-guide.md) maps scoped Rust syntax to symbols
  and preserves ambiguity or incompleteness.
- [Type Inference](inference-guide.md) owns body-local constraints,
  speculative versions, obligations, and finalization.
- [Trait Solver](solver-guide.md) proves canonical propositions and
  produces goal-specific outputs such as normalized types.
- [External Metadata](metadata-guide.md) imports narrow authoritative
  dependency facts without delegating Sage semantic decisions.
