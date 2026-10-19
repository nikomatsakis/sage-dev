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
between stashes when crossing a query boundary. The heterogeneous byte buffer
starts with `usize` alignment, tracks the strongest admitted type alignment,
and reallocates with that layout when it grows or encounters a more strongly
aligned type. Every typed entry address therefore satisfies
`align_of::<T>()` while handles remain compact byte-table indices.

```{anchor}
aligned_stash_buffer
```

This is the ownership consequence of
[Stash for type-level interning](../../../rfds/architectural-foundations/README.md#stash-for-type-level-interning).

{spec}`crates.sage_stash.storage.handle_ownership`

Verification scenarios: Debug tests reject cross-stash pointer and slice
access for equal and unequal element types, copy tests reconstruct nested
pointers and slices in a destination stash, and completed-query tests walk
outputs under the destination stash without consulting source stashes.

All `alloc` and `alloc_slice` operations are hash-consing. Equal values of the
same stash data type return the same handle; hash collisions are resolved by
content comparison. Lookup keys include type, element count, and content hash,
so a scalar collision cannot read an empty slice entry as a value.
Children are allocated before parents, so a parent's hash
can incorporate the already computed hashes of its handles without recursive
tree traversal.

{spec}`crates.sage_stash.storage.typed_interning`

Verification scenarios: Scalar, nested-pointer, slice, duplicate, and
forced-collision tests establish deduplication and non-aliasing; derived
`AllocStashData` implementations receive the same coverage as handwritten
implementations.

Allocation may grow the arena, but it never changes an existing entry.
Algorithms which need mutable state keep that state outside the stash and
publish a result by structurally copying it into a fresh stash. In particular,
body inference records type equalities in its egraph; completion resolves each
type while rebuilding the Typed IR, rather than overwriting inference-variable
entries which may already participate in hash-consing.

```{anchor}
finalize_typed_body_into_fresh_stash
```

This is the immutability consequence of
[Stash for type-level interning](../../../rfds/architectural-foundations/README.md#stash-for-type-level-interning).

{spec}`crates.sage_stash.storage.immutable_entries`

Verification scenarios: The public stash API exposes no mutable indexing
or mutable entry access; a focused finalization test proves that the working
stash retains its inference-variable node while the separately owned output
contains only its resolved type; completed-body tests reject every surviving
inference variable.

`Stashed<T>` pairs a stash with a root value and precomputes a 128-bit
structural fingerprint by following that root. Equality, ordering, hashing,
and `salsa::Update` use the fingerprint. The fingerprint is deterministic for
equal rooted semantic content and ignores unreachable allocation history.

{spec}`crates.sage_stash.storage.rooted_equality`

Verification scenarios: Independently allocated equal and unequal trees,
shared DAGs, reordered/unreachable allocations, and repeated construction
produce the expected equality, hash, ordering, fingerprint, and
`salsa::Update` behavior.

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

{spec}`crates.sage_stash.storage.query_value`

Verification scenarios: Query traces show equal reconstructed roots
stopping downstream invalidation after a producer reexecutes, while an edit
changing reachable semantic content changes the root and invalidates its
consumers.

## Core traits

- `StashData` is the unsafe representation contract for stash-storable
  `Copy + Send + Sync` types. Its `StaticSelf` supports runtime type checking
  across `'db`; the thread-safety bounds preserve `Stash`'s type-erased
  `Send + Sync` contract.
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
[Incrementality and Query Boundaries](../sage_ir/incrementality-guide.md).

## Code map

| Path | Responsibility |
|---|---|
| `crates/sage-stash/src/lib.rs` | typed handles, hash-consed allocation, fingerprints, copying, Salsa updates |
| `crates/sage-stash-macros/` | `AllocStashData` derive |
| `crates/sage-stash/tests/arena_tests.rs` | allocation, collision, fingerprint, copy, and ordering evidence |
| `crates/sage-stash/tests/storage_contracts.rs` | nested ownership, collision boundaries, immutable API, and rooted value evidence |
| `crates/sage-stash/tests/incremental.rs` | real Salsa producer/consumer reuse and backdating |
| `crates/sage-stash/tests/ui/` | rejected attempts to mutate interned scalar and slice entries |

## Implementation and test evidence

The generated [anchor pages](../../anchor/crates/sage_stash/storage.html) report implementation entries,
annotated tests, explicit TODO/N/A gaps, and saved claim/test reviews. These
associations do not themselves establish that a test ran.

The [earlier coverage snapshot](../../../rfds/implementation-sequencing/coverage-history.md#stash-sage-stash)
preserves the former capability descriptions, edit experiments, and limitations
as historical evidence. Current coverage comes from the generated entries;
implementation plans and decision rationale belong in the relevant RFDs.
