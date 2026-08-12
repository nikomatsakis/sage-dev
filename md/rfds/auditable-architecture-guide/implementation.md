# Implementation plan and status

This RFD is accepted. The ownership split between architecture Current Status
sections, cross-cutting roadmap slices, and RFD implementation plans governs
the implementation below.

Each step is independently reviewable. Page moves should be minimized unless
the navigation benefit outweighs broken historical links.

## Step 1: Establish the documentation contract and navigation

- [x] Update `md/design/README.md` with the pipeline, subsystem,
  representation/infrastructure, and validation taxonomy.
- [x] Reshape `md/SUMMARY.md` to make those groups visible while preserving
  useful existing chapters.
- [x] Update `md/contributing/maintaining-the-docs.md` with architecture
  Current Status ownership, cross-cutting roadmap-slice ownership, and RFD
  checkpoint ownership.
- [x] Define stable terminology for phase, subsystem, representation,
  capability, evidence, and terminal incompleteness.
- [x] Build the book and verify that existing incoming links remain valid.

## Step 2: Reshape the maximally zoomed-out architecture page

- [x] Lead with the Rust compilation pipeline and its phase granularities.
- [x] Introduce symbols as the stable semantic identities flowing through the
  pipeline before using symbol-keyed queries in the phase table.
- [x] Add a compact input/output/entry-point table.
- [x] Map semantic subsystems and cross-cutting representations to the phases
  that use them.
- [x] Move or condense crate-layout material so it follows the semantic model.
- [x] Move current implementation prose into clearly marked Current Status
  sections, preserving destination design in the main text.
- [x] Link relevant cross-cutting roadmap slices from each status section.

## Step 3: Add the symbols and semantic identity chapter

- [x] Define symbols as stable definition identities rather than complete
  checked item data.
- [x] Explain local, external, intrinsic, erased, and kind-specific symbol
  representations.
- [x] Explain ownership, scope, associated-item identity, and the relationship
  between names, paths, resolution, and symbols.
- [x] Show how symbol-keyed lazy queries expose signatures, members, fields,
  and bodies.
- [x] Show how types and completed Typed IR refer to resolved symbols.
- [x] State the incremental identity guarantees and the edits that should not
  mint replacement identities.
- [x] Add focused ezanchor excerpts for the erased wrapper, a kind-specific
  wrapper, local tracked identity, and external structural identity.
- [x] Link symbol identity and invalidation tests as review evidence.

## Step 4: Reshape the build-out roadmap around implementation slices

- [x] Replace broad RFD-completion groups with coherent cross-cutting slices.
- [x] Give every slice an observable goal and acceptance target.
- [x] Record why the slice is ordered where it is.
- [x] Record scope, non-goals, affected architecture, and dependencies.
- [x] Give each slice a high-level ordered implementation plan and link the
  RFDs that own its detailed checkpoints without duplicating them.
- [x] Record concise slice-level progress.
- [x] Link completed acceptance evidence from affected architecture Current
  Status sections.
- [x] Preserve and align the Mini-redis roadmap as an application-specific
  vertical-slice view.

## Step 5: Pilot the module and macro expansion phase chapter

- [x] Document the module-level input, successful output, and downstream
  guarantees.
- [x] Explain recursive output expansion and the narrower role of Salsa
  fixed-point iteration in same-module macro resolution cycles.
- [x] Explain that terminal incompleteness arises from errors, unsupported
  constructs, unavailable expansion, or limits rather than pending work.
- [x] Distinguish direct expanded item symbols from all names visible through
  imports and resolution.
- [x] Document the current represented-symbol output and consumer-specific
  completeness audits as a current discrepancy from a unified phase result.
- [x] Add or verify anchors around the entry query, construction mechanism,
  and completeness boundary.
- [x] Add focused expansion, failure, and incremental evidence to the
  phase chapter's Current Status section.
- [x] Link roadmap slices that would remove the documented limitations.
- [x] Reconcile the destination account with the completed tracked-expansion
  RFD without rewriting that RFD's historical proposal.

## Step 6: Document the remaining Rust compilation phases

- [x] Add or reshape the parsing and stable-symbol chapter.
- [x] Refocus signature checking as an item-granularity phase.
- [x] Refocus body checking and elaboration as a body-granularity phase.
- [x] Give each phase its contract, entry points, construction, terminal
  failures, incremental boundary, example, anchors, and Current Status
  section.
- [x] Preserve detailed Typed IR, Stash, Spans, and Trait Solver contracts by
  linking rather than duplicating them.

## Step 7: Complete subsystem and infrastructure entry guides

- [ ] Add a destination-oriented name-resolution subsystem chapter.
- [ ] Separate the type-inference subsystem account from the body-checking
  phase that uses it.
- [ ] Ensure the trait-solver chapter is linked from every phase that submits
  goals without presenting it as a linear compilation phase.
- [ ] Document the external-metadata boundary and what rustc may authoritatively
  supply without solving Sage semantic questions.
- [ ] Add an incrementality entry guide covering Salsa query granularity,
  stable identity, backdating, and evidence expectations.

## Step 8: Connect review workflows and the Semantic Inspector

- [ ] Define a compact review packet for each worked example: Rust input,
  readable output, diagnostics/completeness, query trace, and relevant edit
  behavior.
- [ ] Link existing tests and snapshots before the Semantic Inspector exists.
- [ ] When the Semantic Inspector lands, add reproducible commands and
  checked-in readable output to the relevant architecture Current Status
  sections.
- [ ] Keep human-readable inspector evidence distinct from exact Oracle
  conformance evidence.
- [ ] Add a contributor checklist for keeping evidence links current when
  tests, snapshots, anchors, or query boundaries change.

## Completion

- [ ] Every implemented phase and major subsystem has a destination-oriented
  entry guide or a deliberate linked deferral.
- [ ] Each phase and subsystem chapter has a Current Status section containing
  concrete limitations and inspectable evidence for implemented behavior.
- [ ] The build-out roadmap describes cross-cutting slices, acceptance targets,
  ordering, dependencies, and implementation-plan links without duplicating
  phase-local status or RFD checkpoints.
- [ ] Existing focused representation and validation chapters remain
  accessible and authoritative.
- [ ] The symbols chapter provides the semantic vocabulary used by every phase
  and subsystem chapter.
- [ ] Every architecture chapter links to the roadmap slices relevant to its
  current limitations.
- [ ] The module-expansion pilot and at least one body-checking example have
  complete review packets.
- [ ] `mdbook build` and repository-required documentation validation pass.
- [ ] The accepted documentation structure and maintenance rules are reflected
  in the Architecture & Design index and contributor contract.
