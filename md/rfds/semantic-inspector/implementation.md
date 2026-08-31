# Implementation plan and status

The operational Semantic Inspector and all seven delivery slices have landed,
but this RFD remains accepted while [SI-A8](./README.md#si-a8) is partial. Its
[destination design](./README.md#destination-design) defines the Semantic
Inspector without regard to implementation order. This page inventories the
delivered work and groups it into the reviewer-visible vertical slices used
during implementation.

The [Web Application Walkthrough](./web-application.md) refines the browser,
[JSON Protocol](./protocol.md) pins the bytes at its central boundary. These
documents do not add independent slices; their work lands through the seven
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
  options, pointers, sharing, cycles, and explicit truncation.
- [x] Make derived structural reflection preserve every struct field and enum
  payload while allowing only the documented semantic treatment of
  `Stashed<T>`, raw allocation identity, names, symbols, spans, and explicit
  truncation ([SI-A6](./README.md#si-a6), [SI-A16](./README.md#si-a16)).
- [x] Implement the custom `Reflect` derive for ordinary structs and enums and
  explicit implementations for symbols, spans, stashed values, sharing,
  cycles, and limits. Product code must not hand-serialize Sage structures
  ([SI-A16](./README.md#si-a16)).
- [x] Represent semantic references with canonical symbol paths, separate
  display labels, and generic presentation data.
- [x] Record structural reflection as a distinct server phase, bound it by
  depth and node count, then freeze the observation before serialization or
  client-side expansion ([SI-A7](./README.md#si-a7)).
- [x] Keep service and observation types independent of Axum, JavaScript,
  terminal state, Clap, JSON-RPC, LSP positions, and the oracle schema.
- [x] Test derived structure, semantic leaf overrides, canonical cross-links,
  truncation, bounded traversal of wide sequences, same- and cross-stash
  shared values, and cycles.
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
  before slice 6.
- [x] Make deeply nested structures and execution trees independently
  collapsible, scrollable, resizable where appropriate, and growable to the
  full viewport.
- [x] Add service tests, Axum/JSON integration tests, Vitest/React Testing
  Library component tests, and a small real-process deployment/API suite.
- [x] Create one reviewed API fixture bundle containing a Rust source fixture,
  exact route manifest, request and response bytes, expected demand, and
  navigation scenarios ([SI-A3](./README.md#si-a3)).
- [x] In slice 2, construct independent typed scripted Rust values and use
  Snapbox to compare their actual Axum bytes and provider demand with the
  bundle. Never deserialize an expected response as the value under test.
- [x] Run frontend tests against a strict static server for that same bundle;
  reject unknown routes, record required and forbidden demand, and render an
  invented symbol kind and product ID without a TypeScript case.
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
- ✅ means the anchor is established and becomes a regression gate for every
  later slice.

Every symbol is a link. A stop sign links to the anchor's destination rule, a
yellow circle links to the transition which introduced it, and a checkmark
links to the transition and evidence which established it.

| Anchor | Baseline | [S1 UI](#slice-1-protocol-and-fixture-backed-ui) | [S2 Axum](#slice-2-axum-transport-and-embedded-application) | [S3 Symbols](#slice-3-real-workspace-symbols) | [S4 Products](#slice-4-real-selected-symbol-products) | [S5 Navigation](#slice-5-navigation-and-metadata) | [S6 Tracing](#slice-6-salsa-event-chain-and-execution-tree) | [S7 Revisions](#slice-7-live-updates-and-revision-history) |
|---|:---:|:---:|:---:|:---:|:---:|:---:|:---:|:---:|
| [SI-A1](./README.md#si-a1) database actor | [🛑](./README.md#si-a1 "Not introduced") | [🛑](./README.md#si-a1 "Not introduced") | [🛑](./README.md#si-a1 "Not introduced") | [🟡](#si-a1-slice-3 "Introduced") | [🟡](#si-a1-slice-3 "Introduced") | [🟡](#si-a1-slice-3 "Introduced") | [🟡](#si-a1-slice-3 "Introduced") | [✅](#si-a1-slice-7 "Established") |
| [SI-A2](./README.md#si-a2) typed service below Axum | [🛑](./README.md#si-a2 "Not introduced") | [🛑](./README.md#si-a2 "Not introduced") | [✅](#si-a2-slice-2 "Established") | [✅](#si-a2-slice-2 "Established") | [✅](#si-a2-slice-2 "Established") | [✅](#si-a2-slice-2 "Established") | [✅](#si-a2-slice-2 "Established") | [✅](#si-a2-slice-2 "Established") |
| [SI-A3](./README.md#si-a3) exact shared JSON | [🛑](./README.md#si-a3 "Not introduced") | [🟡](#si-a3-slice-1 "Introduced") | [✅](#si-a3-slice-2 "Established") | [✅](#si-a3-slice-2 "Established") | [✅](#si-a3-slice-2 "Established") | [✅](#si-a3-slice-2 "Established") | [✅](#si-a3-slice-2 "Established") | [✅](#si-a3-slice-2 "Established") |
| [SI-A4](./README.md#si-a4) generic frontend | [🛑](./README.md#si-a4 "Not introduced") | [✅](#si-a4-slice-1 "Established") | [✅](#si-a4-slice-1 "Established") | [✅](#si-a4-slice-1 "Established") | [✅](#si-a4-slice-1 "Established") | [✅](#si-a4-slice-1 "Established") | [✅](#si-a4-slice-1 "Established") | [✅](#si-a4-slice-1 "Established") |
| [SI-A5](./README.md#si-a5) URL intent and reset | [🛑](./README.md#si-a5 "Not introduced") | [✅](#si-a5-slice-1 "Established") | [✅](#si-a5-slice-1 "Established") | [✅](#si-a5-slice-1 "Established") | [✅](#si-a5-slice-1 "Established") | [✅](#si-a5-slice-1 "Established") | [✅](#si-a5-slice-1 "Established") | [✅](#si-a5-slice-1 "Established") |
| [SI-A6](./README.md#si-a6) faithful bounded reflection | [🛑](./README.md#si-a6 "Not introduced") | [🟡](#si-a6-slice-1 "Introduced") | [🟡](#si-a6-slice-1 "Introduced") | [🟡](#si-a6-slice-1 "Introduced") | [✅](#si-a6-slice-4 "Established") | [✅](#si-a6-slice-4 "Established") | [✅](#si-a6-slice-4 "Established") | [✅](#si-a6-slice-4 "Established") |
| [SI-A7](./README.md#si-a7) semantic work before rendering | [🛑](./README.md#si-a7 "Not introduced") | [🛑](./README.md#si-a7 "Not introduced") | [🛑](./README.md#si-a7 "Not introduced") | [🛑](./README.md#si-a7 "Not introduced") | [🟡](#si-a7-slice-4 "Introduced") | [🟡](#si-a7-slice-4 "Introduced") | [✅](#si-a7-slice-6 "Established") | [✅](#si-a7-slice-6 "Established") |
| [SI-A8](./README.md#si-a8) canonical symbol paths | [🛑](./README.md#si-a8 "Not introduced") | [🟡](#si-a8-slice-1 "Introduced") | [🟡](#si-a8-slice-1 "Introduced") | [🟡](#si-a8-slice-1 "Introduced") | [🟡](#si-a8-slice-1 "Introduced") | [🟡](#si-a8-slice-5 "Partially implemented") | [🟡](#si-a8-slice-5 "Partially implemented") | [🟡](#si-a8-slice-5 "Partially implemented") |
| [SI-A9](./README.md#si-a9) eager detail-free index | [🛑](./README.md#si-a9 "Not introduced") | [🟡](#si-a9-slice-1 "Introduced") | [🟡](#si-a9-slice-1 "Introduced") | [✅](#si-a9-slice-3 "Established") | [✅](#si-a9-slice-3 "Established") | [✅](#si-a9-slice-3 "Established") | [✅](#si-a9-slice-3 "Established") | [✅](#si-a9-slice-3 "Established") |
| [SI-A11](./README.md#si-a11) complete demand observation | [🛑](./README.md#si-a11 "Not introduced") | [🛑](./README.md#si-a11 "Not introduced") | [🛑](./README.md#si-a11 "Not introduced") | [🛑](./README.md#si-a11 "Not introduced") | [🛑](./README.md#si-a11 "Not introduced") | [🛑](./README.md#si-a11 "Not introduced") | [✅](#si-a11-slice-6 "Established") | [✅](#si-a11-slice-6 "Established") |
| [SI-A12](./README.md#si-a12) stable evidence projection | [🛑](./README.md#si-a12 "Not introduced") | [🟡](#si-a12-slice-1 "Introduced") | [🟡](#si-a12-slice-1 "Introduced") | [🟡](#si-a12-slice-1 "Introduced") | [🟡](#si-a12-slice-1 "Introduced") | [🟡](#si-a12-slice-1 "Introduced") | [✅](#si-a12-slice-6 "Established") | [✅](#si-a12-slice-6 "Established") |
| [SI-A13](./README.md#si-a13) revisions versus runs | [🛑](./README.md#si-a13 "Not introduced") | [🛑](./README.md#si-a13 "Not introduced") | [🛑](./README.md#si-a13 "Not introduced") | [🛑](./README.md#si-a13 "Not introduced") | [🛑](./README.md#si-a13 "Not introduced") | [🛑](./README.md#si-a13 "Not introduced") | [🛑](./README.md#si-a13 "Not introduced") | [✅](#si-a13-slice-7 "Established") |
| [SI-A14](./README.md#si-a14) oracle independence | [✅](#si-a14-baseline "Established") | [✅](#si-a14-baseline "Established") | [✅](#si-a14-baseline "Established") | [✅](#si-a14-baseline "Established") | [✅](#si-a14-baseline "Established") | [✅](#si-a14-baseline "Established") | [✅](#si-a14-baseline "Established") | [✅](#si-a14-baseline "Established") |
| [SI-A15](./README.md#si-a15) positive non-eager products | [🛑](./README.md#si-a15 "Not introduced") | [🛑](./README.md#si-a15 "Not introduced") | [🛑](./README.md#si-a15 "Not introduced") | [🟡](#si-a15-slice-3 "Introduced") | [🟡](#si-a15-slice-3 "Introduced") | [✅](#si-a15-slice-5 "Established") | [✅](#si-a15-slice-5 "Established") | [✅](#si-a15-slice-5 "Established") |
| [SI-A16](./README.md#si-a16) derive-driven reflection | [🛑](./README.md#si-a16 "Not introduced") | [🛑](./README.md#si-a16 "Not introduced") | [🟡](#si-a16-slice-2 "Introduced") | [🟡](#si-a16-slice-2 "Introduced") | [✅](#si-a16-slice-4 "Established") | [✅](#si-a16-slice-4 "Established") | [✅](#si-a16-slice-4 "Established") | [✅](#si-a16-slice-4 "Established") |
| [SI-A17](./README.md#si-a17) Salsa invocation spans | [🛑](./README.md#si-a17 "Not introduced") | [🛑](./README.md#si-a17 "Not introduced") | [🛑](./README.md#si-a17 "Not introduced") | [🛑](./README.md#si-a17 "Not introduced") | [🛑](./README.md#si-a17 "Not introduced") | [🛑](./README.md#si-a17 "Not introduced") | [✅](#si-a17-slice-6 "Established") | [✅](#si-a17-slice-6 "Established") |

A slice is complete only when every anchor which turns green in its column has
the evidence named below and every already-green anchor remains green.

## Anchor transitions

These transitions explain what each yellow or green cell means. Repeated
yellow cells link to the original introduction; repeated green cells link to
the evidence which first established the anchor.

<a id="si-a1-slice-3"></a>
### SI-A1 — Slice 3: introduced

The real workspace-symbol service first constructs a `DatabaseActor` which
owns `InspectionHost` and keeps one Sage database alive across typed client
messages. Handler tests prohibit direct Axum database access. The anchor
remains yellow because this slice does not yet watch files or prove
same-database edit reuse.

<a id="si-a1-slice-7"></a>
### SI-A1 — Slice 7: established

Cold, unchanged-warm, relevant-edit, and unrelated-edit tests run against one
host. Evidence distinguishes an ordinary input update from a workspace reload
and proves that coherent reads never observe a partially applied edit batch.

<a id="si-a2-slice-2"></a>
### SI-A2 — Slice 2: established

Typed Rust service requests and results exist below Axum. Handler tests prove
that Axum only parses protocol DTOs, invokes the client boundary, maps
transport status, and serializes the result; no database access, semantic
selection, or rendering lives in the handler.

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

Browser tests prove that the directory, positive product lists, generic render
trees, traces, and revisions alone determine behavior. An invented symbol kind
and product ID render without a TypeScript case; omission removes a tab and
request; local and external symbols use the same components.

<a id="si-a5-slice-1"></a>
### SI-A5 — Slice 1: established

Every semantic selection, product, and revision view has a canonical URL.
Direct load, Back, and Forward tests establish push versus replace. Revision
mismatch tests discard every response-derived value, bootstrap a new directory,
replay the URL, and fall back explicitly if its symbol path no longer resolves.
An event-stream reconnect test proves that the browser checks the current
revision and detects an update missed while disconnected.

<a id="si-a6-slice-1"></a>
### SI-A6 — Slice 1: introduced

Generic browser interpreters handle the complete `RenderNode` and `ValueNode`
vocabularies, including references, sharing, cycles, and truncation. Fixture
tests establish the interaction model, but cannot yet prove faithful
reflection of real Rust structures.

<a id="si-a6-slice-4"></a>
### SI-A6 — Slice 4: established

Derived Rust reflection snapshots preserve every field and enum payload for
representative concrete IR, signature, and Typed IR values. The generated
enum match is compile-time exhaustive (including `Ty`), while focused tests
exercise semantic leaves, sharing, cycles, and limits.

<a id="si-a7-slice-4"></a>
### SI-A7 — Slice 4: introduced

Real product requests separate semantic analysis from bounded structural
reflection and freeze an owned observation before serialization. Complete
phase attribution is not yet observable without the tracing work.

<a id="si-a7-slice-6"></a>
### SI-A7 — Slice 6: established

Execution evidence identifies selection, requested analysis, structural
reflection, pure render-tree assembly, and post-trace serialization as separate
phases. Tests prove that every semantic read used by reflection is inside the
recorded boundary and that JSON and browser interpretation add none.

<a id="si-a8-slice-1"></a>
### SI-A8 — Slice 1: introduced

Fixture semantic references carry canonical paths separately from display
labels, and all browser navigation uses those paths. Real Sage identity and
external metadata navigation are not connected yet.

<a id="si-a8-slice-5"></a>
### SI-A8 — Slice 5: partially implemented

Ordinary local and named external references round-trip through backend
ownership traversal. Tests cover path stability across unrelated edits,
sibling reordering and host reconstruction, namespaces, duplicate external
crates, label changes, and explicit rename/move/delete failure. Direct replay
through an anonymous external impl owner is not implemented because doing so
would require a broader external-definition lookup than the current metadata
boundary. Invalid duplicate local definitions also use encounter-order
recovery suffixes, so their paths are not stable under reordering. SI-A8
therefore remains yellow.

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
external metadata beyond the explicitly permitted macro-resolution and
expansion families.

<a id="si-a11-slice-6"></a>
### SI-A11 — Slice 6: established

Balanced lifecycle events cover requests, returns, execution, validated memo
reuse, and already-verified cache hits. A stable fallback category prevents
unknown work from being silently omitted and therefore prevents a false claim
of complete dependency observation.

<a id="si-a12-slice-1"></a>
### SI-A12 — Slice 1: introduced

The fixture bundle establishes the checked transcript shape, action grouping,
and required and forbidden API demand. It makes no Salsa or semantic-operation
evidence claim yet.

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
requesting the advertised products. External product lists are not connected
yet.

<a id="si-a15-slice-5"></a>
### SI-A15 — Slice 5: established

Representative local and external snapshots establish positive server-authored
lists of opaque IDs, labels, and URLs. Forbidden-demand assertions prove that
list construction reads no advertised product, associated value, impl
candidate, callee body, or unrelated metadata.

<a id="si-a16-slice-2"></a>
### SI-A16 — Slice 2: introduced

The custom derive and reflection context operate on scripted Rust DTO fixtures.
They recursively expose ordinary fields and variants and support explicit
semantic overrides, but no real Sage IR coverage exists yet.

<a id="si-a16-slice-4"></a>
### SI-A16 — Slice 4: established

Real Concrete IR, signatures, types, and Typed IR use derived reflection.
Mutation and coverage tests prove fields and variants appear automatically;
custom symbol, span, stash, sharing, cycle, and limit implementations are
explicit; and product producers perform no hand serialization.

<a id="si-a17-slice-6"></a>
### SI-A17 — Slice 6: established

The temporary Salsa fork emits one balanced span per tracked-query invocation
before memo lookup. Tests cover execution, validation, already-current reuse,
nested parentage, termination paths, no per-query Sage annotations, stable
ingredient projection, and the unmapped fallback.

## Delivery slices

Each slice crosses the necessary workstreams above and ends in something a
reviewer can run and inspect. Checkboxes remain in the inventory so the scope
is not duplicated or accidentally narrowed here.

### Slice 1: Protocol and fixture-backed UI

A React/TypeScript application implements the complete mockup against the
reviewed protocol fixture bundle and a strict dummy server. There is no Axum,
Rust DTO, `rust-embed`, `cargo sage inspect` command, or Sage database in this
slice. The bundle is the server-provided dummy data. The application is a
generic interpreter for the symbol directory, positive product descriptors,
`RenderNode`/`ValueNode` trees, and revision state; semantic values and product
meanings are never hard-coded in view components.

Anchors established: [SI-A4](#si-a4-slice-1) and
[SI-A5](#si-a5-slice-1).

Anchors introduced: [SI-A3](#si-a3-slice-1),
[SI-A6](#si-a6-slice-1), [SI-A8](#si-a8-slice-1),
[SI-A9](#si-a9-slice-1), and [SI-A12](#si-a12-slice-1).

Acceptance evidence:

- browser panels are assembled only from protocol requests and responses;
- the complete fixture symbol index is fetched once, then searched, filtered,
  and disclosed without further requests;
- positive product descriptors drive tabs without client guessing, and an
  invented symbol presentation and product identifier render without a new
  TypeScript case;
- local and external fixture links, products, detailed structures, resizing,
  and grow/restore interactions match the mockup;
- every change of semantic view updates the URL, and direct load, reload,
  Back, and Forward restore it by canonical symbol path and product ID;
- a revision mismatch discards all response-derived state, bootstraps the
  current revision and full directory, and replays the URL's durable intent;
- the strict dummy server rejects an unlisted request and records exact
  required and forbidden demand; and
- browser snapshots and action-grouped demand transcripts use the reviewed
  fixture bytes.

### Slice 2: Axum transport and embedded application

Typed Rust protocol DTOs and a reusable inspection-service boundary sit below
an Axum loopback server. `cargo sage inspect` serves the Vite-built application
through `rust-embed` and opens the application URL. The service returns
independent typed scripted values; it does not yet construct a live Sage
database.

Anchors established: [SI-A2](#si-a2-slice-2) and
[SI-A3](#si-a3-slice-2). Every Slice 1 green anchor remains a regression gate.

Anchor introduced: [SI-A16](#si-a16-slice-2).

Acceptance evidence:

- Snapbox compares actual Axum status, contractual headers, JSON bytes, and
  provider demand exactly with the reviewed bundle;
- test inputs are typed scripted Rust values independent of expected JSON;
- handler tests prove the transport layer delegates semantics to the typed
  service;
- embedded production assets and direct routed loads work through Axum;
- terminal logs group API and provider demand under the browser action which
  caused it; and
- one black-box smoke test launches the real command with port `0`
  and snapshots the visible result and server-owned demand together.

### Slice 3: Real workspace symbols

The service constructs one `DatabaseActor` which owns the live
`InspectionHost` for the selected target. Axum reaches it only through
`InspectionClient`. The session and complete detail-free local symbol index use
real Sage data. The browser searches that directory locally and selects its
canonical paths. The catalogs list no detail products until those real
products are connected.

Anchor established: [SI-A9](#si-a9-slice-3).

Anchors introduced or extended: [SI-A1](#si-a1-slice-3),
[SI-A8](#si-a8-slice-1), and [SI-A15](#si-a15-slice-3).

Acceptance evidence:

- index construction snapshots its closed set of local module and
  kind-specific membership operation families, plus only the external metadata
  dynamically required to resolve and expand macros whose output contributes
  local membership;
- expanding, filtering, and searching the returned tree perform no request;
- signatures, field types, bodies, associated values, impl candidates, and
  semantic external metadata are forbidden while constructing the index and
  catalogs; every permitted external metadata read is nested under macro
  resolution or expansion;
- generated local symbols and provenance appear where represented;
- dependency symbols are absent from the workspace tree and search count;
- search sends no user text to the backend and selects the chosen row's
  canonical path; and
- repeated client messages use the same actor-owned live host.

### Slice 4: Real selected-symbol products

Selecting `DbDropGuard::db` shows its real identity, source, Concrete IR,
signature, diagnostics, and completed Typed IR. Each product is requested
independently and rendered from faithful bounded structural reflection.

Anchors established: [SI-A6](#si-a6-slice-4) and
[SI-A16](#si-a16-slice-4).

Anchors introduced or extended: [SI-A7](#si-a7-slice-4),
[SI-A8](#si-a8-slice-1), and [SI-A15](#si-a15-slice-3).

Acceptance evidence:

- opening each tab requests only its documented product, with Diagnostics
  reusing the body product where designed;
- structural snapshots expose actual wrappers, fields, variants, and every
  embedded `Ty` tree;
- reflection coverage and mutation tests prove ordinary derived fields and
  variants appear automatically, while custom symbol cross-links and bounded
  sharing, cycle, truncation, and continuation-exhaustion behavior remain
  explicit;
- client expansion and formatting perform no semantic work;
- incompleteness, truncation, and diagnostics remain explicit within returned
  products;
  and
- the exact oracle comparison remains independent and unchanged.

### Slice 5: Navigation and metadata

Every reflected symbol reference carries a backend-authored canonical path. A
reviewer can move between local symbols and metadata-backed external symbols,
navigate their parent and children, and request each listed product
independently. The frontend treats paths and product identifiers as opaque;
ownership traversal and positive product enumeration remain server
responsibilities.

Anchor partially implemented: [SI-A8](#si-a8-slice-5). Anchor established:
[SI-A15](#si-a15-slice-5).

Acceptance evidence:

- canonical paths distinguish namespaces, unnamed impls, generated symbols,
  and duplicate external crate instances;
- paths survive unrelated edits, declaration reordering, and database
  reconstruction, while rename, move, and deletion fail explicitly;
- changing a display label does not alter selection or navigation;
- external symbols remain absent from the workspace tree;
- requesting one external product does not read the others;
- local and external catalogs request none of the products they advertise;
- absent product pages are omitted, while incompleteness inside listed products
  remains explicit; and
- stale or forged paths and product identifiers return structured not-found
  errors without causing semantic guessing in the client.

### Slice 6: Salsa event chain and execution tree

Every inspection shows a complete dynamic request tree, including Salsa
requests and execution/reuse disposition, Sage semantic operations, solver
work, and external metadata reads.

Anchors established: [SI-A7](#si-a7-slice-6),
[SI-A11](#si-a11-slice-6), [SI-A12](#si-a12-slice-6), and
[SI-A17](#si-a17-slice-6).

Acceptance evidence:

- no promised request is omitted, including already-verified memo fetches;
- the temporary Salsa fork emits one balanced span for every tracked-query
  invocation before memo lookup, covering execution, validation,
  already-current reuse, nested parentage, pre-fetch revision cancellation, and
  every termination path without per-query Sage annotations; ordinary fetch
  and accumulator graph traversal share that traced refresh path;
- unmapped work uses the stable fallback and retains raw diagnostic payload;
- earlier navigation transcripts gain a correlated Salsa/Sage subtree without
  changing their browser-action and API-demand structure;
- checked transcripts sort only explicitly unordered siblings and retain raw
  ordering and debug keys as failure artifacts;
- a focused projection test preserves sequential solver-child order while
  canonicalizing explicitly unordered trait and normalization candidate
  groups;
- required and forbidden dependency assertions survive sibling scheduling
  changes; and
- reflection is recorded before the run freezes, while serialization and
  browser rendering occur afterward and add no semantic work.

### Slice 7: Live updates and revision history

The host watches represented source files, applies edits to the same database,
notifies the browser, reloads the current semantic URL at the new process-wide
revision, discards all response-derived state, bootstraps a fresh directory,
replays the current semantic URL, and compares new results and traces with
earlier revisions. After host reconstruction, the actor supplies the new
authoritative Cargo workspace, selected package, and target source roots. The
watcher replaces its non-recursive Cargo/toolchain and recursive source-tree
watch set, including workspace/package `.cargo/config{,.toml}` directories.
This remains correct when the command starts below the workspace root or
`[lib].path` is nested below the package root. A revisions view shows exact
input changes and all inspection runs retained for each Salsa revision.

Live startup requires exactly one selected library target and preserves Cargo's
distinct package and target names in the session. Reload constructs a fallible
replacement before swapping hosts; a transient invalid manifest leaves the
current database, revision, and watches intact and returns a structured error.
The replacement becomes eligible for the swap only after its rustc metadata
provider reaches `after_expansion` without diagnostics and reports readiness.
The watcher likewise configures its initial roots before command readiness is
published. Expected command startup failures are propagated; an occupied port
recommends `--port`, and browser-launch failure is non-fatal.
Replacement watches are installed before obsolete watches are removed. A
failed replacement installation retains the prior complete set and retries
while reporting a degraded-watcher event; failed cleanup retains the harmless
extra old watch rather than losing the new root.

Anchors established: [SI-A1](#si-a1-slice-7) and
[SI-A13](#si-a13-slice-7).

Acceptance evidence:

- cold and unchanged-warm runs use one host;
- relevant and unrelated edits show intended execution/reuse boundaries;
- every response is tagged with a revision ID and a mismatch forces a complete
  client reset and bootstrap before canonical path resolution;
- input updates and full workspace reloads are distinguished honestly;
- ambiguous multi-target startup is rejected, while explicit package selection
  and distinct package/target names are preserved;
- the selected-package build ignores broken unrelated workspace members and
  retains a normal workspace-member dependency under its Cargo extern name;
- failed reconstruction preserves the prior live host and a subsequent valid
  manifest recovers;
- an explicitly corrupted metadata-provider invocation fails its readiness
  handshake and preserves the prior host, while valid keyword extern aliases
  work and concurrent hosts own distinct temporary stubs;
- initial watcher-configuration failure prevents command readiness, while an
  occupied inspector port exits with a useful CLI error rather than panicking;
- manifest-driven source-root changes replace the watch set from authoritative
  workspace/package/source roots before subsequent source edits are observed;
- a fake watcher verifies that reconfiguration actually removes the old
  recursive source root and installs the new one, and Cargo/toolchain path
  classification covers both toolchain filenames and workspace/package Cargo
  config files;
- injected replacement-installation and obsolete-unwatch failures prove that
  reconfiguration never leaves the reconstructed source root uncovered;
- bootstrap requests the complete symbol index and current product, never
  hidden products or stale lazy-branch resources;
- a revision with no inspection request reports no semantic work; and
- comparisons do not infer causal dependency edges which were not recorded.

## Completion

- [ ] Every item in the complete work inventory is implemented or explicitly
  removed by an accepted design revision.
- [ ] Every delivery slice meets its acceptance evidence.
- [ ] All end-state acceptance tests in the RFD pass.
- [x] Repository-required full validation passes.
- [x] The destination and partial built status are reflected in architecture
  documentation and the build-out roadmap.
- [x] An LSP server is not required for completion.
