# RFD: Semantic Inspector

**Status:** Draft

**Depends on:**

- [Architecture](../../design/architecture.md) — the Salsa database, Cargo
  target selection, and rustc metadata boundary
- [Typed IR](../../design/typed-ir.md) — the semantic body the inspector
  presents
- [Resolve at Position](../resolve-at-position/README.md) — source-position
  selection

**Related:**

- [Oracle Test Harness](../../design/oracle-test-harness.md) — independent
  exact conformance testing
- [Trait Impl Candidate Discovery](../trait-impl-candidate-discovery/README.md)
  — a motivating consumer of edit-and-query dependency tests

**Detailed sub-RFD:**

- [Web Application Walkthrough](./web-application.md) — follows each visible
  part of the mockup through its JavaScript assembly, JSON request, inspection
  service operation, and Sage/Salsa query

## TL;DR

- Add a reusable semantic-inspection service, an Axum backend connected to a
  live Sage database, and a JavaScript browser client opened by
  `cargo sage inspect`.
- Browse the selected target's local symbol tree and inspect source concrete
  syntax, expanded concrete IR, signatures, Typed IR, and other supported
  symbol-keyed results on demand.
- Preserve semantic references in the reflected result so definitions named by
  signatures, types, and Typed IR are clickable navigation targets.
- Keep external symbols out of the workspace tree. They remain reachable from
  semantic references and get a metadata-only view with navigable parents and
  children.
- Record the dynamic query tree, distinguish execution from reuse, retain the
  database across source edits, and support cold/warm and edit-invalidation
  experiments.
- Retain Salsa revision records which separate input edits from the inspection
  runs actually demanded in that revision, and make repeated work comparable
  across edits.
- Keep inspection separate from the exact rustc oracle. The inspector helps a
  human understand Sage; it does not weaken or replace conformance checks.
- Put the reusable service below the web client and a future LSP adapter.

## Motivation

Sage's architecture is demand-driven, but its behavior is difficult to review
without reading implementation code. A reviewer currently has several partial
tools:

- oracle JSON shows a precise shared conformance value, but is intentionally
  optimized for exact comparison rather than human reading;
- unit tests can call individual queries, but require Rust code and knowledge
  of internal symbol handles;
- `Database::take_query_log()` exposes useful execution evidence, but mixes
  raw Salsa debug keys with handwritten metadata strings; and
- the current `cargo sage` command can print expanded module items, but cannot
  inspect a selected semantic item or retain a workspace across edits.

This makes important architectural claims unnecessarily expensive to check.
For example, a reviewer should be able to find
`crate::parse::Parse::next`, inspect its concrete and elaborated bodies, follow
its `Iterator::Item` and method references into dependency metadata, and
inspect the checked signatures involved without reading Sage's implementation.
The same session should show which queries and metadata reads produced the
result, then permit a source edit and show what executed or was reused.

The tool is both an interactive debugger and a test facility. Semantic results
and incremental-computation evidence are observations of Sage; neither becomes
part of Sage's language semantics.

## Change in a nutshell

The command creates a live Sage database for one selected Cargo target and
starts an Axum loopback server:

```text
$ cargo sage inspect --package mini-redis
Inspecting mini-redis (lib) at http://127.0.0.1:<port>/
```

The web application calls a shared typed service:

```mermaid
flowchart TD
    Cargo[Cargo target and dependency world] --> Host[Inspection host]
    Files[Source files] --> Host
    Metadata[rustc metadata service] --> Host
    Host --> Analysis[Read-only analysis view]
    Analysis --> Select[Semantic selector]
    Select --> Inspect[Inspection operation]
    Inspect --> Observation[Owned structured observation]
    Observation --> Axum[Axum JSON API]
    Axum --> Web[JavaScript browser client]
    Observation --> Tests[Test assertions]
    Observation -. future .-> Lsp[LSP adapter]
```

The browser fetches symbol-tree branches and inspection products on demand. A
returned observation contains everything needed to render and expand that
product; ordinary client-side rendering must not execute additional semantic
queries.

### Interaction mockup

An [interactive browser mockup](./mockup.html) uses representative
`DbDropGuard::db` data to show the destination interaction: searching the local
symbol tree, inspecting concrete and typed results, following semantic links
to dependency metadata, and viewing the dynamic execution tree. The mockup is
not connected to Sage; controls for persistent edits and revision comparison
are not yet drawn.

<iframe
  src="./mockup.html"
  title="Semantic Inspector interaction mockup"
  allowfullscreen
  style="width: 100%; height: 900px; border: 1px solid #dbe1dc; border-radius: 8px; background: white;"
></iframe>

Use the [full-page mockup](./mockup.html) when the embedded viewport is too
narrow.

## Destination design

This section describes the complete inspector, independent of the order in
which it is implemented. [Implementation](./implementation.md) inventories all
work required for this destination and then groups it into reviewable slices.

### Persistent inspection host

The host owns one live Sage database, selected Cargo target, source inputs, and
reachable dependency metadata for as long as the Axum server is running. Axum
handlers perform multiple on-demand inspections against that database. The
host watches represented source files, applies ordinary source edits to the
existing database, and reports when a Cargo or dependency change instead
requires a workspace reload.

The service follows an owning-host/read-only-analysis split:

```text
InspectionHost
  analysis() -> Analysis

Analysis
  select(...)
  inspect(...)
```

The exact synchronization mechanism between Axum and the database is not
pinned by this RFD. Sage analysis is synchronous, potentially CPU-intensive
work; handlers must not hold an asynchronous executor thread or an unrelated
lock across an `.await` while analysis runs. An implementation may serialize
requests through a database-owning worker or dispatch analysis as blocking
work. This transport choice stays outside the typed inspection service.

Every response identifies the coherent database revision from which it was
produced, preventing separately fetched panels from appearing to be one atomic
observation when they are not. An existing `SourceFile` input is updated in
the same database for ordinary edits. Changes to `Cargo.toml`, the lockfile,
target selection, enabled features, build-script outputs, file-set membership,
or dependency artifacts may require an explicit reload. The tool must never
present a reload as fine-grained incremental reuse.

The rustc process supplies authoritative dependency metadata only. It does not
type-check local Sage bodies or solve Sage trait goals.

The complete browser-to-server update and history path, including the
distinction between a filesystem edit batch and the Salsa revisions produced
by its input setters, is defined by the [Web Application
Walkthrough](./web-application.md#8-what-happens-when-source-changes).

### Axum backend and on-demand API

`cargo sage inspect` constructs the inspection host and Axum application in the
same process. The server binds to loopback, serves the frontend's static assets,
and exposes a JSON API over the typed inspection service. It does not support a
remote bind or source mutation through HTTP.

The API is product-oriented rather than a serialized database dump:

- opening or filtering the root view requests local symbol summaries;
- expanding a module, trait, or impl requests that symbol's represented
  children;
- selecting a tab requests that symbol's concrete IR, signature, Typed IR, or
  other supported product;
- following a semantic reference requests the destination symbol and its
  availability; and
- expanding a reflected product already returned to the browser is local UI
  work, except for an explicit continuation when a bounded collection was
  truncated.

Responses use the same owned observation structures used by service tests,
with an HTTP envelope for revision, success, unavailable, incomplete, and
failure status. HTTP handlers do not implement semantic selection, reflection,
or pretty-printing themselves.

Navigation references cross the JSON boundary as opaque, session-scoped
handles for typed `NavigationTarget` values. The display path is a label, not
the lookup key. Sending a handle back to Axum recovers the retained local or
external identity without reparsing or ambiguously resolving its text.

Because the API can reveal source and dependency metadata, the server
accepts requests only from its loopback session and does not enable permissive
cross-origin access. The exact session-token and browser-launch mechanics are
implementation choices, but another web origin must not be able to read an
inspector session silently.

The concrete resources, response envelope, update coordinator, and retained
revision/run records are specified in the [Web Application
Walkthrough](./web-application.md).

### JavaScript frontend

The frontend is a JavaScript application served by Axum. It may use a small
framework or browser APIs directly; the RFD does not select a framework. If a
build step is used, Axum serves the resulting static assets and does not
require a separate development server in normal `cargo sage inspect` use.

The frontend owns presentation state such as the selected symbol, open tabs,
expanded tree nodes, filters, and back/forward navigation. It requests symbol
branches and semantic products only when the user asks to see them, caches
responses for their reported revision, and renders unavailable or incomplete
status explicitly. It contains no Sage semantic logic and never infers a
symbol identity from display text.

The [Web Application Walkthrough](./web-application.md) maps every mockup view
through its JavaScript and API request to the semantic query which supplies it,
then applies the same model to automatic refresh and revision comparison.

### Structural reflection

Inspection values are projected into an owned, renderer-neutral tree rather
than formatted independently by every panel. Records, enum variants,
sequences, scalar leaves, optional values, and bounded or truncated subtrees
have explicit representations. A derive handles ordinary structure; semantic
implementations give symbols, names, and spans stable readable forms and add
summaries to recursively reflected types.

The structural view is lossless with respect to the inspected Rust value. It
uses the actual struct field names and enum variant payloads and does not
replace them with a conceptual schema. `Option<T>`, `Ptr<T>`, `Slice<T>`, and
other wrappers remain visible nodes; their contained values are recursively
inspectable. A `Ptr<Ty>` therefore expands to the precise `Ty` variant and all
of that variant's fields, and a `Slice<Ptr<Ty>>` expands each element in the
same way. Raw addresses and process-specific allocation indexes are omitted;
shared pointees may instead carry a display-local identity so repetition and
cycles remain intelligible. Storage infrastructure is the other explicit
exception: `Stashed<T>` exposes its semantic `root`, but the backing `Stash`
and cached fingerprint are not part of the inspected language value and may be
omitted.

Semantic summaries are additive. For example, a `Ty::Adt` node may show
`DbDropGuard` beside the variant name and make its `Symbol` child navigable,
but it still exposes both tuple fields: the symbol and recursively reflected
type arguments. Source-like signature text is a separate presentation and
cannot stand in for structural fields. Adding a field to `FnSig` or a variant
to `Ty` must either make it appear through the derive or cause an explicit
reflection-coverage test to fail.

A semantic reference is not a string leaf. The reflected tree retains a
dedicated reference node carrying the actual navigation target and a separate
display label. Targets include local and external symbols; owner-relative
targets such as fields may extend the same family. The web
client must not parse a displayed path and resolve it again to implement a
link.

Reflection is bounded by depth and total node count, preserves truncation as an
explicit node, and cannot invoke additional semantic operations after an
observation is returned. The exact trait and enum names are implementation
choices.

### Semantic selectors

An inspection begins by resolving a typed selector, not by exposing a raw
Salsa key. Selector forms are:

- an absolute semantic item path such as
  `crate::parse::Parse::next`; and
- a source position expressed as a file plus line and column.

The semantic path form covers modules, free items, inherent associated items,
and trait associated items. If a written path does not uniquely identify one
item, selection returns a structured ambiguity with the stable paths and
kinds of the possible items. It does not choose according to source or hash
map order.

The browser supports selection from the local symbol tree, by absolute
semantic path, and by source position. Source-position selection follows the
Resolve at Position RFD. Conversion between byte offsets and LSP's UTF-16
positions belongs at the client boundary. All selector forms resolve to the
same internal `SelectedItem`; inspection operations do not care how it was
selected.

Stable display paths are user-facing identities. Internal Salsa IDs, arena
pointers, rustc `DefId` debug forms, and process-specific indexes must not be
required in commands or expected output.

### Symbol tree and semantic navigation

The main symbol tree contains represented local symbols in the selected Cargo
target, including generated local symbols with expansion provenance. Building
and expanding the tree may request module membership and associated-item
queries, but it must not type-check every body. External dependency symbols do
not appear in this tree and are not included in its search count.

Signatures, semantic types, concrete and Typed IR, predicates, ownership
relationships, and other inspected structures render every represented symbol
reference as a navigation link. Following a link selects the retained semantic
identity directly. Navigation provides ordinary back-to-previous and
back-to-selected-local-symbol actions so moving through a type or call graph
does not lose the review context.

Local and external identities have different views:

| Property | Local symbol | External symbol |
|---|---|---|
| Appears in workspace tree | yes | no |
| Stable identity and kind | yes | yes |
| Parent and represented children | yes | yes, from keyed metadata |
| Checked signature and predicates | when defined for the kind | when supplied by metadata |
| Source and concrete IR | when represented locally | unavailable |
| Completed Typed IR body | for supported local bodies | unavailable |

The external view states that it is metadata-backed and shows unavailable
products explicitly rather than presenting empty source or body panels. Its
parent and child edges are navigable even when those definitions have never
appeared in the local tree. Unsupported child kinds and incomplete metadata
are reported as such; they are not silently dropped from a view claiming
completeness.

An external page does not synthesize one all-purpose "external node" value.
`SymExt { crate_num, def_index, kind }` is the retained identity. Each visible
semantic product remains a separate, on-demand operation: for example,
`FnSymbol::sig`, `TraitSymbol::sig`, `TraitSymbol::items`, and
`SymExt::expanded_module_items`. Predicates are fields of the corresponding
lowered signature, not a parallel summary invented by the inspector, and
associated items are not embedded into `TraitSignature`. The browser renders
each returned `Option<Stashed<...>>`, binder, slice, symbol wrapper, and nested
type with the same structural-reflection rules as local results.

Navigation edges are inspector values built from retained identities and the
path by which the user arrived; they are not fields added to `SymExt`. Module
membership is the complete result of its keyed metadata query. If the client
bounds a large rendering, it shows an explicit truncation or continuation node
rather than relabelling the visited branch as the query result.

Availability is determined by symbol origin and kind. The service reports an
unsupported or unavailable product as structured status, allowing the client
to disable or omit the corresponding tab without guessing from an empty value.

### Inspection operations

The operation set is:

| Operation | Result |
|---|---|
| `item` | item kind, path, source location, and owner |
| `concrete` | local source-written and effective expanded concrete representation plus provenance |
| `signature` | checked semantic signature and predicates |
| `body` | completed elaborated typed IR and diagnostics |
| `impls` | relevant impl candidate identities and completeness |
| `prove` | a trait proof result with `Proven` output |
| `normalize` | an alias-normalization result with `Type` output |

`item`, `concrete`, `signature`, and `body` expose symbol products. `impls`,
`prove`, and `normalize` expose focused semantic operations. `trace last`,
revision comparison, and `rerun` expose the work and reuse behind either kind
of inspection.

`body` returns completed typed IR, not checking temporaries. Method syntax is
therefore shown as its resolved call, adjustments are explicit operations,
and lifetimes follow the existing `Dummy` policy.

`prove` and `normalize` use the solver's public operation boundary. Trait proof
does not expose a selected impl, and normalization is input-only; the inspector
must not invent a caller expected type as part of candidate selection.

The service returns a typed `InspectionResult` or equivalent observation
value. HTTP, text, test, and future LSP adapters consume that result. The exact
enum layout is deliberately left to implementation so that it can follow the
typed IR and solver APIs already present.

### Human-readable typed IR

The inspector gets a deterministic structural explorer designed for review.
The default body view is an expandable tree which mirrors the completed typed
IR: fields and collection elements are edges, expression variants are nodes,
and every expression displays its semantic type. The complete review detail is
inline in the tree, including the Rust representation, precise type, span,
resolved definition or field, dispatch form, substitutions, lifetime, and
other variant-specific data. Source-like syntax and raw debug structure are
secondary renderings, not the primary representation.

The explorer and its optional pretty-printers follow these rules:

- names are stable and sufficiently qualified to disambiguate;
- symbol-bearing fields remain clickable semantic references rather than
  decorated text;
- inferred type and generic arguments can be shown without raw allocation
  identities;
- every embedded semantic type is an expandable reflected `Ty` tree, not only
  a formatted label attached to its parent;
- resolved calls identify their dispatch form;
- borrows, dereferences, coercions, and other elaborations are explicit;
- diagnostics are adjacent to the item being inspected; and
- optional verbosity can reveal spans and otherwise noisy substitutions.

This rendering is not the oracle schema. It may omit repetitive information
for readability, but it may not perform new type normalization, candidate
selection, or other semantic computation. The service creates a self-contained
structured observation while it can access the database; formatting and
client-side tree expansion operate on that observation without hidden semantic
work.

Pretty-printer snapshots test readability and stability. Exact oracle tests
continue to compare independently emitted shared IR by textual identity. A
pretty-printer snapshot passing cannot make an oracle mismatch pass.

### Structured query traces

The trace answers “what work did this request cause?” rather than dumping an
implementation log. Each event records at least:

- a phase: workspace bootstrap, selection, requested analysis, or rendering;
- a source: Salsa execution, Sage semantic lookup, or external metadata;
- a stable operation family; and
- a stable semantic key; and
- its dynamic parent operation, when it ran while servicing another recorded
  operation.

Useful event classes include:

1. **Salsa query request:** one tracked function was requested by its parent.
   Its disposition distinguishes a function body which actually executed from
   a memoized value which was reused. Reuse may include a memo validated in the
   current request or a value already known to be valid for the current
   revision.
2. **Semantic lookup:** a public keyed boundary was requested, such as impl
   candidates for one trait and optional self head.
3. **External metadata:** a typed `TcxDb` operation requested one stable
   external item, impl header, or associated value.

The current raw `Debug` rendering of a Salsa database key is useful diagnostic
data but is not a durable assertion format. The recorder projects supported
query families into stable keys. An optional raw mode may accompany the
structured trace when debugging an unmapped query.

The interactive presentation is a rooted dynamic execution tree. Its root is
the user's inspection request; children are the Salsa queries, semantic
operations, solver work, or metadata reads requested while executing their
parent. This is the tree for one execution, not Salsa's complete persistent
dependency graph. A flat `WillExecute` sequence is insufficient to recover the
tree: the recorder must capture the active parent or use explicit nested Sage
operation spans rather than inferring parentage from adjacent events.

The tree must remain usable for deep executions. Every branch is independently
collapsible, with expand-all and collapse-all actions. The execution pane can
be widened with a draggable divider and promoted to a full-screen focused view;
neither operation reruns semantic work or changes the recorded observation.

Every major inspection panel—including symbol search, source, typed result, IR
tree, and execution tree—has the same grow/restore
affordance. Growing a panel gives it the full available viewport so deeply
nested or wide structures can be reviewed without changing the inspection
request or executing additional semantic work.

`WillExecute` and `DidValidateMemoizedValue` are useful evidence for executed
and validated queries, respectively, but they are not a complete query-request
API: in particular, an already-verified memo may be returned without either
event describing the fetch. If the selected Salsa version does not expose a
structured fetch hook, the inspector must add explicit request spans around
the stable query families it promises to show (or contribute the required hook
upstream). It must not silently present “no execution event” as proof that no
query was requested.

Test assertions normally normalize the same events as a set or multiset.
Execution order and sibling order are available only as explicit diagnostic
modes; concurrent scheduling order is not a semantic or incremental contract.

Selection and requested analysis remain separate phases so a test can
distinguish the cost of finding an item from the cost of checking it.
Rendering consumes the already-owned observation after the analysis trace is
frozen. A test fails if ordinary rendering needs an unreported semantic query.

### Incremental experiments

An interactive session retains:

- the selector and operation used by `rerun`;
- the last structured result and trace;
- a monotonically increasing workspace revision; and
- whether the latest change was an input update or a full reload.

The destination also retains input deltas and every inspection run under the
Salsa revision in which it occurred. Advancing a revision does not itself run
semantic queries: a revision with no user request or automatic refresh has no
work tree. The revisions view compares edit batches with the work subsequently
executed, validated, or reused without claiming a causal dependency edge that
the recorder did not observe.

Watch mode observes relevant workspace files, applies changes to the host, and
marks the previous result stale. It need not execute continuously after every
keystroke: rerunning on an explicit command or after a debounce is sufficient.
What matters is that the same database receives the edit.

The testing API supports the following matrix:

| Run | Expected evidence |
|---|---|
| cold | required queries and metadata reads execute |
| warm | unchanged top-level result is reused |
| relevant edit | affected boundary and downstream consumers execute |
| unrelated edit | an index/lookup may reexecute, but an equal result is backdated and downstream work remains reused |

Assertions operate on structured events. They can require exact event
sets/multisets for a tightly pinned slice, or state required and forbidden
event patterns when incidental setup may grow. Negative assertions such as
“no associated value other than `Iterator::Item`” and “no callee body” are
first-class test cases.

Tests call the shared inspection service directly for speed and control. A
small number of browser-facing integration snapshots verify the transport and
rendering boundary. Tests which claim live incrementality must retain one host
across all revisions; reconstructing the host between runs is not valid
evidence.

### Web and future LSP clients

`cargo sage inspect` starts the Axum backend and JavaScript client as one
loopback web application. The browser searches and expands the local symbol
tree, fetches supported products for the selected identity on demand, expands
returned reflected structures, and follows semantic references. Tests call the
typed service directly; a small browser-facing test suite verifies that the
same service backs the Axum boundary.

The reusable service must not depend on terminal state, Clap types, JSON-RPC,
or LSP position encodings. A future LSP server can own the same host, apply
document changes, and expose custom requests or virtual documents for:

- item signatures and typed bodies;
- solver and impl-candidate inspections; and
- the trace for the most recent inspection request.

For example, an editor could open a read-only `sage-ir:` virtual document for
the selected function. The precise LSP extension is outside this RFD. The web
server does not need to implement or speak LSP; both are adapters over the same
service.

### Failure and incompleteness

The inspector reports unsupported or incomplete analysis as structured data,
including diagnostics and candidate-source completeness where applicable. It
must not turn ambiguity, overflow, an incomplete impl source, or an
unsupported typed-IR node into a plausible-looking completed result.

A selection failure, unavailable product, analysis failure, and workspace
reload are distinct outcomes. Trace output remains available for failed
operations when doing so is safe.

## End-state acceptance evidence

The completed RFD must include:

- local-tree and semantic-path selection for a free function and an associated
  function without checking every listed body;
- deterministic structural snapshots of concrete IR, a signature, and Typed
  IR for `DbDropGuard::db` or `Parse::next`; these snapshots must expose the
  declaration-shaped `FnCst`/`FnCstData`, recursively reflected `ExprCst` and
  `TypeCst`, `CheckedBody`/`TyBodyData`, and every nested
  `TyExpr { data, ty, span }` rather than a renderer-specific simplification;
- navigation from a Typed IR function or trait reference to an external symbol
  by retained identity, without reparsing its display path;
- separate external-symbol snapshots for `SymExt` identity, a lowered
  signature, and associated or module items, plus parent navigation and
  explicit source/body unavailability; requesting one product must not read
  the others;
- navigation from an external child to its parent and back to the original
  local symbol;
- an Axum integration test proving that loading the symbol browser does not
  eagerly request signatures or bodies and that selecting a product fetches
  only that product;
- round-trip navigation through an opaque session handle, including proof that
  changing its display label does not change the selected identity;
- coherent revision identifiers on independently fetched products;
- proof that rendering and client-side expansion of an owned observation add
  no semantic operations;
- explicit ambiguity, not-found, unavailable-product, truncation, and
  incomplete-child results; and
- one JavaScript-facing JSON integration test backed by the same service used
  in unit tests;
- a complete raw query tree with unmapped fallback;
- cold and unchanged-warm execution in one session;
- relevant and unrelated edits in one session;
- a retained Salsa revision with exact input deltas and no fabricated work
  when no operation was requested;
- automatic refresh which requests only visible demand and records that work
  under the new revision;
- comparison of aligned runs across revisions without presenting temporal
  correlation as an unrecorded invalidation edge;
- required and forbidden structured trace events; and
- a test distinguishing an input update from a workspace reload.

The impl-index edit-invalidation matrix in the Trait Impl Candidate Discovery
RFD remains a separate semantic acceptance suite. This RFD supplies the
recorder and session infrastructure needed to express it.

## Implementation slices

The destination above is one design. Delivery is divided into vertical slices
so each checkpoint produces a reviewer-visible capability and evidence, rather
than landing one architectural layer at a time:

1. **Local semantic browser.** Start `cargo sage inspect`, browse local symbols,
   and inspect `DbDropGuard::db` source, concrete IR, signature, diagnostics,
   and completed Typed IR as faithful expandable structures.
2. **Semantic navigation.** Follow references between local and external
   symbols, inspect independently requested external identities, signatures,
   and member lists, and preserve history and product availability.
3. **Focused semantic operations.** Inspect relevant impls, `prove`, and
   input-only `normalize` using the same typed service and reflected result
   model.
4. **Execution evidence.** Record the complete dynamic request tree, including
   Salsa execution/reuse, semantic lookups, and external metadata reads, with
   stable required and forbidden assertions.
5. **Live incremental experiments.** Watch source files, rerun inspections in
   one database, automatically refresh visible demand, inspect input edits and
   per-revision runs, compare repeated work, and distinguish ordinary input
   updates from full workspace reloads.

The complete work inventory, dependencies between workstreams, and per-slice
acceptance checks live in [Implementation](./implementation.md). Slices may be
reordered when implementation evidence warrants it; reordering does not alter
the destination contract or remove work from the inventory.

## Non-goals

- Replacing or relaxing the exact rustc oracle.
- Giving rustc authority to solve Sage trait or normalization goals.
- Defining a stable public protocol for every internal tracked function.
- Treating raw query execution order as a correctness result.
- Building general IDE features such as completion, rename, or diagnostics
  publication.
- Implementing the LSP server or choosing its custom protocol.
- Supporting simultaneous clients or a shared background daemon.
- Reload-free handling of every Cargo, build-script, file-membership, or
  dependency change.

## Frequently asked questions

### Why not make the web application an LSP client?

The useful reusable boundary is semantic inspection, not JSON-RPC. Making the
web application depend on an LSP server adds protocol concerns while the
server would still need the same typed service. Both clients should share that
service. An LSP-backed remote client can be added later if multi-client
workspace sharing becomes valuable.

### Why not print the oracle JSON more nicely?

The oracle schema answers a different question: whether rustc and Sage
independently emit exactly the same conformance value. It intentionally does
not expose Salsa dependencies, candidate completeness, or checking
diagnostics. Reusing it as the inspection model would either make inspection
too limited or tempt the oracle schema to absorb Sage-specific debugging
state.

### Does observing a trace change the trace?

Starting and stopping the recorder is outside semantic queries. Selection and
analysis may execute queries and are recorded in distinct phases. The
inspection result is then converted to a self-contained observation before
the analysis phase closes. Text or protocol rendering is pure. Tests enforce
this non-perturbation rule.

### Are exact trace snapshots required?

Only where the operation has a deliberately closed dependency contract.
Otherwise tests assert stable required and forbidden event patterns, usually
as sets or multisets. This avoids pinning scheduler order or irrelevant
presentation details while still detecting broadened semantic dependencies.

### Which parts are deliberately unspecified?

The JavaScript framework, exact Rust enum names, route spelling, page layout,
database-worker synchronization mechanism, filesystem-watcher crate, Salsa
snapshot ownership mechanism, raw diagnostic format, and future LSP request
names are implementation choices. Axum, a live same-process database,
on-demand JSON products, server-sent revision notifications,
semantic-reference handles,
origin/kind-based availability, a local-only workspace tree, a navigable
external metadata view, coherent result revisions, a typed inspection service,
pure rendering, complete trace capture, retained revision/run history, and
oracle separation are architectural constraints.

## Implementation

See [Implementation plan and status](./implementation.md).
