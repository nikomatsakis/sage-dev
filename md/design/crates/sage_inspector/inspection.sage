module crate::inspection;

// Sources: actor.rs, protocol.rs, server.rs.
type Response """Revision-tagged response, typed error, reflected value, or query trace.""";
context InspectionService {
    """Service boundary over an injected InspectionProvider.

{anchor}`workspace_actor`
One actor owns one coherent live workspace. Axum and the file
watcher communicate through a bounded typed client; only the actor owns and
mutates the host. Source edit batches are applied serially to the retained
database, while configuration changes reconstruct it at an explicit reload
boundary.

{anchor}`typed_service`
The typed inspection service is below transport. Service request
and result types do not depend on Axum, React, terminal state, LSP, or the
Oracle schema. HTTP handlers only decode, call `InspectionClient`, and encode.

{anchor}`source_evidence`
Inspector integration evidence starts from Rust source. A
checked-in Cargo sample project enters through the production workspace host,
live inspection provider, actor, and Axum server. Reviewed snapshots record
returned values and demand; precomputed JSON and scripted semantic providers
never supply results to an integration test. This is the Inspector-local
consequence of [Semantic evidence starts from source](../../../rfds/architectural-foundations/README.md#semantic-evidence-starts-from-source).

{anchor}`generic_browser`
The browser is a generic protocol interpreter. Symbol
presentation, positive product descriptors, render nodes, value nodes, and
trace nodes determine the UI; JavaScript does not branch on Sage symbol kinds
or known product IDs.

{anchor}`durable_navigation`
The URL stores durable intent, not response state. Selection,
product, search, grown panel, and revision views update browser history. A
revision mismatch discards every response-derived value, fetches the current
directory, and replays the URL's canonical symbol/product intent. Each event
stream connection checks the current revision, closing any notification gap
while the browser was disconnected.

{anchor}`detail_free_index`
Local search starts from one complete detail-free index. The
browser filters the returned local directory without sending search text or
fetching branches. Dependency symbols are excluded. Index construction may
use external facts needed for macro expansion, but no broader semantic
metadata or item details.

{anchor}`product_availability`
Product availability is positive and non-eager. Selecting a
symbol returns opaque product IDs, labels, and URLs without requesting any
advertised product. Local and external symbols use the same mechanism;
unsupported products are absent.

{anchor}`canonical_ownership`
Navigation uses backend-authored canonical ownership paths. Local
and external semantic references carry opaque canonical paths distinct from
labels. Paths survive unrelated/detail edits, sibling movement, and host
reconstruction; rename, move, deletion, or a forged path fails explicitly.

{anchor}`owned_reflection`
Product reflection is faithful, bounded, and owned. Ordinary
structure is derived, semantic leaves are explicit, storage handles expose
their values inline, actual recursive reentry is explicit, and limits yield
immutable continuations which require no later database borrow.

{anchor}`derive_reflection`
Representation growth flows through the derive. Product
producers do not hand-serialize Sage CST, type, signature, or Typed-IR
structures. Adding an ordinary field or variant changes reflected output
through the `Reflect` implementation unless an explicit semantic override is
documented.

{anchor}`balanced_traces`
Every tracked invocation has a balanced lifecycle span. The
span begins before memo lookup, including already-current reuse, and ends on
execution, validation, reuse, cancellation, or unwinding.

{anchor}`unknown_work`
Unknown work is visible rather than omitted. Supported Sage and
metadata operations use stable operation families; any query without a
semantic-key projection is retained as `unmapped` with its raw diagnostic
ingredient. A checked closed dependency claim fails in the presence of an
unexpected fallback.

{anchor}`semantic_phase_boundary`
Semantic work ends before serialization and browser rendering.
Every tracked read used to reflect a product lies inside the recorded
analysis/reflection boundary; view assembly is explicit and later JSON/DOM
work has no database access.

{anchor}`deterministic_trace`
Checked trace assertions use a deterministic projection. Dynamic
parentage and dispositions are retained; only explicitly unordered siblings
are canonically sorted. Tests assert closed sets or extensible
required/forbidden patterns and do not assign semantic meaning to scheduler
order or raw Salsa IDs.

{anchor}`revision_and_demand`
Input revisions and demanded work are separate facts. Revision
advancement records exact changed inputs even when no inspection follows;
retained runs record only actual demand. A workspace reconstruction is
distinct from a fine-grained source-input revision.
"""
    fn request() -> crate::inspection::Response {
        """Serialize requests through the persistent actor, apply file updates, dispatch the typed provider demand, and return revision-tagged results over HTTP. The host adapter owns compilation; this crate has no sage-ir dependency."""
        envelope {
        }
    }
}
