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
| **Ground query** | A canonical solver query with no flexible existential inputs. Rigid generic placeholders count as ground. |
| **Conditional answer** | A solver `Yes` whose substitution and residual form a sufficient condition for the original goal. |
| **Progress envelope** | A necessary condition guaranteed to subsume every final answer still possible from the represented work. |
| **Hard hint** | A substitution fact necessary across every still-possible alternative and therefore safe to apply transactionally while retaining the obligation. |
| **Subsumption** | The directional relation `A` subsumes `B` when `Cond(B) => Cond(A)`; `A` is at least as general as `B`. |
| **Ambiguity** | A sound non-ground result indicating that the solver cannot select a unique definite answer from current information. |
| **Solver overflow** | Explicit resource exhaustion, such as proof depth, term size, work, or fixpoint iterations; never evidence for logical `No`. |
| **Inductive cycle** | A recursive proof path which cannot establish itself merely by recurring. |
| **Coinductive cycle** | A recursive proof path for a designated coinductive goal which may use a provisional cyclic assumption. |
| **Visible impl universe** | Every impl represented in the current compilation: local impls plus impl metadata from reachable dependencies, excluding absent downstream crates. |
| **Simplified self type** | A conservative candidate-index key for the rigid outer shape of a trait goal's self type; unknown and blanket impls use a fallback path. |
