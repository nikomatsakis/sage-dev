# Implementation plan and status

The operational Semantic Inspector and all five delivery slices have landed,
but this RFD remains accepted while source-driven browser evidence for
[SI-A3](./README.md#si-a3), [SI-A4](./README.md#si-a4),
[SI-A5](./README.md#si-a5), and [SI-A7](./README.md#si-a7), plus
[SI-A8](./README.md#si-a8), remain partial. Its
[destination design](./README.md#destination-design) defines the Semantic
Inspector without regard to implementation order. This page inventories the
delivered work and groups it into the reviewer-visible vertical slices used
during implementation.

The [Web Application Walkthrough](./web-application.md) refines the browser,
[JSON Protocol](./protocol.md) pins the bytes at its central boundary. These
documents do not add independent slices; their work lands through the five
parent slices on this page.

The inventory is the scope contract. A slice may be reordered or split without
changing that contract; omitted inventory work requires an RFD design change,
not merely a planning edit.

## Complete work inventory

### Typed inspection and reflection contracts

- [x] Define canonical symbol paths, opaque product IDs, positive product
  lists, one process-wide revision ID, common responses, and structured errors
  ([SI-A4](./README.md#si-a4)).
- [x] Define the exact `/api/v1` route, request, response, error, product,
  generic rendering-tree, reflected-value, run, and revision DTOs below Axum
  ([SI-A2](./README.md#si-a2), [SI-A3](./README.md#si-a3)).
- [x] Define an owned generic rendering tree for page composition and an
  embedded structural value tree for records, variants, sequences, leaves,
  options, transparent stash handles, cycle markers, and explicit truncation.
- [x] Make derived structural reflection preserve every struct field and enum
  payload while allowing only the documented semantic treatment of
  `Stashed<T>`, raw allocation identity, names, symbols, spans, and explicit
  truncation ([SI-A6](./README.md#si-a6), [SI-A16](./README.md#si-a16)).
- [x] Implement the custom `Reflect` derive for ordinary structs and enums and
  explicit implementations for symbols, spans, stashed values, transparent
  arena projection, cycle guards, and limits. Product code must not
  hand-serialize Sage structures
  ([SI-A16](./README.md#si-a16)).
- [x] Represent semantic references with canonical symbol paths, separate
  display labels, and generic presentation data.
- [x] Record structural reflection as a distinct server phase, bound it by
  depth and node count, then freeze the observation before serialization or
  client-side expansion ([SI-A7](./README.md#si-a7)).
- [x] Keep service and observation types independent of Axum, JavaScript,
  terminal state, Clap, JSON-RPC, LSP positions, and the oracle schema.
- [x] Test derived structure, semantic leaf overrides, canonical cross-links,
  truncation, bounded traversal of wide sequences, repeated inline arena
  values, and the cycle guard.
- [x] Snapshot complete representative `Binder<FnSig>`, `FnCst`/`FnCstData`,
  `CheckedBody`/`TyBodyData`, `ExprCst`, and `TyExpr` structural shapes. The
  derive's generated match is exhaustive over every enum (including `Ty`), so
  a new variant is reflected automatically rather than maintained in a second
  handwritten inventory.

### Persistent workspace host

- [x] Implement a dedicated `DatabaseActor` which exclusively owns one
  `InspectionHost`: the selected Cargo target, live Sage database, source
  inputs, reachable dependency metadata, canonical-path index, recorder,
  ephemeral handles, and history.
- [x] Expose a cloneable typed `InspectionClient` backed by a bounded mailbox
  and per-message one-shot owned responses. Do not expose the database or its
  lock to Axum.
- [x] Return coherent owned observations for read requests while keeping
  mutation and workspace reloads at the host boundary.
- [x] Run the actor's synchronous Sage analysis away from Axum's asynchronous
  executor. Process semantic requests, edit batches, and workspace reloads in
  mailbox order. Axum awaits only the one-shot reply and holds no database
  reference or lock.
- [x] Watch represented source files and update existing `SourceFile` inputs in
  the same database.
- [x] Debounce and classify filesystem changes, group multi-write changes under
  an edit batch hidden from readers until complete, and publish that batch
  under the database's actual final coherent Salsa revision.
- [x] Detect changes requiring Cargo/dependency reloads and report the reload
  boundary instead of claiming fine-grained reuse.
- [x] Tag every success and error with its process-wide revision ID. Make the
  browser discard all response-derived state, bootstrap a fresh directory, and
  replay its current URL on mismatch ([SI-A5](./README.md#si-a5)).
- [x] Test multiple inspections, input edits, and reloads against the same host.

### Selection, symbol browsing, and navigation

- [x] Build and filter the selected target's local symbol tree without checking
  every listed body; include generated local symbols and provenance. Return the
  complete detail-free local index in one eager operation so browser search and
  disclosure need no further semantic request.
- [x] Keep search text in the browser. Present every matching local row and
  send only the chosen row's canonical path to the backend.
- [x] Exclude dependency symbols from the workspace tree and search count.
- [ ] Navigate every represented local or external symbol reference by
  canonical path, with history, parent/child edges, and return to the selected
  local symbol.
- [ ] Generate and resolve canonical ownership paths which remain stable across
  unrelated edits, sibling reordering, and host reconstruction; distinguish
  namespaces, unnamed impls, generated symbols, and duplicate external crates;
  never reparse display labels ([SI-A8](./README.md#si-a8)). Ordinary local and
  named external paths are operational; direct replay through an anonymous
  external impl and reorder-stable recovery paths for duplicate local
  definitions remain open.
- [x] Preserve incomplete or unsupported child kinds explicitly.

### Semantic inspection products

- [x] Inspect identity, ownership, provenance, source concrete syntax,
  effective expanded concrete IR, checked signatures and predicates,
  diagnostics, and completed Typed IR where supported by the symbol kind.
- [x] Render concrete IR, signatures, semantic types, and completed Typed IR as
  deterministic expandable structures with all detail inline.
- [x] Recursively expand every embedded `Ptr<Ty>` and `Slice<Ptr<Ty>>` and show
  spans, substitutions, dispatch, and explicit elaborations.
- [x] Keep `SymExt` identity separate from independently requested
  `FnSymbol::sig`, `TraitSymbol::sig`, `TraitSymbol::items`, and
  `SymExt::expanded_module_items` products.
- [x] Omit source, concrete IR, and Typed IR from external-symbol product
  catalogs rather than exposing empty or disabled pages.
- [x] Preserve diagnostics, ambiguity, overflow, terminal incompleteness, and
  unsupported semantic nodes inside listed products rather than rendering
  plausible completed results.
- [x] Keep the exact oracle adapters and textual conformance comparison
  unchanged.

### Structured execution evidence

- [x] Capture every promised query invocation with its dynamic parent, balanced
  completion, and execution, validation, reuse, or cancellation disposition.
- [x] Losslessly coalesce consecutive identical leaf requests with an explicit
  observation count so high-volume solver traces remain inspectable without
  sampling or dropping cache lookups.
- [x] Fork Salsa temporarily so every tracked-query invocation creates a
  balanced span before memo lookup, including already-current memo fetches;
  require no annotations on individual Sage queries and keep the change
  upstreamable ([SI-A17](./README.md#si-a17)).
- [x] Retain a stable unmapped event category so checked tracing cannot
  silently omit work, with the raw debug payload available in diagnostics and
  failure artifacts.
- [x] Project Salsa queries, Sage semantic operations, solver work, and
  external metadata reads into stable operation families; attach semantic keys
  where a projection exists and retain every other query in the explicit raw
  `unmapped` fallback.
- [x] Separate workspace bootstrap, selection, requested analysis, structural
  reflection, pure render-tree assembly, and post-trace serialization/browser phases
  ([SI-A7](./README.md#si-a7)).
- [x] Present one request as a collapsible dynamic tree with filtering,
  execution/reuse state, a draggable width, and full-screen growth.
- [x] Support exact sets or multisets for closed dependency contracts and
  required/forbidden patterns for extensible contracts; never make sibling
  scheduling order a semantic assertion. Define the checked deterministic
  projection and retain raw order/debug keys only as failure artifacts
  ([SI-A12](./README.md#si-a12)).
- [x] Carry producer-authored child ordering through balanced semantic spans;
  keep ordinary Salsa requests sequential, mark solver requests sequential and
  trait and normalization candidate groups unordered, scope async spans to
  individual future polls, and never infer order from operation-name
  substrings.
- [x] Prove that every tracked read used to create the owned observation occurs
  inside the recorded analysis/reflection boundary, and that later view
  assembly, serialization, and browser rendering add no semantic operations.

### Incremental experiments

- [x] Retain the last selector and operation for rerun, the last result and
  trace, and a monotonically increasing workspace revision.
- [x] Compare cold, unchanged-warm, relevant-edit, and unrelated-edit runs in
  one database.
- [x] Show which operations executed or were reused and distinguish an input
  update from a full workspace reload.
- [x] Retain bounded revision records containing exact input deltas and zero or
  more inspection runs; do not treat revision advancement as query execution.
- [x] Compare aligned runs across revisions and distinguish directly observed
  repeated work from an unrecorded causal invalidation edge.
- [x] Make required and forbidden metadata reads, associated values, impls, and
  callee bodies directly assertable.
- [x] Provide reusable edit-sequence assertions and migrate the Trait Impl
  Candidate Discovery invalidation matrix when that RFD is implemented.

### Axum and React application

- [x] Start an Axum loopback server from `cargo sage inspect`, serve the
  embedded React/TypeScript application, and open its application URL. Bind to
  `127.0.0.1:2442` by default, permit an explicit port override, and reserve
  port `0` for operating-system-assigned test isolation.
- [x] Build the frontend with Vite and embed its production assets with
  `rust-embed`; use npm with a committed lockfile and keep normal
  `cargo sage inspect` use to one process.
- [x] Expose JSON endpoints for the current revision, complete local symbol
  index, and on-demand selection, products, external membership, navigation,
  continuations, traces, revision notifications, and comparison.
- [x] Emit a complete selected-symbol product list without requesting any
  advertised product; make the browser create tabs only from its opaque IDs,
  labels, and URLs ([SI-A4](./README.md#si-a4),
  [SI-A15](./README.md#si-a15)).
- [x] Publish completed update batches over a reconnectable server-sent event
  stream. A new revision ID reloads the current semantic URL, which fetches the
  complete local index and current product without eagerly fetching hidden
  products.
- [x] Back HTTP handlers only with `InspectionClient`; keep direct database
  access, semantic selection, reflection, and pretty-printing out of the
  transport layer.
- [x] Keep the listener loopback-only and do not expose source mutation over
  HTTP.
- [x] Implement browser state for selection, tabs, filters, tree expansion,
  navigation history, current revision, rerun, and revision comparison.
- [x] Make the current semantic view URL-addressable with React Router; test
  direct loading, Back/Forward, push versus replace, unresolved paths, and
  full state reset/bootstrap/URL replay after a revision mismatch.
- [x] Add a revisions view showing input edits and the requested, executed,
  validated, or reused work recorded in each revision.
- [x] Fetch the complete detail-free local symbol index eagerly. Filter and
  disclose it locally; fetch semantic products only on demand and cache them
  only for their reported revision.
- [x] Emit concise structured HTTP/provider demand logs from
  `cargo sage inspect` without claiming complete Salsa cache-hit coverage
  before the tracing slice.
- [x] Make deeply nested structures and execution trees independently
  collapsible, scrollable, resizable where appropriate, and growable to the
  full viewport.
- [x] Add service tests, Axum/JSON integration tests, Vitest/React Testing
  Library component tests, and a small real-process deployment/API suite.
- [ ] Pin exact source-driven response bytes for every successful `/api/v1`
  DTO and the continuation and revision-history route families. The current
  real-process suite pins representative success and structured-error bytes,
  contractual headers, assets, and actor demand.
- [x] Create one checked-in Cargo sample project and make live provider,
  command, Axum, value, and demand snapshots originate from its Rust source
  ([SI-A3](./README.md#si-a3)).
- [x] Remove the scripted semantic provider, `--fixture` command mode, route
  manifest, static JSON responses, and dummy API server.
- [ ] Drive the embedded application with a headless browser against the live
  sample-project server; snapshot visible state and correlate browser actions
  with actor/query demand.
- [x] Make the small real-process evidence suite launch the actual
  `cargo sage inspect` command with port `0` and compare one stable transcript
  containing routed assets, reviewed result fields, semantic API requests,
  provider operations, and the requested retained run.

### Documentation and review evidence

- [x] Document a reviewer workflow using `DbDropGuard::db` and `Parse::next`.
- [x] Snapshot local products, external products, navigation, failures, and the
  dynamic execution tree.
- [x] Map every `SI-A<n>` anchor to its implementing checkpoint and focused
  evidence; keep the mapping current when either changes.
- [x] Test that opening the symbol browser eagerly reads only the local
  membership needed for summaries—not checked signatures, field types,
  bodies, associated values, impl candidates, or external metadata beyond the
  explicit macro families—selecting one product reads only that product, and
  rendering performs no semantic work.
- [x] Test coherent revisions, canonical-path stability/failure, omitted
  external products, explicit truncation, and incomplete metadata.
- [x] Record how an LSP host can reuse the service without choosing or
  implementing an LSP extension in this RFD.
- [x] Keep the accepted architecture pages, roadmap, and this status checklist
  current as slices land.
- [x] Run repository-required full validation before completion.

## Design-anchor status by slice

- 🛑 means the anchor has not been introduced.
- 🟡 means its structure exists, but its complete required evidence does not.
- ✅ means the anchor is established and is a regression gate.

The five slices are source-driven from their first semantic result. The status
links describe what changes at each anchor's first yellow or green cell.

| Anchor | Baseline | [S1 Live shell](#slice-1-live-shell-and-workspace-symbols) | [S2 Products](#slice-2-real-selected-symbol-products) | [S3 Navigation](#slice-3-navigation-and-metadata) | [S4 Tracing](#slice-4-salsa-event-chain-and-execution-tree) | [S5 Revisions](#slice-5-live-updates-and-revision-history) |
|---|:---:|:---:|:---:|:---:|:---:|:---:|
| [SI-A1](./README.md#si-a1) database actor | 🛑 | [🟡](#si-a1-status) | 🟡 | 🟡 | 🟡 | [✅](#si-a1-status) |
| [SI-A2](./README.md#si-a2) typed service below Axum | 🛑 | [✅](#si-a2-status) | ✅ | ✅ | ✅ | ✅ |
| [SI-A3](./README.md#si-a3) source-driven integration | 🛑 | [🟡](#si-a3-status) | 🟡 | 🟡 | 🟡 | 🟡 |
| [SI-A4](./README.md#si-a4) generic frontend | 🛑 | [🟡](#si-a4-status) | 🟡 | 🟡 | 🟡 | 🟡 |
| [SI-A5](./README.md#si-a5) URL intent and reset | 🛑 | [🟡](#si-a5-status) | 🟡 | 🟡 | 🟡 | 🟡 |
| [SI-A6](./README.md#si-a6) faithful bounded reflection | 🛑 | [🟡](#si-a6-status) | [✅](#si-a6-status) | ✅ | ✅ | ✅ |
| [SI-A7](./README.md#si-a7) semantic work before rendering | 🛑 | 🛑 | [🟡](#si-a7-status) | 🟡 | 🟡 | 🟡 |
| [SI-A8](./README.md#si-a8) canonical symbol paths | 🛑 | [🟡](#si-a8-status) | 🟡 | 🟡 | 🟡 | 🟡 |
| [SI-A9](./README.md#si-a9) eager detail-free index | 🛑 | [✅](#si-a9-status) | ✅ | ✅ | ✅ | ✅ |
| [SI-A11](./README.md#si-a11) complete demand observation | 🛑 | 🛑 | 🛑 | 🛑 | [✅](#si-a11-status) | ✅ |
| [SI-A12](./README.md#si-a12) stable evidence projection | 🛑 | [🟡](#si-a12-status) | 🟡 | 🟡 | [✅](#si-a12-status) | ✅ |
| [SI-A13](./README.md#si-a13) revisions versus runs | 🛑 | 🛑 | 🛑 | 🛑 | 🛑 | [✅](#si-a13-status) |
| [SI-A14](./README.md#si-a14) oracle independence | [✅](#si-a14-status) | ✅ | ✅ | ✅ | ✅ | ✅ |
| [SI-A15](./README.md#si-a15) positive non-eager products | 🛑 | [🟡](#si-a15-status) | 🟡 | [✅](#si-a15-status) | ✅ | ✅ |
| [SI-A16](./README.md#si-a16) derive-driven reflection | 🛑 | [🟡](#si-a16-status) | [✅](#si-a16-status) | ✅ | ✅ | ✅ |
| [SI-A17](./README.md#si-a17) Salsa invocation spans | 🛑 | 🛑 | 🛑 | 🛑 | [✅](#si-a17-status) | ✅ |

## Current anchor dispositions

<a id="si-a1-status"></a>
### SI-A1 — coherent actor ownership

Slice 1 introduced the database-owning actor. Slice 5 established ordered,
same-host edit and reload behavior with retained revision evidence.

<a id="si-a2-status"></a>
### SI-A2 — service below transport

Slice 1 established typed service operations, a bounded actor client, and Axum
handlers which only decode, await, and encode.

<a id="si-a3-status"></a>
### SI-A3 — source-driven integration

Slice 1 now starts from
`test-projects/semantic-inspector/db-drop-guard` and crosses the live host,
provider, actor, command, and Axum boundary. The scripted provider, fixture
command mode, route manifest, dummy server, and precomputed API responses have
been removed. This remains yellow until a headless browser drives the embedded
application against that same live server and snapshots UI plus demand.

<a id="si-a4-status"></a>
### SI-A4 — generic frontend

The client remains a generic interpreter and isolated component tests cover
render nodes and semantic links without API mocks. Source-driven browser
navigation and a static no-product-dispatch audit are still required for green.

<a id="si-a5-status"></a>
### SI-A5 — durable URL intent

The implementation retains URL-driven state, revision reset, and replay.
The former mocked browser tests no longer count as anchor evidence; direct-load,
Back/Forward, reconnect, and missing-target behavior must be re-established
through the source-driven headless-browser suite.

<a id="si-a6-status"></a>
### SI-A6 — faithful bounded reflection

Slice 1 introduced the generic value vocabulary. Slice 2 established real
Concrete IR, signature, and Typed-IR shape snapshots, repeated-edge inlining,
the cycle guard, continuations, and node/depth limits.

<a id="si-a7-status"></a>
### SI-A7 — semantic work is frozen before rendering

Slice 2 separated reflection and view assembly. Slice 4 established phase
attribution and proved that server-side view assembly adds no semantic database
work. Source-driven browser demand evidence remains required for green.

<a id="si-a8-status"></a>
### SI-A8 — canonical symbol paths

Local and named external round trips, reconstruction, reorder, and rename
behavior are implemented. Anonymous external impl traversal and the complete
duplicate-local-definition matrix remain partial.

<a id="si-a9-status"></a>
### SI-A9 — one eager detail-free index

Slice 1 established the live source-driven index and its exact required and
forbidden query families, including the narrow macro-expansion exception.

<a id="si-a11-status"></a>
### SI-A11 — complete demand observation

Slice 4 established balanced query invocation spans, explicit unmapped
fallback, repeated-leaf multiplicity, and cancellation/unwinding evidence.

<a id="si-a12-status"></a>
### SI-A12 — stable evidence projection

Slice 1 introduced stable request transcripts. Slice 4 established structured
query keys, dispositions, producer-authored child ordering, and checked
set/multiset comparison.

<a id="si-a13-status"></a>
### SI-A13 — revisions and runs remain distinct

Slice 5 established retained edit deltas, zero-run revisions, separate demanded
runs, reload generations, and comparison of observed repeated work.

<a id="si-a14-status"></a>
### SI-A14 — oracle independence

This was a baseline constraint and remains established: Inspector values are
not oracle inputs, and the oracle does not repair Inspector output.

<a id="si-a15-status"></a>
### SI-A15 — positive non-eager products

Slice 1 introduced server-authored catalogs. Slice 3 established local and
external catalogs, omission of unsupported pages, and product-specific demand.
The live black-box regression selects \`Db\` from the source-built directory and
successfully fetches its advertised identity page.

<a id="si-a16-status"></a>
### SI-A16 — derive-driven reflection

Slice 1 introduced the reflection crate and derive. Slice 2 established it over
real Sage products with explicit semantic leaves only for links and storage
projections.

<a id="si-a17-status"></a>
### SI-A17 — Salsa invocation spans

Slice 4 established balanced invocation spans before memo lookup, including
already-current reuse, cancellation, unwinding, and poll-scoped semantic spans.

## Delivery slices

### Slice 1: Live shell and workspace symbols

This slice starts the production command over a checked-in Cargo sample
project. It includes the typed service, database actor, Axum, embedded React
assets, current revision/session resources, one complete detail-free local
symbol index, browser-local filtering, and canonical local selection. Its
snapshots are produced by the real host and server.

Current status: implemented. The source-driven headless-browser portion of
SI-A3/A4/A5 remains open.

### Slice 2: Real selected-symbol products

This slice adds source, concrete IR, signatures, diagnostics, and Typed IR from
the live Sage database, together with derive-driven structural reflection,
bounded continuation, and generic render-tree assembly.

Current status: implemented.

### Slice 3: Navigation and metadata

This slice adds canonical local/external references, external parents and
children, independent external products, and positive product catalogs.

Current status: implemented for ordinary local and named external paths;
SI-A8's anonymous-external-impl and duplicate-local matrices remain open.

### Slice 4: Salsa event chain and execution tree

This slice adds balanced Salsa invocation spans, stable semantic projections,
execution/validation/reuse/cancellation disposition, explicit unmapped
fallback, solver ordering, and the collapsible execution tree.

Current status: implemented.

### Slice 5: Live updates and revision history

This slice adds file watching, coherent same-database input batches, explicit
workspace reloads, SSE revision notification, retained revisions and runs, and
comparison of observed work across revisions.

Current status: implemented.

## Completion

The Inspector is operational and its semantic backend evidence is now
source-driven. The RFD remains accepted until:

- a headless browser drives the embedded application against the live sample
  project and re-establishes SI-A3, SI-A4, SI-A5, and SI-A7 interaction
  evidence; and
- SI-A8's remaining canonical-path matrices are complete.

Before completion, run the repository-required Rust, frontend, documentation,
post-codegen, and Sage sanity validation and update the architecture Current
Status section with the final evidence.
