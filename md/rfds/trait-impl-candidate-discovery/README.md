# RFD: Trait Impl Candidate Discovery

**Status:** Draft

**Depends on:**

- [Trait System](../trait-system/README.md) — checked trait and impl signatures
- [Trait Solving](../trait-solving/README.md) — candidate assembly and result
  completeness
- [Trait Solver Design](../../design/trait-solver.md) — destination discovery
  and incrementality contract

## TL;DR

- Discover trait impl candidates across the complete compilation world visible
  through local source and external compiler metadata.
- Key candidate discovery by the fixed trait rather than enumerating impls for
  every trait.
- Add a conservative simplified-self-type key when useful, always retaining
  blanket and unclassifiable candidates.
- Make indexed discovery semantically equivalent to exhaustive discovery and
  give it a narrow Salsa invalidation boundary.

## Motivation

The original MVP solver called `local_impls(db, local_crate)` and then read
every local impl signature to filter for the queried trait. Its candidate
source had two deliberate but non-destination limitations:

1. It could not prove a goal from an impl defined in an upstream crate, even
   though such impls are globally visible to the current Rust compilation.
2. A change to any local impl invalidates the all-local-impl list and forces
   every trait query to repeat filtering, including queries for unrelated
   traits.

The first limitation is now closed for represented explicit impls. Separate
owned `TcxDb` operations expose a fixed-trait relevant-impl set, optional
conservative rigid self-head refinement, and one binder-aware impl header.
Candidate assembly lowers those headers and uses the same proof path as local
impls. Associated values remain a separate operation. The local source is
trait-keyed at its public Salsa boundary but still scans the expanded local
module tree, so the second invalidation limitation remains.

These limitations must remain explicit in the MVP without becoming accidental
architecture. Candidate discovery is a semantic completeness boundary and an
incremental query boundary in its own right.

## Change in a nutshell

Introduce a trait-keyed candidate-discovery API which combines local checked
impls with external impl metadata. Its conceptual shape is:

```text
trait_impl_candidates(
    trait_symbol,
    optional_simplified_self_type,
) -> complete deterministic candidate set
```

The fixed trait is mandatory. The external half is split into a tracked
relevant-identity query and a tracked per-header import over owned `TcxDb`
values. The exact fine-grained local index remains open. A self-type key is an
optimization which may return a conservative superset.

## Detailed plans

### Define the visible impl universe

For one compilation, discovery includes:

- impls declared in the local crate;
- impls encoded in every reachable upstream crate's metadata; and
- local impls of external traits once the external trait's defining predicates
  are available.

It excludes downstream crates and crates not present in the compilation. Rust
coherence and orphan rules constrain where impls may be defined, but candidate
discovery consumes the compiler's authoritative visible set rather than
reconstructing that set by scanning source roots.

Builtin, auto, negative, and specialization candidates may use additional
assembly paths. This RFD must state whether they share the same index or are
merged by the solver after user-impl discovery.

### Key first by trait

The primary lookup key is the complete trait identity. A query for `TraitA`
must not depend on or inspect signatures of impls for `TraitB` merely because
both occur in the same crate.

Local collection must therefore move beyond one monolithic
`local_impls(LocalCrateSymbol)` dependency. External metadata should expose a
corresponding trait-keyed operation rather than requiring Sage to enumerate
every anonymous impl definition in every dependency.

### Refine by simplified self type

An optional secondary key may classify the rigid outer shape of the normalized
or opportunistically resolved self type. Candidate classes may include:

- a specific ADT or foreign type identity;
- references and raw pointers;
- tuples, arrays, slices, and function types;
- scalar or intrinsic types; and
- an unknown/fallback class.

The index is allowed false positives but no false negatives. Blanket impls,
impls headed by a parameter, aliases which cannot yet be normalized, and any
unsupported classification remain in a fallback bucket included in every
compatible lookup.

The simplification operation must be cheap, deterministic, and independent of
speculative candidate state. A non-ground self type may omit the secondary key
and use the complete trait bucket.

### Preserve semantic equivalence

Indexing changes work selection, never proof meaning. For every goal, the
indexed candidate set contains every candidate which exhaustive visible-impl
enumeration could apply. Candidate ordering is canonicalized before it can
affect diagnostics or response normalization.

The solver's candidate-source completeness bit becomes true only after every
enabled local, external, fallback, and builtin source has been represented. A
missing metadata path yields an incomplete source and prevents exhaustive
`No`.

### Define the incremental boundary

The accepted implementation must make these invalidation properties
observable in tests:

- adding or editing an impl for `TraitB` does not reexecute candidate discovery
  for `TraitA`;
- adding an applicable impl for `TraitA` invalidates its candidate query;
- changing unrelated item bodies does not invalidate impl discovery;
- a self-head index may avoid invalidation from an impl in a disjoint rigid
  head bucket; and
- local and external candidate identities remain stable across unrelated
  edits.

The RFD will select the Salsa representation needed to achieve those
properties. A trait-keyed tracked collection, per-trait index object, or
equivalent fine-grained input may be used; one aggregate vector followed by a
tracked filter is insufficient if reading the vector records a broad
dependency.

## Required tests

The implementation is not complete until executable tests cover both semantic
and incremental requirements.

### Query-trace harness

Sage already constructs `Database` with a Salsa event callback which records
`EventKind::WillExecute`, and `Database::take_query_log()` drains that log.
`ProxyTcxDb` shares the same sink for external metadata requests. The existing
hook records tracked-function bodies which actually execute; a memoized value
served without reexecution does not emit `WillExecute`.

Executable tests now consume `take_query_log` through caller-provided and live
proxy databases. The external `IntoIter: Iterator` checkpoint asserts cold and
warm execution, the exact number and shape of relevant-impl/header requests,
and the absence of associated-item or callee-body reads. The broader test
matrix still needs a stable normalization layer over three useful event
classes:

1. **Salsa execution** — tracked function and stable key whose body executed.
2. **Semantic lookup** — trait and optional simplified-self-type key requested
   from the candidate API.
3. **External metadata** — typed `TcxDb` operation and stable external key.

Raw Salsa debug strings include internal IDs and are too brittle to be the only
contract. Semantic lookup events provide stable trait/self-type assertions;
Salsa `WillExecute` events prove whether memoized computation reran; external
events prove which rustc metadata was requested.

Each test warms fixture construction, drains the setup trace, performs one
operation, and then asserts a normalized event set or multiset. Concurrent
completion order is not snapshot-stable and is not part of the contract.
Counts are asserted only where repeated execution would itself be a bug.

### Semantic discovery tests

- A local impl of a local trait is discovered.
- A local impl of an external trait for a local type is discovered once
  external trait predicates are available.
- An upstream impl is discovered for a goal in the dependent local crate.
- An upstream blanket impl is returned through the fallback bucket.
- Impl candidates for a different trait are absent from the trait-keyed query.
- A rigid self-type lookup includes matching specific and blanket impls but
  excludes a provably disjoint head.
- A non-ground or unsimplifiable self type falls back to the complete trait
  bucket without losing candidates.
- Indexed and deliberately exhaustive discovery produce identical canonical
  solver results over a generated fixture matrix.

### Incremental reuse tests

- Salsa `WillExecute` events show that an unrelated-trait impl edit does not
  reexecute the queried trait's candidate lookup.
- Adding a relevant impl does reexecute and changes the candidate set.
- Editing an impl body without changing its signature does not invalidate the
  signature-level index.
- When self-head partitioning is enabled, a disjoint-head signature edit does
  not invalidate the narrower bucket.
- Equivalent external metadata snapshots retain stable candidate identities
  and cached results.
- A cold goal trace contains only the expected trait-keyed lookup, relevant impl
  signatures, and required external metadata calls; impls for other traits are
  absent.
- Repeating the unchanged goal produces the same solver result without a
  `WillExecute` for the cached candidate query.

These tests land with the implementation. Adding ignored or expected-failing
tests now would not enforce the requirement and adding assertions for the MVP
behavior would incorrectly make the limitation durable.

## Frequently asked questions

### Does “global impls” include downstream crates?

No. It means every impl visible in the current compilation: the local crate
and its reachable dependencies. A downstream crate which has not been compiled
as part of this world cannot affect the current query.

### Why is the trait mandatory in the key?

Trait identity is known for every trait goal and partitions impls without
loss. It is both a semantic relevance boundary and the minimum useful
incremental invalidation boundary.

### Must the self type always be part of the key?

No. A non-ground or normalization-blocked self type may use only the trait.
Self-type simplification is a conservative optimization, not a completeness
precondition.

### Why not keep filtering `local_impls` in the solver?

It gives correct local MVP behavior, but it forces broad dependencies and
cannot incorporate rustc's external relevant-impl metadata cleanly. Candidate
discovery belongs behind an explicit query contract.

## Implementation

See [Implementation plan and status](./implementation.md).
