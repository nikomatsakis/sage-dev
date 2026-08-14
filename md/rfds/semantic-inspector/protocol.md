# Semantic Inspector `/api/v1` protocol

**Status:** Draft

**Parent:** [Semantic Inspector](./README.md)

This page is the normative JSON contract between the Semantic Inspector
backend and browser. The [web application walkthrough](./web-application.md)
explains when each resource is requested and which Sage operation supplies it;
this page defines the bytes which cross that boundary.

The protocol is internal to one `cargo sage inspect` executable and its
embedded assets. It is versioned and tested exactly, but it is not a public
remote API compatibility promise.

## Serialization rules

- JSON is UTF-8 without a byte-order mark.
- Checked response files use two-space indentation, LF line endings, and one
  trailing newline.
- Object fields appear in the order shown by the DTOs on this page. A missing
  variant field is omitted rather than serialized as `null`; fields explicitly
  declared as nullable are always present.
- Semantic maps cross the protocol as ordered entry arrays. JSON object-key
  order is never used to encode semantic order.
- Enum and union variants use the exact lowercase kebab-case tags shown here.
- IDs and handles are case-sensitive JSON strings. A session-scoped handle has
  one of the URL-safe forms `nav_<token>`, `op_<token>`, `cont_<token>`,
  `run_<token>`, or `rev_<token>`, where `<token>` contains only ASCII letters,
  digits, `_`, and `-`. Clients treat the entire value as opaque.
- Handles therefore require no percent-encoding when substituted into the
  path templates below. Other query values use ordinary UTF-8 percent
  encoding.
- Exact fixture tests compare response bytes directly. They do not parse and
  reserialize, redact, or filter the actual response.

The DTO notation below uses these string aliases:

```text
NavigationHandle = String  // nav_<token>
OperationHandle = String   // op_<token>
ContinuationHandle = String // cont_<token>
RunHandle = String         // run_<token>
RevisionHandle = String    // rev_<token>
```

The comments constrain the prefix; all five values remain opaque strings to
the client.

## Contractual HTTP behavior

Every `/api/v1` request sends:

```text
Accept: application/json
X-Sage-Session: <unguessable session token>
```

Every request after session bootstrap also sends:

```text
X-Sage-Expected-Epoch: <database_epoch>
X-Sage-Expected-Revision: <revision>
X-Sage-Action: <opaque browser-action identifier>
```

`X-Sage-Action` correlates requests caused by one user action. It has no
semantic meaning. The fixture server validates its scenario-defined value; the
production server records it without trusting it as authorization.

Ordinary JSON responses send:

```text
Content-Type: application/json; charset=utf-8
Cache-Control: no-store
X-Content-Type-Options: nosniff
```

The event stream instead sends `Content-Type: text/event-stream` and
`Cache-Control: no-store`. Static application assets and their content-security
headers are specified by the web application rather than this JSON protocol.

All well-formed requests to a known semantic resource return HTTP 200,
including semantic `unavailable`, `incomplete`, and `failed` results. Transport
and session failures use:

| HTTP status | Meaning |
|---:|---|
| 400 | malformed JSON, invalid tag, or invalid path/query parameter |
| 403 | invalid session or rejected origin |
| 404 | unknown API route |
| 409 | expected epoch or revision is stale |
| 410 | a well-formed session handle has expired |

These failures return `ProtocolError` rather than a semantic envelope:

```text
ProtocolError {
  code: String,
  message: String,
}
```

Clients branch on `code` and display `message`; they never parse message text.

## Common semantic envelope

Every ordinary semantic response has this field order:

```text
Envelope<T> {
  database_epoch: String,
  revision: String,
  request_id: String,
  run_id: RunHandle | null,
  result: Result<T>,
}

Result<T> =
    {
      status: "available",
      value_version: String,
      value: T,
    }
  | {
      status: "unavailable",
      reason: Issue,
    }
  | {
      status: "incomplete",
      reason: Issue,
      partial: T | null,
    }
  | {
      status: "failed",
      error: Issue,
    }
  | {
      status: "cancelled",
      reason: Issue,
    }

Issue {
  code: String,
  message: String,
}
```

`run_id` is always present. It is `null` when satisfying the request performed
no recordable semantic work, including reads of retained run, continuation, or
revision data. `value_version` identifies the complete available value within
one database epoch; it is not an incremental revision.

`available` means a complete value was produced. `unavailable` means the
operation is meaningful but cannot be supplied for this target, for example an
external body. `incomplete` preserves an explicit terminally incomplete result
and optional partial value. `failed` reports an analysis failure. `cancelled`
reports work abandoned because its coherent read could not finish.

## Shared scalar DTOs

```text
Origin = "local" | "external"

SymbolKind =
    "module"
  | "function"
  | "struct"
  | "enum"
  | "variant"
  | "field"
  | "trait"
  | "impl"
  | "associated-function"
  | "associated-type"
  | "associated-const"
  | "type-alias"
  | "opaque-type"
  | "const"
  | "static"
  | "macro"
  | "unsupported"

Position {
  line: u32,
  column: u32,
}

SourceLocation {
  file: String,
  start: Position,
  end: Position,
  encoding: "utf-8",
}

NavigationReference {
  handle: NavigationHandle,
  label: String,
  origin: Origin,
  symbol_kind: SymbolKind,
}

Provenance =
    {
      kind: "source",
      location: SourceLocation,
    }
  | {
      kind: "generated",
      location: SourceLocation,
      generated_by: NavigationReference | null,
    }
  | {
      kind: "external-metadata",
      crate_name: String,
    }
```

Positions are zero-based UTF-8 byte columns. An LSP adapter converts UTF-16
positions at its boundary.

## Session

```http
GET /api/v1/session
```

```text
Session {
  protocol_version: "1",
  target: CargoTarget,
  workspace_root: String,
  capabilities: [Capability],
  retained_revisions: RetainedRevisionRange,
}

CargoTarget {
  package: String,
  target_kind: "lib" | "bin" | "example" | "test" | "bench",
  target_name: String,
}

Capability =
    "symbol-index"
  | "path-selection"
  | "position-selection"
  | "products"
  | "focused-operations"
  | "runs"
  | "events"
  | "revisions"
  | "revision-comparison"

RetainedRevisionRange {
  first: RevisionHandle | null,
  last: RevisionHandle | null,
}
```

Capabilities appear in the order listed above. A delivery slice advertises
only implemented capabilities; fixture data never claims a capability whose
route is intentionally absent in that slice.

## Complete local symbol index

```http
GET /api/v1/symbols
```

```text
SymbolIndex {
  root: NavigationHandle,
  symbols: [SymbolSummary],
}

SymbolSummary {
  handle: NavigationHandle,
  parent: NavigationHandle | null,
  name: String,
  display_path: String,
  origin: "local",
  symbol_kind: SymbolKind,
  provenance: Provenance,
  children: ChildCompleteness,
}

ChildCompleteness =
    { status: "complete" }
  | { status: "not-applicable" }
  | { status: "incomplete", reason: Issue }
```

The array is a deterministic source/preorder traversal: parents precede their
children and siblings follow represented source order. Parent edges are the
complete local tree; there is no branch-fetch resource. `children` reports
whether the index's represented children are complete and is not a list or an
invitation to fetch another local resource. External symbols never appear.

## Selecting a symbol

An existing handle is selected with:

```http
GET /api/v1/symbols/{navigation-handle}
```

Path and source-position selection use:

```http
POST /api/v1/select
```

```text
SelectRequest {
  selector: Selector,
}

Selector =
    {
      kind: "path",
      path: String,
    }
  | {
      kind: "source-position",
      file: String,
      line: u32,
      column: u32,
      encoding: "utf-8",
    }

SelectionOutcome =
    {
      kind: "selected",
      symbol: SelectedSymbol,
    }
  | {
      kind: "ambiguous",
      selector: Selector,
      candidates: [SelectionCandidate],
    }
  | {
      kind: "not-found",
      selector: Selector,
    }

SelectionCandidate {
  handle: NavigationHandle,
  display_path: String,
  origin: Origin,
  symbol_kind: SymbolKind,
}
```

`POST /select` returns `Envelope<SelectionOutcome>`. An ambiguity or not-found
result is an available, complete selection outcome, not a transport error or
semantic failure. `GET /symbols/{handle}` returns `Envelope<SelectedSymbol>`.

```text
SelectedSymbol {
  handle: NavigationHandle,
  name: String,
  display_path: String,
  origin: Origin,
  symbol_kind: SymbolKind,
  parent: NavigationReference | null,
  source_location: SourceLocation | null,
  provenance: Provenance,
  products: [ProductDescriptor],
}

ProductKind =
    "source"
  | "concrete"
  | "signature"
  | "body"
  | "fields"
  | "items"

ProductDescriptor {
  kind: ProductKind,
  availability: ProductAvailability,
}

ProductAvailability =
    {
      state: "available",
      href: String,
    }
  | {
      state: "unavailable",
      reason: Issue,
    }
  | {
      state: "not-applicable",
    }
```

The product array always contains all six kinds in the order listed above.
The browser may reorder known views for presentation, but it does not infer
availability. An available `href` is an exact same-origin `/api/v1` path. The
server revalidates a product request, so a stale direct request can still
return unavailable, incomplete, failed, or cancelled.

## Symbol products

```http
GET /api/v1/symbols/{navigation-handle}/products/{product-kind}
```

The response is `Envelope<ProductValue>`:

```text
ProductValue =
    {
      product: "source",
      source: SourceProduct,
    }
  | {
      product: "concrete",
      value: ValueNode,
    }
  | {
      product: "signature",
      value: ValueNode,
    }
  | {
      product: "body",
      value: ValueNode,
    }
  | {
      product: "fields",
      value: ValueNode,
    }
  | {
      product: "items",
      value: ValueNode,
    }

SourceProduct {
  location: SourceLocation,
  text: String,
  highlight_start: u32,
  highlight_end: u32,
}
```

Source highlight offsets are UTF-8 byte offsets into `text`. Diagnostics are a
browser view of the body product; there is no diagnostics route.

## Reflected value tree

```text
ValueNode =
    {
      kind: "record",
      type_name: String,
      fields: [ValueField],
    }
  | {
      kind: "variant",
      enum_name: String,
      variant_name: String,
      fields: [ValueField],
    }
  | {
      kind: "sequence",
      type_name: String,
      items: [ValueNode],
    }
  | {
      kind: "scalar",
      type_name: String,
      value: null | bool | number | String,
    }
  | {
      kind: "reference",
      label: String,
      navigation_target: NavigationHandle,
      origin: Origin,
      symbol_kind: SymbolKind,
    }
  | {
      kind: "operation-target",
      label: String,
      operation_target: OperationHandle,
      operations: [OperationDescriptor],
      value: ValueNode,
    }
  | {
      kind: "shared",
      identity: String,
      value: ValueNode,
    }
  | {
      kind: "shared-reference",
      identity: String,
    }
  | {
      kind: "truncated",
      summary: String,
      continuation: ContinuationHandle,
    }

ValueField {
  name: String,
  value: ValueNode,
}

OperationDescriptor {
  kind: "impls" | "prove" | "normalize",
  href: String,
}
```

Records, variants, wrappers, options, and map-entry projections use these
ordinary nodes. Summaries are additive: an `operation-target` wraps the full
reflected `value`, and a reference retains both its label and typed navigation
handle. The client matches this union exhaustively; an unknown `kind` is a
visible protocol error rather than a silently omitted subtree.

`shared.identity` and `shared-reference.identity` are display-local IDs, not
process addresses. The first occurrence carries the value. Truncation is
explicit and can be continued without executing Sage work.

## Focused semantic operations

```http
POST /api/v1/operations/impls
POST /api/v1/operations/prove
POST /api/v1/operations/normalize
```

```text
ImplsRequest {
  trait: OperationHandle,
  self_head: OperationHandle | null,
}

ProveRequest {
  goal: OperationHandle,
}

NormalizeRequest {
  alias: OperationHandle,
}

OperationValue =
    {
      operation: "impls",
      value: ValueNode,
    }
  | {
      operation: "prove",
      value: ValueNode,
    }
  | {
      operation: "normalize",
      value: ValueNode,
    }
```

Responses are `Envelope<OperationValue>`. The handles recover typed inputs
retained by the service. `NormalizeRequest` has no expected-type field. The
reflected proof value is `Proven`, not a selected impl. Impls reflects candidate
identities and completeness.

## Continuations

```http
GET /api/v1/continuations/{continuation-handle}
```

```text
ContinuationValue {
  continuation: ContinuationHandle,
  items: [ValueNode],
  next: ContinuationHandle | null,
}
```

The response is `Envelope<ContinuationValue>` with `run_id: null` and reads
only the retained owned observation. `next` is always present.

## Run observations and traces

```http
GET /api/v1/runs/{run-handle}
```

```text
RunObservation {
  run_id: RunHandle,
  action_id: String | null,
  request: RunRequest,
  root: TraceNode,
}

RunRequest =
    { kind: "symbol-index" }
  | { kind: "selection", selector: Selector }
  | { kind: "symbol", target: NavigationHandle }
  | { kind: "product", target: NavigationHandle, product: ProductKind }
  | { kind: "operation", operation: "impls" | "prove" | "normalize" }
  | { kind: "automatic-refresh", resource: String }

TraceNode {
  phase: "bootstrap" | "selection" | "analysis" | "reflection",
  source: "salsa" | "sage" | "solver" | "external-metadata",
  operation: String,
  key: TraceKey,
  disposition: "executed" | "validated" | "reused" | "observed",
  child_order: "sequential" | "unordered",
  children: [TraceNode],
}

TraceKey =
    {
      kind: "semantic",
      value: String,
    }
  | {
      kind: "unmapped",
      ingredient: String,
    }
```

The response is `Envelope<RunObservation>` with `run_id: null`: reading an
already-retained run is not a child of the run being displayed.

`child_order` is recorded by the producer of the parent span. `sequential`
preserves capture order. `unordered` declares that sibling order is not part of
the contract. Checked serialization recursively canonicalizes every unordered
child subtree, serializes that subtree using this protocol, sorts the resulting
UTF-8 byte strings lexicographically, and retains duplicates. Identical byte
strings need no tie-breaker because exchanging them cannot change the output.

An unmapped event retains a stable code-generated Salsa ingredient name. Raw
Salsa keys, timestamps, and arrival order appear only in interactive
diagnostics and test-failure artifacts. If even the ingredient cannot be named,
the checked key uses `ingredient: "unknown"`; the event remains visible, but a
test containing it cannot claim a closed exact dependency contract until that
family is mapped.

## Revision events

```http
GET /api/v1/events
Accept: text/event-stream
```

The stream emits these event names and JSON data payloads:

```text
event: revision-advanced
data: RevisionAdvanced

event: workspace-reloaded
data: WorkspaceReloaded
```

```text
RevisionAdvanced {
  database_epoch: String,
  revision: RevisionHandle,
  edit_batch: String,
  changed_inputs: [InputIdentity],
}

WorkspaceReloaded {
  previous_database_epoch: String,
  database_epoch: String,
  revision: RevisionHandle,
  reason: Issue,
}

InputIdentity {
  kind: "source-file",
  path: String,
  field: "text",
}
```

The SSE `id` is an opaque monotonically increasing connection-event token used
for reconnect. Keepalive comments carry no semantic data.

## Revision history and comparison

```http
GET /api/v1/revisions?cursor={cursor}
GET /api/v1/revisions/{revision-handle}
GET /api/v1/revisions/compare?from={revision-handle}&to={revision-handle}&selector={selector-token}&product={product-kind}
```

`cursor` and `selector-token` are opaque URL-safe strings using the same token
alphabet as handles.

```text
RevisionPage {
  revisions: [RevisionSummary],
  next_cursor: String | null,
}

RevisionSummary {
  revision: RevisionHandle,
  database_epoch: String,
  input_delta_count: u32,
  run_count: u32,
}

RevisionDetail {
  summary: RevisionSummary,
  input_deltas: [InputDelta],
  runs: [RunHandle],
}

InputDelta {
  input: InputIdentity,
  old_hash: String,
  new_hash: String,
  diff: String,
}

RunComparison {
  from_revision: RevisionHandle,
  to_revision: RevisionHandle,
  selector: String,
  product: ProductKind,
  value_changed: bool,
  executed_only_before: [TraceIdentity],
  executed_only_after: [TraceIdentity],
  reused_only_before: [TraceIdentity],
  reused_only_after: [TraceIdentity],
}

TraceIdentity {
  source: "salsa" | "sage" | "solver" | "external-metadata",
  operation: String,
  key: TraceKey,
}
```

These resources return their matching `Envelope<T>` with `run_id: null`.
Comparison reports observed differences; it does not invent a causal
invalidation edge.

## Exact fixture bundle

The reviewed contract fixture has this shape:

```text
test-fixtures/semantic-inspector/db-drop-guard/
├── source/
│   └── src/lib.rs
├── api/
│   ├── routes.json
│   ├── requests/
│   ├── responses/
│   └── demand/
└── scenarios/
```

```text
RouteFixture {
  name: String,
  request: FixtureRequest,
  response: FixtureResponse,
  expected_demand: String,
}

FixtureRequest {
  method: "GET" | "POST",
  path: String,
  headers: [HeaderEntry],
  body: String | null,
}

FixtureResponse {
  status: u16,
  headers: [HeaderEntry],
  body: String,
}

HeaderEntry {
  name: String,
  value: String,
}
```

Header arrays are lowercase-name sorted. `body` and `expected_demand` name
files relative to `api/`; a request body is canonical JSON using the same
formatting rules. The strict dummy server matches method, path plus query,
contractual headers, and exact request-body bytes. It rejects an unknown or
duplicate request unless the scenario explicitly declares multiplicity.

Slice 1 uses this bundle only as dummy-server input for the browser; it has no
Axum or Rust DTO implementation. Slice 2 constructs independent typed scripted
Rust values, serializes them through the production DTO and Axum path, and
compares those actual bytes with the reviewed response files. It never loads a
response file as the value to serialize. As later slices replace scripted
resources with real Sage operations over `source/`, the same comparisons become
semantic backend evidence one resource at a time.

Snapshot-update mode writes candidate files for review. A backend mismatch
never passes merely because the implementation emitted new output.

## Protocol evolution

While this RFD is Draft, a protocol change updates this page, every affected
response/request fixture, backend expectation, frontend type, and scenario in
the same commit. Once a delivery slice establishes
[SI-A3](./README.md#si-a3), later slices treat those established shapes as
regression gates. An incompatible redesign uses `/api/v2`; adding a field or
variant to `/api/v1` is still a reviewed exact-contract change even when old
clients could theoretically ignore it.
