# Terminology

Key terms used throughout sage's design and code.

| Term | Meaning |
|---|---|
| **Stash** | A per-database arena with hash-consing that interns types and other IR nodes. Returns `Ptr<T>` handles. |
| **Ptr** | A pointer-sized handle into the stash. Supports zero-copy decomposition of compound types. |
| **Symbol** | The uniform IR unit for any named entity (function, struct, field, variant, generic parameter, etc.). |
| **Oracle** | The test harness that compares sage's output against rustc's for the same input program. |
| **CST** | Concrete syntax tree — the tree-sitter parse tree before lowering to sage's IR. |
| **Span** | A source location, stored as a relative offset from the owning symbol's anchor span. |
| **TyData** | The interned payload behind a `Ptr<Ty>` — an enum of type kinds (scalar, reference, tuple, ADT, etc.). |
| **BodyCheck** | The per-function type-checking context; runs async and independently per function body. |
| **Salsa** | The incremental computation framework; all major queries are salsa tracked functions. |
| **RFD** | Request for Discussion — a design document proposing a change (see [RFDs](./rfds/README.md)). |
