# Semantic Inspector `/api/v1` protocol

**Status:** Completed

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
- Product IDs and ephemeral handles are case-sensitive URL-safe strings.
  Product IDs contain ASCII letters, digits, `_`, and `-`; clients treat
  continuation, run, and revision handles as opaque rather than interpreting
  a prefix.
- A `SymbolPath` is a canonical slash-separated sequence of backend-authored,
  URL-safe ownership segments. The frontend treats the complete string as
  opaque. When used as a query value it is percent-encoded normally; clients
  do not parse or construct its segments.
- Exact fixture tests compare response bytes directly. They do not parse and
  reserialize, redact, or filter the actual response.

The DTO notation below uses these string aliases:

```text
SymbolPath = String         // canonical backend-authored ownership path
ProductId = String          // opaque URL-safe product identifier
ContinuationHandle = String // cont_<token>
RunHandle = String         // run_<token>
RevisionId = String        // rev_<token>
```

The comments constrain serialization; all five values remain opaque strings
to the client.

## Contractual HTTP behavior

Every `/api/v1` JSON request sends:

```text
Accept: application/json
```

Requests do not send a session token, expected revision, or browser-action
header. A browser action correlates its requests using the `request_id`
returned in each response.

Ordinary JSON responses send:

```text
Content-Type: application/json; charset=utf-8
Cache-Control: no-store
```

The event stream instead sends `Content-Type: text/event-stream` and
`Cache-Control: no-store`. Static application assets are specified by the web
application rather than this JSON protocol.

Successful resource requests return HTTP 200. Missing resources and protocol
or server failures use:

| HTTP status | Meaning |
|---:|---|
| 400 | malformed JSON, invalid tag, or invalid path/query parameter |
| 404 | unknown route, unresolved symbol path, invalid ephemeral handle, or a product not listed for that symbol |
| 500 | unexpected inspector failure |

Every successful response has this field order:

```text
Response<T> {
  revision_id: RevisionId,
  request_id: String,
  run_id: RunHandle | null,
  value: T,
}
```

Every error response, including an unresolved path or invalid ephemeral handle,
repeats the backend's current revision:

```text
ErrorResponse {
  revision_id: RevisionId,
  request_id: String,
  run_id: RunHandle | null,
  error: ApiError,
}

ApiError {
  code: String,
  message: String,
}
```

`run_id` is always present. It is `null` when satisfying the request performed
no recordable semantic work, including reads of retained run, continuation, or
revision data. The service assigns `revision_id` from one process-wide
sequence, including across a database reconstruction, so the browser never
needs a separate database-lifetime identifier.

For a success, `revision_id` identifies the coherent state which produced the
value and is still current when the owned response is frozen. For an error, it
is the backend's current revision when the error is constructed. A later
update is announced through the revision event stream. The browser never
installs a response whose ID differs from the revision it is displaying. It
discards every response-derived value, bootstraps a fresh directory, and
replays the current URL instead.

Domain outcomes do not use `ErrorResponse`: selection ambiguity, solver
ambiguity or overflow, candidate incompleteness, diagnostics from erroneous
Rust, and explicit unsupported Sage features remain variants or fields of the
corresponding value DTO. Cancellation caused by a concurrent update is retried
internally; it is not a public result status.

## Shared scalar DTOs

```text
Issue {
  code: String,
  message: String,
}

Badge {
  label: String,
  tone: "neutral" | "accent" | "success" | "warning" | "danger",
}

SymbolPresentation {
  eyebrow: String | null,
  badges: [Badge],
}

SymbolReference {
  path: SymbolPath,
  label: String,
  presentation: SymbolPresentation,
}
```

Symbol kind, origin, source location, and provenance may be reflected inside
product content, but they are not frontend control enums. Directory and header
styling uses only the generic presentation fields above.

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
  | "products"
  | "runs"
  | "events"
  | "revisions"
  | "revision-comparison"

RetainedRevisionRange {
  first: RevisionId | null,
  last: RevisionId | null,
}
```

Capabilities appear in the order listed above. A delivery slice advertises
only implemented capabilities; fixture data never claims a capability whose
route is intentionally absent in that slice.

## Current revision

```http
GET /api/v1/revision
```

This returns `Response<null>`. Its top-level `revision_id` is the backend's
current coherent revision. The browser requests it during bootstrap and may
request it again whenever it wants to check whether the view it is displaying
is current.

## Complete local symbol index

```http
GET /api/v1/symbols
```

```text
SymbolIndex {
  root: SymbolPath,
  symbols: [SymbolSummary],
}

SymbolSummary {
  path: SymbolPath,
  parent: SymbolPath | null,
  label: String,
  display_path: String,
  search_text: String,
  presentation: SymbolPresentation,
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

## Selecting a known symbol

The browser selects a local symbol using the canonical path already present in
the complete symbol index. It selects an external symbol using a path from a
reflected semantic reference:

```http
GET /api/v1/symbol?path={symbol-path}
```

This returns `Response<SelectedSymbol>`. The query value uses ordinary percent
encoding; the frontend does not parse the path before sending it.

Local search text never crosses the API boundary. The browser filters the
complete local index and sends the chosen row's path. External symbols are
not text-searchable in this RFD and remain reachable through semantic links.
Source-position selection is deferred until an editor-facing client requires
it.

```text
SelectedSymbol {
  path: SymbolPath,
  label: String,
  display_path: String,
  presentation: SymbolPresentation,
  parent: SymbolReference | null,
  products: [ProductDescriptor],
}

ProductDescriptor {
  id: ProductId,
  label: String,
  href: String,
}
```

The product array contains only the pages valid for this symbol, in display
order. IDs are opaque and are not a frontend enum. The browser creates tabs
directly from this array and labels them from `label`; it does not maintain a
symbol-kind, origin, or product table. Each `href` is an exact relative
`/api/v1` path. The server revalidates a product request. A direct request for
a product which the current list does not contain returns HTTP 404 with error
code `product-not-found`.

## Symbol products

```http
GET /api/v1/product?symbol={symbol-path}&product={product-id}
```

The response is `Response<ProductPage>`:

```text
ProductPage {
  id: ProductId,
  title: String,
  content: RenderNode,
}
```

The browser neither branches on `id` nor interprets the semantic meaning of
`title`. A source page, a diagnostic page, and a reflected typed body all use
the same response shape.

## Generic rendering tree

```text
RenderNode =
    {
      kind: "group",
      layout: "block" | "row" | "columns",
      children: [RenderNode],
    }
  | {
      kind: "heading",
      level: 1 | 2 | 3,
      text: String,
    }
  | {
      kind: "text",
      text: String,
    }
  | {
      kind: "code",
      language: String,
      text: String,
      highlights: [CodeHighlight],
    }
  | {
      kind: "notice",
      tone: "neutral" | "info" | "warning" | "error",
      title: String | null,
      text: String,
    }
  | {
      kind: "navigation",
      target: SymbolReference,
    }
  | {
      kind: "value",
      value: ValueNode,
    }

CodeHighlight {
  start: u32,
  end: u32,
  role: String,
}
```

Code highlight offsets are UTF-8 byte offsets into `text`. The rendering union
is the complete frontend page vocabulary. An unknown node is a visible
protocol error. Product identifiers never select a renderer; the page's tree
does.

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
      target: SymbolReference,
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
      continuation?: ContinuationHandle,
    }

ValueField {
  name: String,
  value: ValueNode,
}
```

Records, variants, wrappers, options, and map-entry projections use these
ordinary nodes. Summaries are additive, and a reference retains both its
display label and canonical symbol path. The client matches this union
exhaustively; an unknown `kind` is a visible protocol error rather than a
silently omitted subtree.

`shared.identity` and `shared-reference.identity` are display-local IDs, not
process addresses. The first occurrence carries the value. Truncation is
explicit and can be continued without executing Sage work.

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

The response is `Response<ContinuationValue>` with `run_id: null` and reads
only the retained owned observation. `next` is always present.

## Run observations and traces

```http
GET /api/v1/runs/{run-handle}
```

```text
RunObservation {
  run_id: RunHandle,
  request: RunRequest,
  root: TraceNode,
}

RunRequest =
    { kind: "symbol-index" }
  | { kind: "symbol", target: SymbolPath }
  | { kind: "product", target: SymbolPath, product: ProductId }
  | { kind: "automatic-refresh", resource: String }

TraceNode {
  phase: "bootstrap" | "selection" | "analysis" | "reflection" | "view-assembly",
  source: "salsa" | "sage" | "solver" | "external-metadata",
  operation: String,
  key: TraceKey,
  disposition: "executed" | "validated" | "reused" | "cancelled" | "observed",
  child_order: "sequential" | "unordered",
  observations?: Integer,
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

The response is `Response<RunObservation>` with `run_id: null`: reading an
already-retained run is not a child of the run being displayed.

Every tracked-query invocation is represented by the balanced span emitted by
the temporary Salsa fork, including an already-current memo fetch. Its
disposition is `executed`, `validated`, `reused`, or `cancelled`; nested spans
determine the tree rather than arrival adjacency. Sage and metadata spans use
`observed` when none of the Salsa dispositions applies. Repeated identical
leaf requests may be stored once with `observations > 1`; omission means one.
This is a lossless multiplicity encoding, not sampling.

`child_order` is recorded by the producer of the parent span. `sequential`
preserves capture order. `unordered` declares that sibling order is not part of
the contract. Checked serialization recursively canonicalizes every unordered
child subtree, serializes that subtree using this protocol, sorts the resulting
UTF-8 byte strings lexicographically, and retains duplicate multiplicity
through `observations`. Identical byte strings need no tie-breaker because
exchanging them cannot change the output.

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

The browser requests `GET /api/v1/revision` whenever this stream connects or
reconnects. Events are wakeups rather than a durable replay log; the revision
handshake detects an update published while the connection was absent.

```text
RevisionAdvanced {
  revision_id: RevisionId,
  edit_batch: String,
  changed_inputs: [InputIdentity],
}

WorkspaceReloaded {
  previous_revision_id: RevisionId,
  revision_id: RevisionId,
  reason: Issue,
}

InputIdentity {
  kind: "source-file",
  path: String,
  field: "text",
}
```

Keepalive comments carry no semantic data. The stream does not promise replay
IDs; the current-revision handshake is the reconnect contract.

## Revision history and comparison

```http
GET /api/v1/revisions?cursor={cursor}
GET /api/v1/revisions/{revision-id}
GET /api/v1/revisions/compare?from={revision-id}&to={revision-id}&symbol={symbol-path}&product={product-id}
```

`cursor` is an opaque URL-safe string. The symbol path uses ordinary query
percent-encoding.

```text
RevisionPage {
  revisions: [RevisionSummary],
  next_cursor: String | null,
}

RevisionSummary {
  revision_id: RevisionId,
  cause: RevisionCause,
  input_delta_count: u32,
  run_count: u32,
}

RevisionCause =
  { kind: "initial" }
  | { kind: "input-edit", edit_batch: String }
  | {
      kind: "workspace-reload",
      previous_revision_id: RevisionId,
      reason: ErrorBody,
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
  from_revision: RevisionId,
  to_revision: RevisionId,
  symbol: SymbolPath,
  product: ProductId,
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
  observations?: Integer,
}
```

These resources return their matching `Response<T>` with `run_id: null`.
Comparison reports observed differences; it does not invent a causal
invalidation edge. `RevisionCause` records whether the retained revision is
the initial workspace state, an incremental input-edit batch, or a database
rebuild. A rebuild links to the last revision of the previous database
generation so that revision history does not disguise a wholesale reload as
ordinary incremental invalidation.

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

The completed `/api/v1` contract is a regression gate. A protocol change
updates this page, every affected response/request fixture, backend
expectation, frontend type, and scenario in the same commit. An incompatible
redesign uses `/api/v2`; adding a field or variant to `/api/v1` is still a
reviewed exact-contract change even when old clients could theoretically
ignore it.
