# Semantic Subsystems

Semantic subsystems are used by several compilation phases and therefore do
not form a simple linear pipeline. Name resolution participates in expansion,
signature checking, and body checking. Type inference and trait solving
cooperate during body checking. External metadata supplies authoritative facts
about dependency definitions without deciding Sage's semantic questions.

Each subsystem chapter explains its public semantic boundary, permitted
dependencies, important algorithms, and the phases that invoke it. Its final
**Current Status** section records implemented behavior, limitations, and
inspectable evidence.

The [Trait Solver](../trait-solver.md) is the first focused subsystem chapter.
