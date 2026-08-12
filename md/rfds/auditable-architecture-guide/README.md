# RFD: Phase-Oriented, Auditable Architecture Guide

**Status:** Accepted

**Depends on:**

- [Maintaining this book](../../contributing/maintaining-the-docs.md) — the
  existing ownership rules for architecture pages, RFDs, and the roadmap
- [Architecture](../../design/architecture.md) — the current structural
  overview that this RFD will reshape

**Related:**

- [Semantic Inspector and Incremental Query Testing](../semantic-inspector/README.md)
  — the planned interactive source of readable semantic output and query
  traces
- [Oracle Test Harness](../../design/oracle-test-harness.md) — exact
  cross-implementation conformance evidence
- [Mini-redis Conformance Roadmap](../../implementation/mini-redis.md) —
  application-scale vertical slices and acceptance evidence

## TL;DR

- Reorganize the architecture guide around Sage's Rust compilation pipeline,
  semantic subsystems, representations, infrastructure, and validation tools.
- Explain symbols as Sage's core semantic identities: the values that connect
  parsing, expansion, resolution, signatures, types, bodies, and external
  metadata.
- Describe every compilation phase from its contract outward: granularity,
  input, output, guarantees, entry points, construction, terminal failure
  modes, and incremental dependencies.
- Retain focused cross-cutting chapters such as Typed IR, Stash, Spans, Trait
  Solver, and Oracle Test Harness.
- Keep destination design in the main body of each architecture chapter, then
  give that chapter a Current Status section with current limitations and
  inspectable evidence for what works.
- Focus the build-out roadmap on cross-cutting implementation slices, their
  acceptance targets, ordering, dependencies, and implementation plans.
- Map important design claims to inspectable tests, snapshots, query traces,
  edit-invalidation experiments, or exact oracle results.
- Ground phase and subsystem chapters in small ezanchor excerpts so a reviewer
  can enter the implementation at its load-bearing boundaries.
- Use module and macro expansion as the first phase-chapter pilot.

## Motivation

The existing architecture book contains useful material, but it does not
provide a straightforward account of how Sage processes Rust. The main
architecture page begins with crate layout and a module map, then accumulates
implementation details from several subsystems. A reader must reconstruct the
compilation pipeline and its contracts from those details.

For each phase, a reviewer first needs to know:

1. At what granularity does the phase run?
2. What does it consume?
3. What does it produce?
4. What may downstream consumers rely upon after it finishes?
5. How is the result constructed?

Load-bearing mechanisms must be visible at that level. For example, module
expansion uses fixed-point iteration when resolving a macro requires the
expanded symbols of the same module. That fact should not require a close read
of Salsa annotations and resolver calls.

The book also needs a clearer separation between destination and current
implementation. Architecture pages intentionally describe where Sage is
going. The current build-out roadmap instead groups work primarily by RFD and
uses broad states such as “Macro expansion: Done.” Completing one RFD does not
mean Sage has reached the destination for all Rust macro expansion. This makes
status hard to interpret and invites duplication when architecture chapters
also try to record current limitations.

Finally, architectural review needs inspectable evidence. Source anchors show
where a mechanism lives, but they do not establish its observable behavior or
incremental boundary. A reviewer should be able to inspect a small source
example, the resulting symbols or typed IR, the queries performed, and a
focused test without reading every implementation detail.

## Change in a nutshell

The architecture guide will offer two complementary paths through the system:

```text
Rust compilation pipeline
  parse and create symbols
  expand modules and macros
  check signatures
  check and elaborate bodies

Semantic and cross-cutting reference
  name resolution
  type inference
  trait solving
  external metadata
  Typed IR
  Stash and spans
  incrementality
  oracle and inspection facilities
```

Pipeline chapters describe transformations. Subsystem and representation
chapters describe facilities or data models used by more than one phase.

The main body of an architecture chapter defines its destination contract. A
clearly marked Current Status section then records the distance from that
destination:

| Capability | Status | Current limitation | Evidence |
|---|---|---|---|
| Fixed-point module expansion | Implemented | — | focused cycle test and query trace |
| Terminal incompleteness | Partial | completeness is consumer-specific | diagnostic and completeness tests |
| General active attributes | Not implemented | semantics are not represented | — |

The build-out roadmap has a different axis. A slice such as “type-check
`Parse::next`” can require coordinated work in body checking, method
resolution, trait solving, external metadata, Typed IR, and the oracle. The
roadmap records that cross-cutting goal and its ordered implementation plan;
each affected architecture chapter records what that slice changed in its own
Current Status section.

RFD implementation files continue to record checkpoint progress. RFD status
does not stand in for capability status.

## Detailed plans

### Information architecture

The Architecture & Design section will be organized into the following
conceptual groups. Exact page names may change while avoiding unnecessary file
moves.

1. **Overview and tenets**
   - Sage-wide design tenets
   - the maximally zoomed-out pipeline and subsystem map
   - the granularity of each phase and public query boundary

2. **Rust compilation pipeline**
   - parsing and stable symbol creation
   - module and macro expansion
   - signature checking
   - body checking and typed-IR elaboration

3. **Semantic subsystems**
   - name resolution
   - type inference
   - trait solving
   - external metadata

4. **Representations and infrastructure**
   - symbols and semantic identity
   - Typed IR
   - Stash
   - spans and provenance
   - Salsa and incrementality

5. **Validation and inspection**
   - Oracle Test Harness
   - Semantic Inspector once implemented
   - focused examples

Existing focused chapters remain authoritative for their concerns. Pipeline
chapters link to them instead of repeating their specifications. For example,
the body-checking chapter defines when a `CheckedBody` is produced and links
to Typed IR for the representation contract, Trait Solver for obligation
semantics, Stash for storage, and Oracle Test Harness for conformance.

### The maximally zoomed-out overview

The top-level architecture page will stop serving as an append-only inventory
of implementation details. It will contain:

- Sage-wide design tenets by reference;
- a pipeline diagram;
- a phase table with granularity, input, output, and primary entry query;
- an introduction to symbols as the semantic values that flow through and
  identify work across that pipeline;
- a map of semantic subsystems used across phases;
- a map of representations and validation facilities; and
- links to the detailed chapters and their roadmap sections.

Crate layout and a concise code map remain useful, but follow the semantic
overview rather than defining it.

### Symbols as the semantic spine

Symbols receive a dedicated representation chapter under Representations and
Infrastructure, but the top-level overview introduces them before presenting
the detailed pipeline. Without that concept, phase inputs such as
`LocalModSym` and entry queries such as `LocalFnSym::sig` appear to be
implementation accidents rather than the organizing model.

The symbols chapter explains:

- a symbol is a stable semantic identity for a definition, not the
  definition's complete checked data;
- local symbols inherit stable Salsa identity from source or generated items,
  while external symbols identify definitions through external metadata;
- `Symbol` is the erased union used for heterogeneous module membership, while
  kind-specific wrappers such as `FnSymbol`, `TraitSymbol`, and `ModSymbol`
  retain the distinctions required by semantic operations;
- ownership and scope connect items, associated items, modules, and crates;
- names and paths are syntax used by resolution to discover symbols; they are
  not themselves definition identity;
- signatures, fields, associated items, and bodies are lazy queries keyed by
  symbols rather than data stored eagerly in one global tree;
- types and completed Typed IR refer back to symbols for resolved definitions;
  and
- stable symbol identity is an incremental boundary: changing detail behind a
  symbol need not change the identity consumed by unrelated queries.

The chapter also distinguishes three related questions:

1. **What is a symbol?** The cross-cutting semantic representation.
2. **How are local symbols created?** Part of parsing, generated-item parsing,
   and module expansion.
3. **How are symbols discovered and used?** The responsibility of resolution
   and downstream signature/body queries.

This separation prevents the parsing chapter from becoming the only
explanation of symbols and prevents the symbols chapter from pretending that
symbol construction is an independent linear compilation phase.

### Phase-chapter contract

Every compilation-phase chapter follows the common structure proposed in
[Phase chapter shape](./phase-chapter-shape.md):

1. role in the pipeline and granularity;
2. input;
3. output and guarantees;
4. entry queries and functions;
5. construction algorithm;
6. failure, unsupported input, and resource limits;
7. incremental dependencies;
8. a worked example;
9. code map and anchored excerpts; and
10. a Current Status section containing current limitations, evidence, and
    links to relevant roadmap slices.

The main body states the destination. Current implementation facts are
confined to the Current Status section so the destination remains readable
without hiding the distance from it.

### Terminal incompleteness

Phase contracts distinguish a terminal incomplete result from unfinished
computation. Continuing the same query is not expected to make such a result
complete.

For module expansion, incompleteness may arise from:

- invalid or ambiguous user input;
- a Rust construct Sage does not yet represent;
- a resource limit such as maximum expansion depth; or
- unavailable external expansion information.

The fixed-point computation may converge while the result is still
incomplete. Conversely, incompleteness is not a request to schedule more work.
When a partial representation is retained for recovery, the chapter identifies
which guarantees still hold and how consumers conservatively refine it for a
particular question.

This RFD documents that distinction. It does not by itself change an API such
as `local_expanded_module_items` to return structured incompleteness. If the
documentation audit shows that an API should change, that semantic change
requires its own design decision or RFD.

### Phase and subsystem entry points

Each chapter identifies the smallest public or semipublic boundary from which
the rest of the implementation can be followed. Entry points are generally
Salsa queries or methods on symbols and CST nodes.

Code excerpts use ezanchor and are deliberately small:

- the public phase/query boundary;
- the key construction mechanism;
- a load-bearing failure or completeness boundary; and
- an important incremental lookup boundary when applicable.

Anchors are not used to reproduce whole modules. The surrounding code remains
the source of truth, and the generated source link is the path from the guide
into implementation detail.

### Current status and evidence

Each architecture chapter ends with a Current Status section. It contains:

1. **Current frontier.** The broadest coherent portion of the destination that
   works today.
2. **Implemented capabilities and evidence.** Claim-specific review trails for
   behavior that works.
3. **Current limitations.** Concrete differences from the destination and
   their observable consequence.
4. **Related roadmap slices.** Cross-cutting milestones that will change this
   status.

Useful evidence includes:

- focused contract tests;
- readable symbol, signature, diagnostic, or typed-IR snapshots;
- structured cold and warm query traces;
- relevant-edit and unrelated-edit invalidation tests;
- negative dependency assertions such as “does not read callee bodies”;
- exact Sage/rustc oracle comparisons; and
- Semantic Inspector commands and checked-in output once that facility exists.

Evidence entries are small review trails. Each entry names the claim it
supports, the observable artifact, how to reproduce or inspect it, and the
implementation entry point when deeper reading is useful. A broad link to a
test directory or a statement that the full suite passes is not sufficient
evidence for a specific architectural claim.

For example:

| ID | Claim | Inspectable evidence | Code entry |
|---|---|---|---|
| E1 | Successful expansion returns every direct represented symbol | source fixture and symbol snapshot | expanded-module query anchor |
| E2 | Same-module macro resolution reaches a fixed point | focused cycle test and query trace | cycle-initial query anchor |
| E3 | An unrelated body edit does not reexecute module expansion | edit-invalidation trace | expansion query boundary |
| E4 | An unresolved macro is terminally incomplete | diagnostic and completeness test | completeness boundary anchor |

The Current Status section may use concise states such as **Implemented**,
**Partial**, **Not implemented**, and **Deliberately deferred**, but every
positive state must be paired with focused evidence for the defining
guarantees. Status is about observable capability, not test coverage
percentage.

The Semantic Inspector is the preferred eventual presentation layer for
human-readable semantic output and query traces. This RFD does not depend on
its implementation: existing focused tests and snapshots are linked in the
meantime. Inspector output remains distinct from the Oracle Test Harness,
whose conformance decision is exact textual identity after thin independent
adapters.

### Roadmap ownership

The build-out roadmap is organized around cross-cutting implementation slices,
not around architecture chapters. A slice is a reviewable semantic outcome
that commonly crosses several phases and subsystems. Examples include a pinned
mini-redis function body, complete method lookup for one receiver family, or a
structured module-expansion result.

Each roadmap slice contains:

1. **Goal and acceptance target.** The observable result that defines the
   slice as complete.
2. **Why this slice and why now.** Its value and position in the implementation
   sequence.
3. **Scope and non-goals.** The boundary that keeps the slice reviewable.
4. **Affected architecture.** Links to the phase, subsystem, representation,
   and validation chapters involved.
5. **Dependencies.** Earlier slices or design decisions required before work
   begins.
6. **Implementation plan.** The high-level ordered stages across the affected
   areas, with links to RFDs for detailed checkpoint plans.
7. **Progress.** A concise slice-level state, with completed acceptance
   evidence linked from the affected chapters' Current Status sections.

A roadmap section therefore has this shape:

```text
Slice: type-check mini-redis Parse::next
  Goal and acceptance target
  Why this slice
  Scope and non-goals
  Affected architecture
  Dependencies
  High-level implementation plan
  Progress
```

The roadmap may summarize ordering across RFDs, but it does not duplicate the
checkpoint checklist in an RFD's `implementation.md`. Conversely, an
architecture chapter does not reproduce the cross-cutting slice plan; it
records only the current state and evidence for its own design area.

The completed and accepted RFD lists continue to describe the lifecycle of
individual proposals. A completed RFD may contribute to a partially
implemented destination capability. The roadmap must not label the broader
capability done solely because its contributing RFD completed.

The Mini-redis Conformance Roadmap is already close to this model: it is an
application-scale sequence of vertical slices. The general build-out roadmap
uses the same slice-oriented approach for work that is not specific to
mini-redis.

### First pilot: module and macro expansion

The first new phase chapter will cover module and macro expansion. It is a
useful pilot because it exercises all parts of the proposed format:

- **granularity:** one local module;
- **input:** stable unexpanded item symbols and the resolution environment;
- **successful output:** the complete ordered list of direct expanded item
  symbols, including represented generated items;
- **construction:** recursive expansion plus Salsa fixed-point iteration when
  macro resolution depends on the same module's expanded symbols;
- **terminal incompleteness:** user errors, unsupported constructs, unavailable
  expansion, and resource limits;
- **incrementality:** a tracked module-level boundary with stable generated
  provenance; and
- **current discrepancy:** the existing query returns represented symbols
  while consumer-specific audits separately establish whether omitted source
  could affect a trait or method-provider question.

The chapter distinguishes direct module members from all names visible in a
module. Imports are item symbols at expansion time; resolving their targets
and effects belongs to name resolution.

The pilot will also reconcile the architecture guide with the completed Macro
Expansion as a Tracked Query RFD. The RFD remains a historical change record;
the new phase chapter becomes the destination account of the current and
planned mechanism.

### Documentation maintenance

The documentation update contract will be extended so that:

- a new or changed phase guarantee updates its architecture chapter;
- a capability changing status, limitations, or evidence updates that
  chapter's Current Status section;
- a slice being added, reordered, blocked, or completed updates the roadmap;
- an RFD checkpoint updates only that RFD's `implementation.md` unless it also
  changes a chapter's current status or a roadmap slice's progress;
- code implementing an anchored mechanism updates or preserves the matching
  anchor; and
- evidence links are checked when tests or snapshots are renamed.

All new pages are registered in `SUMMARY.md`. `mdbook build` remains the
required structural and link validation.

## Frequently asked questions

### Does this replace the existing Typed IR, Stash, Spans, or Oracle chapters?

No. Those pages describe cross-cutting representations, infrastructure, or
validation contracts. Pipeline chapters explain where they participate and
link to their focused specifications.

### Why are symbols a representation chapter rather than only part of parsing?

Parsing creates local item symbols, but symbol identity outlives parsing and
organizes every later phase. Expansion returns symbols, resolution maps names
to symbols, signature and body queries are keyed by symbols, types refer to
symbols, and external metadata enters through external symbols. The parsing
chapter explains construction; the symbols chapter explains the shared
semantic model.

### Is name resolution a compilation phase?

Not in the linear sense used by this RFD. Expansion, signature checking, and
body checking all use resolution. It is therefore documented as a semantic
subsystem, with each phase chapter explaining the resolution operations and
completeness it requires.

### Why does current status live in the architecture chapter?

The reader needs to see the destination, the present limitation, and the
evidence for implemented behavior together. Keeping Current Status as a
clearly marked final section preserves the destination-oriented main text
while making the chapter useful for progressive review.

### What belongs in the roadmap?

Cross-cutting implementation slices: their acceptance targets, scope,
ordering, dependencies, affected architecture, implementation RFDs, and
progress. The roadmap answers “what coherent outcome are we building next?”
rather than repeating the local status of every architecture chapter.

### Does a completed RFD imply an implemented destination capability?

No. It means the scoped change described by that RFD landed. The roadmap
independently records the progress of the slices to which it contributes, and
each affected architecture chapter records its resulting current status.

### Must every implementation detail receive an anchor and a test?

No. The guide concentrates on non-obvious, load-bearing design points from
which ordinary implementation details can be understood. Evidence establishes
the defining guarantees, not every branch of the code.

### Does this RFD change compiler behavior?

No. It changes how architecture, status, and evidence are organized and
reviewed. It may expose mismatches that motivate later compiler RFDs, but those
changes are outside this RFD.

## Implementation

See [Implementation plan and status](./implementation.md).
