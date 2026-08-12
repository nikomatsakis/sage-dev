# Stash (`sage-stash`)

`Stash` owns compact, heterogeneous trees of `Copy` values behind typed
four-byte handles. Sage uses it for per-item CST, checked signatures, types,
and Typed IR: data which should move through one semantic query result without
becoming hundreds of separately tracked Salsa entities.

## Representation contract

`Ptr<T>` identifies one value and `Slice<T>` identifies a contiguous sequence.
Both are valid only with the `Stash` that created them. Debug builds attach a
stash identity to catch cross-stash indexing; semantic code explicitly copies
between stashes when crossing a query boundary.

All `alloc` and `alloc_slice` operations are hash-consing. Equal values of the
same stash data type return the same handle; hash collisions are resolved by
content comparison. Children are allocated before parents, so a parent's hash
can incorporate the already computed hashes of its handles without recursive
tree traversal.

`Stashed<T>` pairs a stash with a root value and precomputes a 128-bit
structural fingerprint by following that root. Equality, ordering, hashing,
and `salsa::Update` use the fingerprint. The fingerprint is deterministic for
equal rooted semantic content and ignores unreachable allocation history.

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

## Core traits

- `StashData` is the unsafe representation contract for stash-storable `Copy`
  types. Its `StaticSelf` supports runtime type checking across `'db`.
- `AllocStashData` adds structural hashing and equality required by
  hash-consing; derive it for ordinary stash nodes.
- `StashHash` hashes a value in its stash context.
- `StashCopy` reconstructs a value and its reachable children in another
  stash.
- `StashEq` and `StashOrd` support contextual structural comparisons when a
  value is not already wrapped in `Stashed<T>`.

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

- `hash_cons_dedup` and `hash_cons_slice_dedup`;
- `hash_cons_collision_stores_both`;
- `stashed_eq_same_content_fingerprint` and
  `stashed_ne_different_content_fingerprint`; and
- `fingerprint_deterministic`.

Run them with `cargo test -p sage-stash`.

### Current limitations

- Fingerprint equality is probabilistic at 128 bits; the architecture accepts
  that collision risk for incremental equality but semantic hash-consing still
  checks actual content before sharing a handle.
- Stash values are `Copy` and use a closed derive-supported storage model;
  owned variable-sized data must be represented through slices or interned
  semantic values.
- Fine-grained edits below a `Stashed<T>` root are not independently tracked.

### Related roadmap slices

The [Semantic Inspector and persistent edit-testing
slice](../implementation/roadmap.md#planned-slice-semantic-inspector-and-persistent-edit-testing)
will make the effect of equal/backdated stashed results visible in structured
traces. Application slices extend the set of tree nodes but do not change this
storage contract.
