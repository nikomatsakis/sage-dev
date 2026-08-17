# Stash (`sage-stash`)

`Stash` owns compact, heterogeneous trees of `Copy` values behind typed
handles. A handle is four bytes in release builds; debug builds add a
four-byte stash-identity tag for cross-stash diagnostics. Sage uses Stash for
per-item CST, checked signatures, types, and Typed IR: data which should move
through one semantic query result without becoming hundreds of separately
tracked Salsa entities.

## Representation contract

`Ptr<T>` identifies one value and `Slice<T>` identifies a contiguous sequence.
Both are valid only with the `Stash` that created them. Debug builds attach a
stash identity to catch cross-stash indexing; semantic code explicitly copies
between stashes when crossing a query boundary.

This is the ownership consequence of
[D2](./decisions.md#d2-stash-for-type-level-interning).

<a id="stash-a1"></a>
> **STASH-A1 — Handles never cross stash ownership implicitly.** A `Ptr<T>` or
> `Slice<T>` is interpreted only by its allocating `Stash`. Retaining a value
> across a stash boundary requires a structural copy into the destination; a
> checked output tree must not contain handles back into its source CST or an
> imported query result.
>
> **Required verification:** Debug tests reject cross-stash pointer and slice
> access for equal and unequal element types, copy tests reconstruct nested
> pointers and slices in a destination stash, and completed-query tests walk
> outputs under the destination stash without consulting source stashes.

All `alloc` and `alloc_slice` operations are hash-consing. Equal values of the
same stash data type return the same handle; hash collisions are resolved by
content comparison. Children are allocated before parents, so a parent's hash
can incorporate the already computed hashes of its handles without recursive
tree traversal.

<a id="stash-a2"></a>
> **STASH-A2 — Allocation is content-addressed within one stash.** Every value
> and slice allocation is hash-consed by its typed structural content. Equal
> content of the same stash data type yields the same handle, while hash
> collisions are resolved by actual content comparison rather than sharing.
>
> **Required verification:** Scalar, nested-pointer, slice, duplicate, and
> forced-collision tests establish deduplication and non-aliasing; derived
> `AllocStashData` implementations receive the same coverage as handwritten
> implementations.

`Stashed<T>` pairs a stash with a root value and precomputes a 128-bit
structural fingerprint by following that root. Equality, ordering, hashing,
and `salsa::Update` use the fingerprint. The fingerprint is deterministic for
equal rooted semantic content and ignores unreachable allocation history.

<a id="stash-a3"></a>
> **STASH-A3 — Query-result equality follows rooted semantic content.** The
> equality, ordering, hashing, and Salsa update behavior of `Stashed<T>` use a
> deterministic structural fingerprint of the reachable root, not stash
> addresses, allocation order, padding, or unreachable allocations.
>
> **Required verification:** Independently allocated equal and unequal trees,
> shared DAGs, reordered/unreachable allocations, and repeated construction
> produce the expected equality, hash, ordering, fingerprint, and
> `salsa::Update` behavior.

## Why this is the query boundary

A function body can contain hundreds of expressions, statements, patterns,
and types. Tracking each node independently would expose unstable internal
structure and add per-node Salsa overhead, while body consumers need the
completed tree as a unit. `Stashed<T>` gives Salsa one semantic value to
compare and backdate.

The source and destination stashes remain separate during checking:

```text
per-item CST Stashed<T> --read--> Check / InferCtx --allocate--> checked Stashed<U>
```

No checked pointer refers back into its source CST stash. Imports copy the
required semantic values into the destination before retaining their handles.

<a id="stash-a4"></a>
> **STASH-A4 — A stash-owned tree is one incremental value.** Salsa compares
> and backdates a `Stashed<T>` root as a unit; nodes below that root are not
> independent tracked identities. Finer incremental boundaries are introduced
> as semantic queries outside the tree, not by exposing its internal handles.
>
> **Required verification:** Query traces show equal reconstructed roots
> stopping downstream invalidation after a producer reexecutes, while an edit
> changing reachable semantic content changes the root and invalidates its
> consumers.

## Core traits

- `StashData` is the unsafe representation contract for stash-storable `Copy`
  types. Its `StaticSelf` supports runtime type checking across `'db`.
- `AllocStashData` adds structural hashing and equality required by
  hash-consing; derive it for ordinary stash nodes.
- `StashHash` hashes a value in its stash context.
- `StashCopy` reconstructs a value and its reachable children in another
  stash.

## Incremental behavior

Fingerprint equality is the incremental firewall at a stash-owned query
boundary. If a query reexecutes and reconstructs equal rooted semantic output,
`Stashed<T>::maybe_update` reports no change and Salsa can backdate the result.

This does not make the producing query itself immune to coarse inputs. A body
query may still reexecute because of a module- or file-level dependency; the
fingerprint only prevents an equal output from propagating further. See
[Incrementality and Query Boundaries](./infrastructure/incrementality.md).

## Code map

| Path | Responsibility |
|---|---|
| `crates/sage-stash/src/lib.rs` | typed handles, hash-consed allocation, fingerprints, copying, Salsa updates |
| `crates/sage-stash-macros/` | `AllocStashData` derive |
| `crates/sage-stash/tests/arena_tests.rs` | allocation, collision, fingerprint, copy, and ordering evidence |

## Current status

### Current frontier and evidence

Hash-consed values/slices, collision handling, deterministic rooted
fingerprints, cross-stash copying, and Salsa updates are operational across
CST, signatures, solver values, and Typed IR. Focused evidence includes:

- **[STASH-A1](#stash-a1):** `cross_stash_ptr_wrong_type`,
  `cross_stash_ptr_same_type_panics_in_debug`,
  `cross_stash_slice_panics_in_debug`, `copy_into_simple`,
  `copy_into_compound`, and `copy_into_with_slice`;
- **[STASH-A2](#stash-a2):** `hash_cons_dedup`, `hash_cons_slice_dedup`, and
  `hash_cons_collision_stores_both`, plus
  `derived_stash_hash_leaf_struct` and `derived_stash_hash_with_ptr_field`;
- **[STASH-A3](#stash-a3):** `stashed_eq_same_content_fingerprint`,
  `stashed_ne_different_content_fingerprint`,
  `stashed_hash_consistent_with_eq`, `stashed_ord_consistent_with_eq`,
  `stashed_eq_compound_dag`, and `fingerprint_deterministic`.

Run them with `cargo test -p sage-stash`.

### Current limitations

- Fingerprint equality is probabilistic at 128 bits; the architecture accepts
  that collision risk for incremental equality but semantic hash-consing still
  checks actual content before sharing a handle.
- Stash values are `Copy` and use a closed derive-supported storage model;
  owned variable-sized data must be represented through slices or interned
  semantic values.
- Focused stash tests establish selected value behavior for STASH-A1 through
  STASH-A3, but the reordered/unreachable-allocation matrix is not yet
  explicit and STASH-A1's completed-query ownership walk is not yet isolated.
  A query-level relevant/equal edit experiment directly establishing STASH-A4
  has not yet been isolated as stash-specific evidence.

### Related roadmap slices

The [Semantic Inspector and persistent edit-testing
slice](../implementation/roadmap.md#planned-slice-semantic-inspector-and-persistent-edit-testing)
will make the effect of equal/backdated stashed results visible in structured
traces. Application slices extend the set of tree nodes but do not change this
storage contract.
