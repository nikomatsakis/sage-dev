# Terminology

Key terms used throughout sage's design and code.

| Term | Meaning |
|---|---|
| **Stash** | An owning arena with hash-consing for CST, types, and other tree-shaped IR. A query result may own a Stash; it is not one database-global arena. |
| **Ptr** | A typed handle into one owning Stash. It is four bytes in release builds and carries an additional debug-only stash identity. |
| **Symbol** | The shared identity family for represented top-level and associated definitions. Generic parameters, fields, and locals use separate scoped identity representations. |
| **Oracle** | The test harness that compares sage's output against rustc's for the same input program. |
| **CST** | Concrete syntax tree — the tree-sitter parse tree before lowering to sage's IR. |
| **Span** | A source location composed from an absolute parse-source root and relative placements through nested owners. |
| **TyData** | The interned payload behind a `Ptr<Ty>` — an enum of type kinds (scalar, reference, tuple, ADT, etc.). |
| **BodyCheck** | The per-function type-checking context; runs async and independently per function body. |
| **Salsa** | The incremental computation framework; all major queries are salsa tracked functions. |
| **RFD** | Request for Discussion — a design document proposing a change (see [RFDs](./rfds/README.md)). |
| **Compilation phase** | A demand-driven transformation with a stated semantic input, output, granularity, and downstream guarantees. |
| **Semantic subsystem** | A service, such as name resolution or trait solving, used by more than one compilation phase rather than one step in a linear pipeline. |
| **Representation** | A shared semantic data model, such as symbols or Typed IR, that is produced or consumed across phase boundaries. |
| **Capability** | One reviewable portion of an architecture chapter's destination contract whose current implementation state can be evidenced independently. |
| **Evidence** | An inspectable artifact tied to a design claim: a focused test, snapshot, query trace, edit experiment, oracle result, inspector command, or code anchor. |
| **Terminal incompleteness** | A completed computation whose result cannot provide the full phase guarantee because of invalid input, unsupported Sage functionality, a resource limit, or unavailable external information; it is not pending work that more polling will finish. |
| **Roadmap slice** | A cross-cutting, reviewable implementation outcome with an acceptance target, scope, dependencies, and ordered plan that may touch several phases and subsystems. |
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
