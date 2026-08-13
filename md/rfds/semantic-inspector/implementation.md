# Implementation plan and status

This RFD is a draft. Its [destination design](./README.md#destination-design)
defines the completed Semantic Inspector without regard to implementation
order. This page first inventories all work required by that design, then
groups the work into reviewer-visible vertical slices.

The [Web Application Walkthrough](./web-application.md) refines the browser,
JSON, transport, and host work below as one end-to-end path. It does not add
independent slices; its work lands through the five parent slices on this page.

The inventory is the scope contract. A slice may be reordered or split without
changing that contract; omitted inventory work requires an RFD design change,
not merely a planning edit.

## Complete work inventory

### Typed inspection and reflection contracts

- [ ] Define typed selectors, `SelectedItem`, inspection requests, product
  availability, coherent revision identifiers, and structured success,
  unavailable, incomplete, and failure results.
- [ ] Define an owned renderer-neutral value tree for records, enum variants,
  sequences, scalar and semantic leaves, options, pointers, shared values,
  cycles, and explicit truncation.
- [ ] Make derived structural reflection preserve every struct field and enum
  payload while allowing documented semantic treatment of `Stashed<T>`, raw
  allocation identity, names, symbols, and spans.
- [ ] Represent semantic references as typed `NavigationTarget` values with
  separate display labels and opaque session handles.
- [ ] Bound reflection by depth and node count without performing semantic work
  during later rendering or client-side expansion.
- [ ] Keep service and observation types independent of Axum, JavaScript,
  terminal state, Clap, JSON-RPC, LSP positions, and the oracle schema.
- [ ] Test derived structure, semantic leaf overrides, retained reference
  identity, truncation, shared values, and cycles.
- [ ] Snapshot complete representative `Binder<FnSig>`, `FnCst`/`FnCstData`,
  `CheckedBody`/`TyBodyData`, `ExprCst`, and `TyExpr` values, plus every `Ty`
  variant, so representation changes cannot disappear silently.

### Persistent workspace host

- [ ] Extract an `InspectionHost` which owns the selected Cargo target, one
  live Sage database, source inputs, and reachable dependency metadata.
- [ ] Expose coherent read-only `Analysis` views while keeping mutation and
  workspace reloads at the host boundary.
- [ ] Run synchronous Sage analysis without blocking Axum's asynchronous
  executor or holding unrelated locks across `.await`.
- [ ] Watch represented source files and update existing `SourceFile` inputs in
  the same database.
- [ ] Debounce and classify filesystem changes, correlate every input write
  with its actual Salsa revision, and group multi-write changes under an edit
  batch which is hidden from readers until complete.
- [ ] Detect changes requiring Cargo/dependency reloads and report the reload
  boundary instead of claiming fine-grained reuse.
- [ ] Tag every response and cached browser product with its coherent database
  revision; mark older products stale after an update.
- [ ] Test multiple inspections, input edits, and reloads against the same host.

### Selection, symbol browsing, and navigation

- [ ] Resolve stable absolute semantic paths for modules, free items, and
  associated items without exposing Salsa or arena identities.
- [ ] Adapt source-position selection from the Resolve at Position RFD to the
  same `SelectedItem` type.
- [ ] Return structured ambiguity and not-found results rather than choosing by
  map or source order.
- [ ] Build and filter the selected target's local symbol tree without checking
  every listed body; include generated local symbols and provenance.
- [ ] Exclude dependency symbols from the workspace tree and search count.
- [ ] Navigate every represented local or external symbol reference by retained
  identity, with history, parent/child edges, and return to the selected local
  symbol.
- [ ] Round-trip navigation targets through opaque session handles without
  reparsing their display paths.
- [ ] Preserve incomplete or unsupported child kinds explicitly.

### Semantic inspection products

- [ ] Inspect identity, ownership, provenance, source concrete syntax,
  effective expanded concrete IR, checked signatures and predicates,
  diagnostics, and completed Typed IR where supported by the symbol kind.
- [ ] Render concrete IR, signatures, semantic types, and completed Typed IR as
  deterministic expandable structures with all detail inline.
- [ ] Recursively expand every embedded `Ptr<Ty>` and `Slice<Ptr<Ty>>` and show
  spans, substitutions, dispatch, and explicit elaborations.
- [ ] Keep `SymExt` identity separate from independently requested
  `FnSymbol::sig`, `TraitSymbol::sig`, `TraitSymbol::items`, and
  `SymExt::expanded_module_items` products.
- [ ] Show source, concrete IR, and Typed IR as unavailable for external symbols
  rather than as empty values.
- [ ] Expose relevant impl discovery, trait `prove`, and input-only `normalize`
  through their public Sage boundaries; do not return a selected impl from
  proof or add an expected type to normalization.
- [ ] Preserve diagnostics, ambiguity, overflow, terminal incompleteness, and
  unsupported products rather than rendering plausible completed results.
- [ ] Keep the exact oracle adapters and textual conformance comparison
  unchanged.

### Structured execution evidence

- [ ] Capture every promised query request with its dynamic parent, balanced
  completion, and execution or reuse disposition.
- [ ] Add the required Salsa lifecycle hook in a temporary fork, with the intent
  to upstream it, if existing events cannot observe already-verified memo
  fetches.
- [ ] Retain an unmapped raw fallback so tracing cannot silently omit work.
- [ ] Project supported Salsa queries, Sage semantic operations, solver work,
  and external metadata reads into stable operation families and semantic keys.
- [ ] Separate workspace bootstrap, selection, requested analysis, and pure
  rendering phases.
- [ ] Present one request as a collapsible dynamic tree with filtering,
  execution/reuse state, a draggable width, and full-screen growth.
- [ ] Support exact sets or multisets for closed dependency contracts and
  required/forbidden patterns for extensible contracts; never make sibling
  scheduling order a semantic assertion.
- [ ] Prove that creating or rendering an owned observation adds no hidden
  semantic operations.

### Incremental experiments

- [ ] Retain the last selector and operation for rerun, the last result and
  trace, and a monotonically increasing workspace revision.
- [ ] Compare cold, unchanged-warm, relevant-edit, and unrelated-edit runs in
  one database.
- [ ] Show which operations executed or were reused and distinguish an input
  update from a full workspace reload.
- [ ] Retain bounded revision records containing exact input deltas and zero or
  more inspection runs; do not treat revision advancement as query execution.
- [ ] Compare aligned runs across revisions and distinguish directly observed
  repeated work from an unrecorded causal invalidation edge.
- [ ] Make required and forbidden metadata reads, associated values, impls, and
  callee bodies directly assertable.
- [ ] Provide reusable edit-sequence assertions and migrate the Trait Impl
  Candidate Discovery invalidation matrix when that RFD is implemented.

### Axum and JavaScript application

- [ ] Start an Axum loopback server from `cargo sage inspect`, serve the
  JavaScript application, and open its session URL.
- [ ] Expose on-demand JSON endpoints for symbol branches, selection, products,
  navigation, continuations, traces, revision notifications, and comparison.
- [ ] Publish completed update batches over a reconnectable server-sent event
  stream and refresh visible browser demand without eagerly fetching hidden
  products.
- [ ] Back HTTP handlers with the typed service; keep semantic selection,
  reflection, and pretty-printing out of the transport layer.
- [ ] Prevent unrelated web origins from reading an inspector session and do
  not expose remote bind or source mutation over HTTP.
- [ ] Implement browser state for selection, tabs, filters, tree expansion,
  navigation history, stale revisions, rerun, and revision comparison.
- [ ] Add a revisions view showing input edits and the requested, executed,
  validated, or reused work recorded in each revision.
- [ ] Fetch symbol branches and products only on demand and cache them only for
  their reported revision.
- [ ] Make deeply nested structures and execution trees independently
  collapsible, scrollable, resizable where appropriate, and growable to the
  full viewport.
- [ ] Add service tests, Axum/JSON integration tests, and a small
  JavaScript-facing suite backed by the same service.

### Documentation and review evidence

- [ ] Document a reviewer workflow using `DbDropGuard::db` and `Parse::next`.
- [ ] Snapshot local products, external products, navigation, failures, and the
  dynamic execution tree.
- [ ] Test that opening the symbol browser is lazy, selecting one product reads
  only that product, and rendering performs no semantic work.
- [ ] Test coherent revisions, opaque handle identity, external
  unavailability, explicit truncation, and incomplete metadata.
- [ ] Record how an LSP host can reuse the service without choosing or
  implementing an LSP extension in this RFD.
- [ ] Keep the accepted architecture pages, roadmap, and this status checklist
  current as slices land.
- [ ] Run repository-required full validation before completion.

## Delivery slices

Each slice crosses the necessary workstreams above and ends in something a
reviewer can run and inspect. Checkboxes remain in the inventory so the scope
is not duplicated or accidentally narrowed here.

### Slice 1: Local semantic browser

`cargo sage inspect --package mini-redis` opens a web application backed by a
live, read-only Sage database. A reviewer can find `DbDropGuard::db` and inspect
its source, `FnCst`, `Binder<FnSig>`, diagnostics, and completed `CheckedBody`
as faithful expandable structures.

Acceptance evidence:

- the local tree is built without checking every body;
- opening each tab requests only that product;
- structural snapshots expose actual wrappers, fields, variants, and embedded
  `Ty` trees; and
- client expansion performs no semantic work.

### Slice 2: Semantic navigation

A reviewer can follow local and external symbol references, navigate back, and
inspect external identity, signature, and membership as separate products.
Absolute paths and source positions select the same typed item.

Acceptance evidence:

- links round-trip retained identity through opaque handles;
- changing a display label does not alter identity;
- external symbols remain absent from the workspace tree;
- requesting one external product does not read the others; and
- unavailable and incomplete products remain explicit.

### Slice 3: Focused semantic operations

The same UI and service inspect relevant impl candidates, `Prove(P) -> Proven`,
and `Normalize(alias) -> Type` without changing solver semantics.

Acceptance evidence:

- proof does not expose a selected impl;
- normalization candidate selection has no expected-type input;
- candidate completeness, ambiguity, and overflow are visible; and
- the oracle comparison remains exact and independent.

### Slice 4: Execution evidence

Every inspection can show a complete dynamic request tree, including Salsa
requests and execution/reuse disposition, Sage semantic operations, solver
work, and external metadata reads.

Acceptance evidence:

- no promised request is omitted, including already-verified memo fetches;
- unmapped work appears through the raw fallback;
- required and forbidden dependency assertions are stable under sibling
  scheduling changes; and
- rendering is outside the frozen analysis trace.

### Slice 5: Live incremental experiments

The host watches represented source files, applies edits to the same database,
notifies the browser, refreshes visible demand, and compares the new results
and traces with earlier revisions. A revisions view shows the input changes
and all inspection runs retained for each Salsa revision.

Acceptance evidence:

- cold and unchanged-warm runs use one host;
- relevant and unrelated edits show the intended execution/reuse boundaries;
- older browser products are marked stale; and
- input updates and full workspace reloads are distinguished honestly;
- a revision with no inspection request reports no semantic work; and
- comparisons do not infer causal dependency edges which were not recorded.

## Completion

- [ ] Every item in the complete work inventory is implemented or explicitly
  removed by an accepted design revision.
- [ ] Every delivery slice meets its acceptance evidence.
- [ ] All end-state acceptance tests in the RFD pass.
- [ ] Repository-required full validation passes.
- [ ] The destination and built status are reflected in architecture
  documentation and the build-out roadmap.
- [ ] An LSP server is not required for completion.
