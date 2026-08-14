# Implementation plan and status

This RFD is a draft. Its [destination design](./README.md#destination-design)
defines the completed Semantic Inspector without regard to implementation
order. This page first inventories all work required by that design, then
groups the work into reviewer-visible vertical slices.

The [Web Application Walkthrough](./web-application.md) refines the browser,
[JSON Protocol](./protocol.md) pins the bytes at its central boundary. These
documents do not add independent slices; their work lands through the seven
parent slices on this page.

The inventory is the scope contract. A slice may be reordered or split without
changing that contract; omitted inventory work requires an RFD design change,
not merely a planning edit.

## Complete work inventory

### Typed inspection and reflection contracts

- [ ] Define typed selectors, `SelectedItem`, inspection requests, product
  catalogs with available/unavailable/not-applicable entries, coherent
  revision identifiers, and structured available, unavailable, incomplete,
  failed, and cancelled results ([SI-A4](./README.md#si-a4)).
- [ ] Define the exact `/api/v1` route, request, tagged envelope, product
  descriptor, reflected value, focused-operation, run, and revision DTOs below
  Axum ([SI-A2](./README.md#si-a2), [SI-A3](./README.md#si-a3)).
- [ ] Define an owned renderer-neutral value tree for records, enum variants,
  sequences, scalar and semantic leaves, options, pointers, shared values,
  cycles, and explicit truncation.
- [ ] Make derived structural reflection preserve every struct field and enum
  payload while allowing only the documented semantic treatment of
  `Stashed<T>`, raw allocation identity, names, symbols, spans, and explicit
  truncation ([SI-A6](./README.md#si-a6)).
- [ ] Represent semantic references as typed `NavigationTarget` values with
  separate display labels and opaque session handles.
- [ ] Record structural reflection as a distinct server phase, bound it by
  depth and node count, then freeze the observation before serialization or
  client-side expansion ([SI-A7](./README.md#si-a7)).
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
  associated items without exposing Salsa or arena identities; land this with
  slice 3.
- [ ] Adapt source-position selection from the Resolve at Position RFD to the
  same `SelectedItem` type; land this with slice 4.
- [ ] Return structured ambiguity and not-found results rather than choosing by
  map or source order.
- [ ] Build and filter the selected target's local symbol tree without checking
  every listed body; include generated local symbols and provenance. Return the
  complete detail-free local index in one eager operation so browser search and
  disclosure need no further semantic request.
- [ ] Exclude dependency symbols from the workspace tree and search count.
- [ ] Navigate every represented local or external symbol reference by retained
  identity, with history, parent/child edges, and return to the selected local
  symbol.
- [ ] Round-trip navigation targets through opaque session handles without
  reparsing their display paths ([SI-A8](./README.md#si-a8)).
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
  proof or add an expected type to normalization. Accept only opaque typed
  operation-target handles emitted by reflected semantic nodes
  ([SI-A10](./README.md#si-a10)).
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
- [ ] Retain a stable unmapped event category so checked tracing cannot
  silently omit work, with the raw debug payload available in diagnostics and
  failure artifacts.
- [ ] Project supported Salsa queries, Sage semantic operations, solver work,
  and external metadata reads into stable operation families and semantic keys.
- [ ] Separate workspace bootstrap, selection, requested analysis, structural
  reflection, and post-trace serialization/browser rendering phases
  ([SI-A7](./README.md#si-a7)).
- [ ] Present one request as a collapsible dynamic tree with filtering,
  execution/reuse state, a draggable width, and full-screen growth.
- [ ] Support exact sets or multisets for closed dependency contracts and
  required/forbidden patterns for extensible contracts; never make sibling
  scheduling order a semantic assertion. Define the checked deterministic
  projection and retain raw order/debug keys only as failure artifacts
  ([SI-A12](./README.md#si-a12)).
- [ ] Prove that every tracked read used to create the owned observation is
  recorded in the reflection phase, and that later serialization and browser
  rendering add no semantic operations.

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

### Axum and React application

- [ ] Start an Axum loopback server from `cargo sage inspect`, serve the
  embedded React/TypeScript application, and open its session URL.
- [ ] Build the frontend with Vite and embed its production assets with
  `rust-embed`; use npm with a committed lockfile and keep normal
  `cargo sage inspect` use to one process.
- [ ] Expose JSON endpoints for the complete local symbol index and on-demand
  selection, products, external membership, navigation, continuations, traces,
  revision notifications, and comparison.
- [ ] Emit a complete selected-symbol product catalog without requesting any
  advertised product; make the browser render only that server-authored
  availability ([SI-A4](./README.md#si-a4),
  [SI-A15](./README.md#si-a15)).
- [ ] Publish completed update batches over a reconnectable server-sent event
  stream and refresh visible browser demand without eagerly fetching hidden
  products.
- [ ] Back HTTP handlers with the typed service; keep semantic selection,
  reflection, and pretty-printing out of the transport layer.
- [ ] Prevent unrelated web origins from reading an inspector session and do
  not expose remote bind or source mutation over HTTP.
- [ ] Implement browser state for selection, tabs, filters, tree expansion,
  navigation history, stale revisions, rerun, and revision comparison.
- [ ] Make the current semantic view URL-addressable with React Router; test
  direct loading, reload, Back/Forward, push versus replace, and expired
  session-scoped handles.
- [ ] Add a revisions view showing input edits and the requested, executed,
  validated, or reused work recorded in each revision.
- [ ] Fetch the complete detail-free local symbol index eagerly. Filter and
  disclose it locally; fetch semantic products only on demand and cache them
  only for their reported revision.
- [ ] Emit concise structured HTTP/provider demand logs from
  `cargo sage inspect` without claiming complete Salsa cache-hit coverage
  before slice 6.
- [ ] Make deeply nested structures and execution trees independently
  collapsible, scrollable, resizable where appropriate, and growable to the
  full viewport.
- [ ] Add service tests, Axum/JSON integration tests, Vitest/React Testing
  Library component tests, and a small Playwright suite.
- [ ] Create one reviewed API fixture bundle containing a Rust source fixture,
  exact route manifest, request and response bytes, expected demand, and
  navigation scenarios ([SI-A3](./README.md#si-a3)).
- [ ] In slice 2, construct independent typed scripted Rust values and use
  Snapbox to compare their actual Axum bytes and provider demand with the
  bundle. Never deserialize an expected response as the value under test.
- [ ] Run frontend tests against a strict static server for that same bundle;
  reject unknown routes and record required and forbidden demand.
- [ ] Make the small Playwright evidence suite launch the real
  `cargo sage inspect` command on a random loopback port and compare one stable
  transcript containing browser actions, routed and rendered results, semantic
  API requests, provider operations, and later correlated Salsa/Sage events.

### Documentation and review evidence

- [ ] Document a reviewer workflow using `DbDropGuard::db` and `Parse::next`.
- [ ] Snapshot local products, external products, navigation, failures, and the
  dynamic execution tree.
- [ ] Map every `SI-A<n>` anchor to its implementing checkpoint and focused
  evidence; keep the mapping current when either changes.
- [ ] Test that opening the symbol browser eagerly reads only the local
  membership needed for summaries—not checked signatures, field types,
  bodies, associated values, impl candidates, or external metadata—selecting
  one product reads only that product, and rendering performs no semantic
  work.
- [ ] Test coherent revisions, opaque handle identity, external
  unavailability, explicit truncation, and incomplete metadata.
- [ ] Record how an LSP host can reuse the service without choosing or
  implementing an LSP extension in this RFD.
- [ ] Keep the accepted architecture pages, roadmap, and this status checklist
  current as slices land.
- [ ] Run repository-required full validation before completion.

## Design-anchor status by slice

- 🛑 means the anchor has not been introduced.
- 🟡 means its structure exists, but its complete required evidence does not.
- ✅ means the anchor is established and becomes a regression gate for every
  later slice.

Every symbol is a link. A stop sign links to the anchor's destination rule, a
yellow circle links to the transition which introduced it, and a checkmark
links to the transition and evidence which established it.

| Anchor | Baseline | [S1 UI](#slice-1-protocol-and-fixture-backed-ui) | [S2 Axum](#slice-2-axum-transport-and-embedded-application) | [S3 Symbols](#slice-3-real-workspace-symbols) | [S4 Products](#slice-4-real-selected-symbol-products) | [S5 Navigation](#slice-5-navigation-metadata-and-focused-operations) | [S6 Tracing](#slice-6-salsa-event-chain-and-execution-tree) | [S7 Revisions](#slice-7-live-updates-and-revision-history) |
|---|:---:|:---:|:---:|:---:|:---:|:---:|:---:|:---:|
| [SI-A1](./README.md#si-a1) live host | [🛑](./README.md#si-a1 "Not introduced") | [🛑](./README.md#si-a1 "Not introduced") | [🛑](./README.md#si-a1 "Not introduced") | [🟡](#si-a1-slice-3 "Introduced") | [🟡](#si-a1-slice-3 "Introduced") | [🟡](#si-a1-slice-3 "Introduced") | [🟡](#si-a1-slice-3 "Introduced") | [✅](#si-a1-slice-7 "Established") |
| [SI-A2](./README.md#si-a2) typed service below Axum | [🛑](./README.md#si-a2 "Not introduced") | [🛑](./README.md#si-a2 "Not introduced") | [✅](#si-a2-slice-2 "Established") | [✅](#si-a2-slice-2 "Established") | [✅](#si-a2-slice-2 "Established") | [✅](#si-a2-slice-2 "Established") | [✅](#si-a2-slice-2 "Established") | [✅](#si-a2-slice-2 "Established") |
| [SI-A3](./README.md#si-a3) exact shared JSON | [🛑](./README.md#si-a3 "Not introduced") | [🟡](#si-a3-slice-1 "Introduced") | [✅](#si-a3-slice-2 "Established") | [✅](#si-a3-slice-2 "Established") | [✅](#si-a3-slice-2 "Established") | [✅](#si-a3-slice-2 "Established") | [✅](#si-a3-slice-2 "Established") | [✅](#si-a3-slice-2 "Established") |
| [SI-A4](./README.md#si-a4) catalog-driven UI | [🛑](./README.md#si-a4 "Not introduced") | [✅](#si-a4-slice-1 "Established") | [✅](#si-a4-slice-1 "Established") | [✅](#si-a4-slice-1 "Established") | [✅](#si-a4-slice-1 "Established") | [✅](#si-a4-slice-1 "Established") | [✅](#si-a4-slice-1 "Established") | [✅](#si-a4-slice-1 "Established") |
| [SI-A5](./README.md#si-a5) URL semantic view | [🛑](./README.md#si-a5 "Not introduced") | [✅](#si-a5-slice-1 "Established") | [✅](#si-a5-slice-1 "Established") | [✅](#si-a5-slice-1 "Established") | [✅](#si-a5-slice-1 "Established") | [✅](#si-a5-slice-1 "Established") | [✅](#si-a5-slice-1 "Established") | [✅](#si-a5-slice-1 "Established") |
| [SI-A6](./README.md#si-a6) faithful bounded reflection | [🛑](./README.md#si-a6 "Not introduced") | [🟡](#si-a6-slice-1 "Introduced") | [🟡](#si-a6-slice-1 "Introduced") | [🟡](#si-a6-slice-1 "Introduced") | [✅](#si-a6-slice-4 "Established") | [✅](#si-a6-slice-4 "Established") | [✅](#si-a6-slice-4 "Established") | [✅](#si-a6-slice-4 "Established") |
| [SI-A7](./README.md#si-a7) analysis/reflection/rendering split | [🛑](./README.md#si-a7 "Not introduced") | [🛑](./README.md#si-a7 "Not introduced") | [🛑](./README.md#si-a7 "Not introduced") | [🛑](./README.md#si-a7 "Not introduced") | [🟡](#si-a7-slice-4 "Introduced") | [🟡](#si-a7-slice-4 "Introduced") | [✅](#si-a7-slice-6 "Established") | [✅](#si-a7-slice-6 "Established") |
| [SI-A8](./README.md#si-a8) retained identity | [🛑](./README.md#si-a8 "Not introduced") | [🟡](#si-a8-slice-1 "Introduced") | [🟡](#si-a8-slice-1 "Introduced") | [🟡](#si-a8-slice-1 "Introduced") | [🟡](#si-a8-slice-1 "Introduced") | [✅](#si-a8-slice-5 "Established") | [✅](#si-a8-slice-5 "Established") | [✅](#si-a8-slice-5 "Established") |
| [SI-A9](./README.md#si-a9) eager detail-free index | [🛑](./README.md#si-a9 "Not introduced") | [🟡](#si-a9-slice-1 "Introduced") | [🟡](#si-a9-slice-1 "Introduced") | [✅](#si-a9-slice-3 "Established") | [✅](#si-a9-slice-3 "Established") | [✅](#si-a9-slice-3 "Established") | [✅](#si-a9-slice-3 "Established") | [✅](#si-a9-slice-3 "Established") |
| [SI-A10](./README.md#si-a10) typed focused operations | [🛑](./README.md#si-a10 "Not introduced") | [🟡](#si-a10-slice-1 "Introduced") | [🟡](#si-a10-slice-1 "Introduced") | [🟡](#si-a10-slice-1 "Introduced") | [🟡](#si-a10-slice-1 "Introduced") | [✅](#si-a10-slice-5 "Established") | [✅](#si-a10-slice-5 "Established") | [✅](#si-a10-slice-5 "Established") |
| [SI-A11](./README.md#si-a11) complete demand observation | [🛑](./README.md#si-a11 "Not introduced") | [🛑](./README.md#si-a11 "Not introduced") | [🛑](./README.md#si-a11 "Not introduced") | [🛑](./README.md#si-a11 "Not introduced") | [🛑](./README.md#si-a11 "Not introduced") | [🛑](./README.md#si-a11 "Not introduced") | [✅](#si-a11-slice-6 "Established") | [✅](#si-a11-slice-6 "Established") |
| [SI-A12](./README.md#si-a12) stable evidence projection | [🛑](./README.md#si-a12 "Not introduced") | [🟡](#si-a12-slice-1 "Introduced") | [🟡](#si-a12-slice-1 "Introduced") | [🟡](#si-a12-slice-1 "Introduced") | [🟡](#si-a12-slice-1 "Introduced") | [🟡](#si-a12-slice-1 "Introduced") | [✅](#si-a12-slice-6 "Established") | [✅](#si-a12-slice-6 "Established") |
| [SI-A13](./README.md#si-a13) revisions versus runs | [🛑](./README.md#si-a13 "Not introduced") | [🛑](./README.md#si-a13 "Not introduced") | [🛑](./README.md#si-a13 "Not introduced") | [🛑](./README.md#si-a13 "Not introduced") | [🛑](./README.md#si-a13 "Not introduced") | [🛑](./README.md#si-a13 "Not introduced") | [🛑](./README.md#si-a13 "Not introduced") | [✅](#si-a13-slice-7 "Established") |
| [SI-A14](./README.md#si-a14) oracle independence | [✅](#si-a14-baseline "Established") | [✅](#si-a14-baseline "Established") | [✅](#si-a14-baseline "Established") | [✅](#si-a14-baseline "Established") | [✅](#si-a14-baseline "Established") | [✅](#si-a14-baseline "Established") | [✅](#si-a14-baseline "Established") | [✅](#si-a14-baseline "Established") |
| [SI-A15](./README.md#si-a15) narrow semantic catalogs | [🛑](./README.md#si-a15 "Not introduced") | [🛑](./README.md#si-a15 "Not introduced") | [🛑](./README.md#si-a15 "Not introduced") | [🟡](#si-a15-slice-3 "Introduced") | [🟡](#si-a15-slice-3 "Introduced") | [✅](#si-a15-slice-5 "Established") | [✅](#si-a15-slice-5 "Established") | [✅](#si-a15-slice-5 "Established") |

A slice is complete only when every anchor which turns green in its column has
the evidence named below and every already-green anchor remains green.

## Anchor transitions

These transitions explain what each yellow or green cell means. Repeated
yellow cells link to the original introduction; repeated green cells link to
the evidence which first established the anchor.

<a id="si-a1-slice-3"></a>
### SI-A1 — Slice 3: introduced

The real workspace-symbol service first constructs an `InspectionHost` and
keeps one Sage database alive across requests. The anchor remains yellow
because this slice does not yet watch files or prove same-database edit reuse.

<a id="si-a1-slice-7"></a>
### SI-A1 — Slice 7: established

Cold, unchanged-warm, relevant-edit, and unrelated-edit tests run against one
host. Evidence distinguishes an ordinary input update from a workspace reload
and proves that coherent reads never observe a partially applied edit batch.

<a id="si-a2-slice-2"></a>
### SI-A2 — Slice 2: established

Typed Rust service requests and results exist below Axum. Handler tests prove
that Axum only parses protocol DTOs, invokes that service, maps transport
status, and serializes the result; no semantic selection or rendering lives in
the handler.

<a id="si-a3-slice-1"></a>
### SI-A3 — Slice 1: introduced

The normative protocol and exact fixture bundle drive a strict dummy server
and the browser. This pins the intended bytes from the client side, but no Rust
DTO or Axum implementation exists yet.

<a id="si-a3-slice-2"></a>
### SI-A3 — Slice 2: established

Independent, typed scripted Rust values pass through the production DTO and
Axum serialization path. Snapbox compares status, contractual headers, body
bytes, and provider demand with the reviewed bundle. The test never
deserializes its expected response to manufacture the value under test.

<a id="si-a4-slice-1"></a>
### SI-A4 — Slice 1: established

Browser tests prove that server-provided product catalogs alone determine
which tabs and actions appear, how unavailable states are explained, and which
resources are fetched. The client does not infer availability from an empty
value or duplicate an origin/kind table.

<a id="si-a5-slice-1"></a>
### SI-A5 — Slice 1: established

Every semantic selection, tab, focused operation, and revision view has a
canonical URL. Direct load, reload, Back, and Forward tests establish the
push-versus-replace policy against the dummy server.

<a id="si-a6-slice-1"></a>
### SI-A6 — Slice 1: introduced

One generic browser renderer handles the complete `ValueNode` protocol
vocabulary, including references, sharing, cycles, and truncation. Fixture
tests establish the interaction model, but cannot yet prove faithful
reflection of real Rust structures.

<a id="si-a6-slice-4"></a>
### SI-A6 — Slice 4: established

Derived Rust reflection snapshots preserve every field and enum payload for
representative concrete IR, signature, and Typed IR values, recursively cover
every `Ty` variant, and explicitly test semantic leaves, sharing, cycles, and
limits.

<a id="si-a7-slice-4"></a>
### SI-A7 — Slice 4: introduced

Real product requests separate semantic analysis from bounded structural
reflection and freeze an owned observation before serialization. Complete
phase attribution is not yet observable without the tracing work.

<a id="si-a7-slice-6"></a>
### SI-A7 — Slice 6: established

Execution evidence identifies selection, requested analysis, structural
reflection, and post-trace serialization as separate phases. Tests prove that
every semantic read used by reflection is inside the recorded boundary and
that JSON and browser rendering add none.

<a id="si-a8-slice-1"></a>
### SI-A8 — Slice 1: introduced

Fixture semantic references carry opaque navigation handles separately from
display labels, and all browser navigation uses those handles. Real Sage
identity and external metadata navigation are not connected yet.

<a id="si-a8-slice-5"></a>
### SI-A8 — Slice 5: established

Local and external references round-trip through real retained identities.
Tests cover parent and child navigation, handle expiry, and changing a display
label without changing the selected symbol.

<a id="si-a9-slice-1"></a>
### SI-A9 — Slice 1: introduced

The browser eagerly receives one complete detail-free fixture index, then
searches and expands it locally. No fixture detail request is permitted during
those interactions.

<a id="si-a9-slice-3"></a>
### SI-A9 — Slice 3: established

The same behavior is backed by the real Sage workspace. Provider-demand tests
prove that the complete local index excludes dependencies and does not request
signatures, field types, bodies, associated values, impl candidates, or
external metadata.

<a id="si-a10-slice-1"></a>
### SI-A10 — Slice 1: introduced

The fixture protocol and UI expose focused impl, proof, and normalization
requests only through opaque typed operation-target handles. No real solver
operation is invoked yet.

<a id="si-a10-slice-5"></a>
### SI-A10 — Slice 5: established

Real reflected nodes mint the handles and invoke Sage's public impl-discovery,
`Prove(P) -> Proven`, and input-only `Normalize(alias) -> Type` boundaries.
Tests prove that proof exposes no selected impl and normalization accepts no
expected type.

<a id="si-a11-slice-6"></a>
### SI-A11 — Slice 6: established

Balanced lifecycle events cover requests, returns, execution, validated memo
reuse, and already-verified cache hits. A stable fallback category prevents
unknown work from being silently omitted and therefore prevents a false claim
of complete dependency observation.

<a id="si-a12-slice-1"></a>
### SI-A12 — Slice 1: introduced

The fixture bundle establishes the checked transcript shape, action grouping,
and required and forbidden API demand. Salsa and semantic-operation evidence
is deliberately marked unavailable.

<a id="si-a12-slice-6"></a>
### SI-A12 — Slice 6: established

Tests establish a deterministic checked projection of the complete dynamic
trace: only explicitly unordered siblings are canonically sorted, raw order
and debug keys remain failure artifacts, and closed dependency claims fail in
the presence of the unmapped fallback.

<a id="si-a13-slice-7"></a>
### SI-A13 — Slice 7: established

The revisions view and edit matrix distinguish input revisions from inspection
runs, retain exact input deltas, align comparable runs, and report observed
execution or reuse without inventing unrecorded causal invalidation edges.

<a id="si-a14-baseline"></a>
### SI-A14 — Baseline: established

The existing oracle already compares minimally adapted Sage and rustc outputs
by exact textual identity. Its tests remain an unchanged regression gate in
every inspector slice; inspector DTOs and reflection never become oracle
adapters.

<a id="si-a15-slice-3"></a>
### SI-A15 — Slice 3: introduced

The real service first constructs local-symbol product catalogs without
requesting the advertised products. External catalogs and the complete
focused-operation surface are not connected yet.

<a id="si-a15-slice-5"></a>
### SI-A15 — Slice 5: established

Representative local and external catalog snapshots establish server-authored
availability for all supported products and operations. Forbidden-demand
assertions prove that catalog construction reads no advertised product,
associated value, impl candidate, callee body, or unrelated metadata.

## Delivery slices

Each slice crosses the necessary workstreams above and ends in something a
reviewer can run and inspect. Checkboxes remain in the inventory so the scope
is not duplicated or accidentally narrowed here.

### Slice 1: Protocol and fixture-backed UI

A React/TypeScript application implements the complete mockup against the
reviewed protocol fixture bundle and a strict dummy server. There is no Axum,
Rust DTO, `rust-embed`, `cargo sage inspect` command, or Sage database in this
slice. The bundle is the server-provided dummy data; semantic values are never
hard-coded in view components.

Anchors established: [SI-A4](#si-a4-slice-1) and
[SI-A5](#si-a5-slice-1).

Anchors introduced: [SI-A3](#si-a3-slice-1),
[SI-A6](#si-a6-slice-1), [SI-A8](#si-a8-slice-1),
[SI-A9](#si-a9-slice-1), [SI-A10](#si-a10-slice-1), and
[SI-A12](#si-a12-slice-1).

Acceptance evidence:

- browser panels are assembled only from protocol requests and responses;
- the complete fixture symbol index is fetched once, then searched, filtered,
  and disclosed without further requests;
- catalogs drive tabs, unavailability, and actions without client guessing;
- local and external fixture links, products, focused operations, detailed
  structures, resizing, and grow/restore interactions match the mockup;
- every change of semantic view updates the URL, and direct load, reload,
  Back, and Forward restore it;
- the strict dummy server rejects an unlisted request and records exact
  required and forbidden demand; and
- browser snapshots and action-grouped demand transcripts use the reviewed
  fixture bytes.

### Slice 2: Axum transport and embedded application

Typed Rust protocol DTOs and a reusable inspection-service boundary sit below
an Axum loopback server. `cargo sage inspect` serves the Vite-built application
through `rust-embed` and opens a session URL. The service returns independent
typed scripted values; it does not yet construct a live Sage database.

Anchors established: [SI-A2](#si-a2-slice-2) and
[SI-A3](#si-a3-slice-2). Every Slice 1 green anchor remains a regression gate.

Acceptance evidence:

- Snapbox compares actual Axum status, contractual headers, JSON bytes, and
  provider demand exactly with the reviewed bundle;
- test inputs are typed scripted Rust values independent of expected JSON;
- handler tests prove the transport layer delegates semantics to the typed
  service;
- embedded production assets and direct routed loads work through Axum;
- terminal logs group API and provider demand under the browser action which
  caused it; and
- one black-box smoke test launches the real command on a random loopback port
  and snapshots the visible result and server-owned demand together.

### Slice 3: Real workspace symbols

The service constructs one live, read-only `InspectionHost` for the selected
target. The session and complete detail-free local symbol index use real Sage
data, and absolute-path selection resolves to those same retained handles.
Detail products remain explicitly unavailable.

Anchor established: [SI-A9](#si-a9-slice-3).

Anchors introduced or extended: [SI-A1](#si-a1-slice-3),
[SI-A8](#si-a8-slice-1), and [SI-A15](#si-a15-slice-3).

Acceptance evidence:

- index construction requests only local module and kind-specific membership;
- expanding, filtering, and searching the returned tree perform no request;
- signatures, field types, bodies, associated values, impl candidates, and
  external metadata are forbidden while constructing the index and catalogs;
- generated local symbols and provenance appear where represented;
- dependency symbols are absent from the workspace tree and search count;
- absolute paths select the same identities as tree rows; and
- repeated requests use the same live host.

### Slice 4: Real selected-symbol products

Selecting `DbDropGuard::db` shows its real identity, source, Concrete IR,
signature, diagnostics, and completed Typed IR. Each product is requested
independently and rendered from faithful bounded structural reflection.
Source-position selection resolves through the same `SelectedItem` boundary.

Anchor established: [SI-A6](#si-a6-slice-4).

Anchors introduced or extended: [SI-A7](#si-a7-slice-4),
[SI-A8](#si-a8-slice-1), and [SI-A15](#si-a15-slice-3).

Acceptance evidence:

- opening each tab requests only its documented product, with Diagnostics
  reusing the body product where designed;
- structural snapshots expose actual wrappers, fields, variants, and every
  embedded `Ty` tree;
- source-position and tree/path selection identify the same symbol;
- client expansion and formatting perform no semantic work;
- availability, incompleteness, truncation, and diagnostics remain explicit;
  and
- the exact oracle comparison remains independent and unchanged.

### Slice 5: Navigation, metadata, and focused operations

Every reflected symbol reference is an opaque clickable navigation target. A
reviewer can move between local symbols and metadata-backed external symbols,
navigate their parent and children, and request each product independently.
The service also exposes relevant impl candidates, `Prove(P) -> Proven`, and
input-only `Normalize(alias) -> Type` through typed operation handles.

Anchors established: [SI-A8](#si-a8-slice-5),
[SI-A10](#si-a10-slice-5), and [SI-A15](#si-a15-slice-5).

Acceptance evidence:

- links round-trip retained identity, and changing a display label does not
  alter selection;
- external symbols remain absent from the workspace tree;
- requesting one external product does not read the others;
- local and external catalogs request none of the products they advertise;
- unavailable and incomplete products remain explicit;
- proof exposes no selected impl and normalization has no expected-type input;
- focused requests use retained typed handles rather than display strings; and
- candidate completeness, ambiguity, and overflow are visible.

### Slice 6: Salsa event chain and execution tree

Every inspection shows a complete dynamic request tree, including Salsa
requests and execution/reuse disposition, Sage semantic operations, solver
work, and external metadata reads.

Anchors established: [SI-A7](#si-a7-slice-6),
[SI-A11](#si-a11-slice-6), and [SI-A12](#si-a12-slice-6).

Acceptance evidence:

- no promised request is omitted, including already-verified memo fetches;
- unmapped work uses the stable fallback and retains raw diagnostic payload;
- earlier navigation transcripts gain a correlated Salsa/Sage subtree without
  changing their browser-action and API-demand structure;
- checked transcripts sort only explicitly unordered siblings and retain raw
  ordering and debug keys as failure artifacts;
- required and forbidden dependency assertions survive sibling scheduling
  changes; and
- reflection is recorded before the run freezes, while serialization and
  browser rendering occur afterward and add no semantic work.

### Slice 7: Live updates and revision history

The host watches represented source files, applies edits to the same database,
notifies the browser, refreshes visible demand, and compares new results and
traces with earlier revisions. A revisions view shows exact input changes and
all inspection runs retained for each Salsa revision.

Anchors established: [SI-A1](#si-a1-slice-7) and
[SI-A13](#si-a13-slice-7).

Acceptance evidence:

- cold and unchanged-warm runs use one host;
- relevant and unrelated edits show intended execution/reuse boundaries;
- older browser products are marked stale;
- input updates and full workspace reloads are distinguished honestly;
- visible membership refresh requests the complete symbol index, never stale
  lazy-branch resources;
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
