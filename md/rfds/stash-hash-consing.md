# RFD: Hash-consed stash with fingerprinted equality

**Status:** Implemented

**Depends on:**
- [Module sym tree](./module-sym-tree.md) — stash architecture, `Stashed<T>`

## Background

A `Stash` is a type-erased arena for `Copy` data, accessed via typed thin handles (`Ptr<T>`, `Slice<T>`). A `Stashed<T>` pairs a `Stash` with a root value of type `T`, forming a self-contained unit suitable for caching (e.g., as a salsa return value).

## Goal

Make stash equality correct and efficient by:
1. Hash-consing all allocations so that equal content always produces equal `Ptr` indices within a stash.
2. Computing fingerprints at `Stashed<T>` construction for cross-stash equality.

## Problem

### Byte-level `Stashed` equality is fragile

`Stash::PartialEq` compares `self.buf == other.buf`. This works only because today the byte layout is deterministic for identical allocation sequences. But it would break if:
- Handle metadata (like a stash ID) is embedded in `Ptr`/`Slice`
- Allocation order or padding changes between otherwise-equivalent stashes
- Any non-content bytes end up in the buffer

### Byte-level hashing and comparison are unsound in the presence of padding

Struct padding bytes are not preserved across typed moves/copies (the compiler may leave them as garbage). Raw byte hashing or comparison of stash-allocated data would be non-deterministic for types with padding. This rules out `memcmp` and byte-level hashing as correctness strategies, even if padding is zeroed at allocation time.

## Design

### Part 1: Hash-consing all allocations

Unify `alloc` and `intern` into a single content-addressed path. Every allocation hashes the value, checks for an existing entry with matching content, and returns the existing `Ptr` if found.

**Key invariant:** equal content always produces equal `Ptr` indices within a stash. This means `Ptr<T>: PartialEq` on index alone is a correct deep equality check. Standard derived `PartialEq` on compound types works: `Ptr` fields compare by index, scalars compare normally.

**Hashing uses `StashHash`, not raw bytes.** Struct padding makes byte-level hashing unsound. All hashing goes through the `StashHash` trait (field-by-field), which is immune to padding by construction. Collision resolution during interning uses `PartialEq` (also field-by-field). `StashEq` is no longer needed and can be removed.

#### `StashHasher` trait

```rust
pub trait StashHasher: Hasher {
    fn stash_hash_ptr<T: StashHash>(&mut self, ptr: Ptr<T>, stash: &Stash);
    fn stash_hash_slice<T: StashHash>(&mut self, slice: Slice<T>, stash: &Stash);
}
```

`StashHasher` extends `std::hash::Hasher`, so types that only implement `Hash` work transparently — `Hash::hash` only ever calls `write_*` methods, never `finish()`. `StashDirect` is a marker trait for types whose `Eq`/`Hash` don't need stash context (scalars like `u32`, `bool`, salsa interned IDs, etc.).

`StashHash` impls are written once against this trait:

```rust
pub trait StashHash {
    fn stash_hash(&self, stash: &Stash, hasher: &mut impl StashHasher);
}
```

Blanket impl for `StashDirect` types:

```rust
impl<T: Hash + StashDirect> StashHash for T {
    fn stash_hash(&self, _stash: &Stash, hasher: &mut impl StashHasher) {
        self.hash(hasher); // works because StashHasher: Hasher
    }
}
```

`Ptr<T>` and `Slice<T>` delegate to the hasher, which controls the recursion strategy:

```rust
impl<T: StashHash + StashData> StashHash for Ptr<T> {
    fn stash_hash(&self, stash: &Stash, hasher: &mut impl StashHasher) {
        hasher.stash_hash_ptr(*self, stash);
    }
}
```

#### `StashHasher` implementations

**`InternHasher<H: Hasher>`** — adapts any `Hasher` into a `StashHasher`. `stash_hash_ptr` reads the child entry's inline FxHash (a `u64` stored alongside each entry in the buffer — see Buffer Layout below) and writes those bytes into the wrapped hasher. `stash_hash_slice` does the same for slice entries. No recursion — children are always allocated before parents, so their inline hashes are already available. Used at allocation time, wrapping `FxHasher`.

**`FingerprintHasher`** — wraps a fingerprint-quality hasher (currently XXH3-128) plus a per-entry fingerprint cache. `stash_hash_ptr` checks the cache by entry index; on miss, creates a fresh sub-hasher, recurses into the child, finalizes to get the child's fingerprint, caches it, then writes the fingerprint bytes into the parent's hasher. `stash_hash_slice` works the same way. Caching avoids re-traversing shared DAG nodes. Used at `Stashed` construction.

#### Buffer layout

Each entry in the stash buffer stores the data followed by metadata at the tail:

```
[data: size_of::<T>() bytes] [hash: u64] [next_offset: u32]?
```

Data is always at byte offset 0 of the entry, so `stash[ptr]` is a direct pointer cast with no offset arithmetic. The `hash` is the FxHash computed at allocation time via `InternHasher<FxHasher>`. The `u64` hash may be unaligned (if `size_of::<T>()` is not a multiple of 8) and is read via `u64::from_ne_bytes()`. The optional `next_offset` forms a collision chain when multiple entries share the same FxHash.

**Alignment.** `push_raw` enforces a minimum alignment of `max(4, align_of::<T>())`. Most stash-allocated types already have alignment >= 4. This minimum alignment guarantees that byte offsets into the buffer are always 4-byte aligned, freeing the low bit for use as a tag.

**Index vs offset newtypes.** `Ptr<T>` stores an *entry index* (index into the `entries: Vec<Entry>` table, used for type-safety validation). The intern hashmap and collision chain use *byte offsets* into the buffer. These are distinct newtypes to prevent confusion. `EntryIndex` stores the index + 1 as a `NonZeroU32`, so `Option<Ptr<T>>` is a single word with niche optimization (zero represents `None`). This also eliminates the need for `Ptr::DANGLING`.

```rust
/// Index into the entries table. Stored in `Ptr<T>` and `Slice<T>`.
/// Stores index + 1 as NonZeroU32 so that Option<Ptr<T>> has niche optimization.
struct EntryIndex(NonZeroU32);

/// Byte offset into the stash buffer. Used by intern hashmap and collision chains.
/// Always 4-byte aligned, so the low bit is available as a tag.
struct BufOffset(u32);
```

**Intern hashmap and collision chains.** The intern hashmap maps `(TypeId, u64) -> BufOffset` — keyed by both type and hash, since different types can produce the same FxHash. The low bit of the `BufOffset` is a tag:
- `offset | 0`: sole entry for this hash — layout is `[data..., hash:u64]`
- `offset | 1`: chain head — layout is `[data..., hash:u64, next_offset:u32]`

The chain is a singly-linked list threaded through the buffer. New entries prepend to the chain; existing entries are never rewritten, so existing `Ptr`/`Slice` values stay valid. On collision, the new entry stores a `next_offset` pointing to the previous head; the previous head's layout remains unchanged (it becomes the chain tail, tagged with `| 0`).

```
hashmap[H] = offset | 0  ->  buffer: [data..., hash:u64]                       (sole entry)
hashmap[H] = offset | 1  ->  buffer: [data..., hash:u64, next_offset:u32]      (chain head)
                                                              |
                               next | 0  ->  [data..., hash:u64]               (chain tail)
                               next | 1  ->  [data..., hash:u64, next_offset]  (chain middle)
```

**Slice entries.** Slices are hash-consed as a unit. The `StashHash` for a slice hashes the element count followed by each element's `StashHash`. The slice's element count is stored in the `Entry` table (as today), not inline in the buffer. Collision resolution compares slices element-by-element via `PartialEq`.

#### Derive macro changes

The `AllocStashData` derive generates `StashHash` impls following a field-by-field pattern: scalar fields call `self.field.hash(hasher)`, `Ptr` fields call `hasher.stash_hash_ptr(...)`, `Slice` fields call `hasher.stash_hash_slice(...)`. Since hash-consing requires `StashHash` for all allocated types, this is bundled into the existing derive rather than a separate one.

### Part 2: Cross-stash fingerprinting for `Stashed`

At `Stashed<T>` construction, compute a fingerprint over the root value using `FingerprintHasher`:

```rust
let mut hasher = FingerprintHasher::new();
root.stash_hash(&stash, &mut hasher);
let fingerprint = hasher.finalize();
Stashed { stash, root, fingerprint }
```

The fingerprint is an opaque type:

```rust
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Fingerprint([u8; 16]); // XXH3-128 for now
```

The fingerprint is the sole basis for all `Stashed` comparisons — no trait impl on `Stashed` ever walks the `T` value or accesses the stash:

```rust
impl<T> PartialEq for Stashed<T> {
    fn eq(&self, other: &Self) -> bool {
        self.fingerprint == other.fingerprint
    }
}

impl<T> Hash for Stashed<T> {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.fingerprint.hash(state);
    }
}

impl<T> Ord for Stashed<T> {
    fn cmp(&self, other: &Self) -> Ordering {
        self.fingerprint.cmp(&other.fingerprint)
    }
}
```

Note: the fingerprint-based `Ord` is consistent but non-semantic — it has no relationship to the logical content ordering. This is by design; it exists only for use in ordered collections like `BTreeMap`.

**Algorithm choice.** The current choice is XXH3-128: fast on small payloads (which is the typical case — type tree nodes are small structs hashed individually), 128-bit collision resistance (sufficient for non-adversarial compiler internals). The algorithm is not hardcoded — changing to BLAKE3-256 or another hash only requires changing `FingerprintHasher` internals and the `Fingerprint` array size. `StashHash` impls are unaffected.

**Lazy computation.** The fingerprint is computed at `Stashed` construction, not during allocation. Most stash entries never become `Stashed` roots. The `FingerprintHasher` walks the reachable tree from the root, caching per-entry fingerprints (e.g., in an `FxHashMap<u32, Fingerprint>` or a `Vec<Option<Fingerprint>>` indexed by entry) to avoid re-traversing shared DAG substructure.

## Implementation notes

**Buffer layout deviation.** The design above specifies inline hashes embedded in the byte buffer with collision chains threaded via tagged `BufOffset`s. The implementation instead stores inline hashes in the `Entry` struct (same 8 bytes per entry, different location) and uses a `HashMap<(TypeId, u64, collision_index), EntryIndex>` for collision resolution. This is simpler but uses more memory per collision. See [Faster collision chains](./stash-faster-collision-chains.md) for the follow-up proposal to adopt the buffer-threaded design.

**Derive macro calls `StashHash` for all fields.** The design says scalar fields call `self.field.hash(hasher)` and `Ptr`/`Slice` fields delegate to `stash_hash_ptr`/`stash_hash_slice`. The implementation calls `StashHash::stash_hash` uniformly for all fields — no type classification in the macro. This works because `StashHasher: Hasher` and the `StashDirect` blanket bridges `Hash` to `StashHash` for scalars.

**`InternStashData` removed.** With all allocations hash-consed, the `alloc`/`intern` distinction is gone. `InternStashData` was removed; `AllocStashData` now requires `StashHash + PartialEq`. The `InternStashData` derive macro is kept as a legacy alias.

## Implementation plan

Each phase compiles and passes tests independently.

### Phase 1: `StashHasher` trait and `StashHash` migration

**Changes:**
- Add the `StashHasher` trait (extending `Hasher`) to `sage-stash`.
- Add `InternHasher<H>` — for now, a thin wrapper that panics on `stash_hash_ptr`/`stash_hash_slice` (no inline hashes exist yet). This lets the trait compile and be used for leaf types.
- Change `StashHash` from `fn stash_hash<H: Hasher>(... state: &mut H)` to `fn stash_hash(... hasher: &mut impl StashHasher)`.
- Update the blanket `StashDirect` impl, the manual `Ptr<T>`/`Slice<T>`/`Option<T>` impls, and all callers.
- `Ptr<T>`/`Slice<T>` `StashHash` impls delegate to `hasher.stash_hash_ptr()`/`hasher.stash_hash_slice()` (which panic for now — they aren't called until Phase 3).

**Tests:**
- `stash_hasher_scalar`: hash a `u32` through `InternHasher<FxHasher>`, verify it produces the same value as hashing directly through `FxHasher`.
- `stash_hasher_stash_direct`: hash a `StashDirect` type through `InternHasher`, verify it delegates to `Hash`.

### Phase 2: Derive macro generates `StashHash`

**Changes:**
- Extend `AllocStashData` (and `InternStashData`) derive to emit a `StashHash` impl for the type. For each field: if the field type is `Ptr<T>`, emit `hasher.stash_hash_ptr(self.field, stash)`; if `Slice<T>`, emit `hasher.stash_hash_slice(self.field, stash)`; otherwise emit `self.field.hash(hasher)`.
- This requires the derive macro to distinguish `Ptr`/`Slice` fields from others. Strategy: match on the path segments of the field type (the same approach used by serde and similar derives).

**Tests:**
- `derived_stash_hash_leaf_struct`: define a test struct with only scalar fields (e.g., `Point { x: i32, y: i32 }`), hash it through `InternHasher<FxHasher>`, verify the result matches hashing each field sequentially through `FxHasher`.
- `derived_stash_hash_with_ptr_field`: define a struct with a `Ptr` field. Constructing the `StashHash` impl should compile (the `stash_hash_ptr` call won't execute in this test since we don't call it).

### Phase 3: `Ptr` as `NonZeroU32`, `EntryIndex`/`BufOffset` newtypes, `DANGLING` removal

**Changes:**
- Introduce `EntryIndex(NonZeroU32)` newtype (stores index + 1). Change `Ptr<T>` and `Slice<T>` to use `EntryIndex` instead of raw `u32`.
- Remove `Ptr::DANGLING`.
- Introduce `BufOffset(u32)` newtype for buffer byte offsets.
- Update `Entry`, `push_raw`, `Index` impls, and all internal code to use the newtypes.

**Tests:**
- `option_ptr_size`: `assert_eq!(size_of::<Option<Ptr<T>>>(), size_of::<Ptr<T>>())` — verifies niche optimization.
- All existing alloc/intern/index tests continue to pass (regression).

### Phase 4: Buffer layout and hash-consed allocation

**Changes:**
- Change `push_raw` to enforce `max(4, align_of::<T>())` minimum alignment.
- Change buffer layout: append the FxHash (`u64`) at the tail of each entry after the data. Store `BufOffset` in each `Entry` for the inline hash location (or compute from entry offset + data size).
- Build `InternHasher<FxHasher>` to read child inline hashes: `stash_hash_ptr` looks up the entry by `Ptr` index, reads the `u64` at the tail of that entry, writes those bytes. `stash_hash_slice` does the same.
- Unify `alloc` and `intern` into a single method (e.g., `add` / `add_slice`). Every allocation: compute `StashHash` via `InternHasher<FxHasher>`, look up `(TypeId, hash)` in the intern hashmap, walk the collision chain comparing via `PartialEq`, return existing `Ptr` on match or allocate and insert on miss.
- Add the collision chain structure (low-bit tagging on `BufOffset`, optional `next_offset` at the tail).
- Update the `InternStashData` / `AllocStashData` trait bounds: both now require `StashHash + PartialEq`. The distinction between `alloc` and `intern` disappears — keep the old method names as aliases during migration if needed.

**Tests:**
- `hash_cons_dedup`: allocate the same `Point` value twice via the new unified path, assert the two `Ptr`s are equal (same index). This is the core hash-consing invariant — currently this test would *fail* with the old `alloc`.
- `hash_cons_distinct`: allocate two different `Point` values, assert the `Ptr`s differ.
- `hash_cons_slice_dedup`: allocate the same slice twice, assert the `Slice`s are equal.
- `hash_cons_slice_distinct`: allocate different slices, assert the `Slice`s differ.
- `hash_cons_compound_type`: define a struct with `Ptr` fields (e.g., `Pair { a: Ptr<Point>, b: Ptr<Point> }`). Allocate children first, then the parent twice with the same children — assert the parent `Ptr`s are equal.
- `hash_cons_collision_chain`: force a hash collision (e.g., two types that produce the same FxHash — can be done by mocking or by finding a natural collision with crafted field values). Verify both values are stored and retrievable.
- `intern_hasher_reads_inline_hash`: allocate a value, then hash it through `InternHasher<FxHasher>`, verify `stash_hash_ptr` returns the stored inline hash.
- Update existing test `alloc_same_value_produces_distinct_ptrs` — this now produces *equal* ptrs (hash-consing). Rename to `alloc_same_value_deduplicates`.
- Existing `intern_*` tests continue to pass unchanged.
- Existing index/read-back tests continue to pass unchanged.

### Phase 5: Remove `StashEq` and `StashOrd`

**Changes:**
- Remove the `StashEq`, `StashOrd` traits and all their impls (`Ptr<T>`, `Slice<T>`, `Option<T>`, `StashDirect` blanket).
- Remove any callers of `stash_eq` / `stash_cmp` (grep the full codebase). With hash-consing, standard `PartialEq` (index-based for `Ptr`/`Slice`, derived for compounds) is correct.
- Remove the `StashDirect` blankets for `StashEq`/`StashOrd` (keep the `StashHash` blanket and the marker trait).

**Tests:**
- Remove `stash_eq_*` tests that tested the old `StashEq` trait.
- `ptr_eq_is_value_eq`: allocate two identical values, get two `Ptr`s, assert `p1 == p2` (index equality = value equality, the hash-consing guarantee).
- `ptr_ne_is_value_ne`: allocate two different values, assert `p1 != p2`.
- Full crate test suite passes.

### Phase 6: `Fingerprint`, `FingerprintHasher`, and `Stashed` equality

**Changes:**
- Add `xxhash-rust` dependency (with `xxh3` feature) to `sage-stash`.
- Add the `Fingerprint([u8; 16])` type with `Clone`, `PartialEq`, `Eq`, `PartialOrd`, `Ord`, `Hash`.
- Add `FingerprintHasher` wrapping `XxHash128` + per-entry fingerprint cache. `stash_hash_ptr` checks cache by entry index; on miss, creates a sub-hasher, recurses, caches, writes fingerprint bytes into parent.
- Add `fingerprint` field to `Stashed<T>`. Change `Stashed::new` to accept `T: StashHash`, compute the fingerprint via `FingerprintHasher`, store it.
- Change `Stashed::PartialEq` to compare fingerprints.
- Change `Stashed::Hash` to hash the fingerprint.
- Add `Stashed::Ord` comparing fingerprints.
- Remove `Stash::PartialEq` and `Stash::Hash` (byte-level) — no longer used.
- Update `salsa::Update` impl for `Stashed<T>` if needed (it uses `PartialEq`, which now compares fingerprints).

**Tests:**
- `stashed_eq_same_content`: build two `Stashed<Ptr<Point>>` from independent stashes with the same logical content, assert equal. (Replaces the existing test, which relied on byte-level equality.)
- `stashed_ne_different_content`: build two with different content, assert not equal.
- `stashed_hash_consistent_with_eq`: two equal `Stashed` values produce the same `Hash` output.
- `stashed_ord_consistent_with_eq`: two equal `Stashed` values compare `Ordering::Equal`.
- `stashed_eq_compound_dag`: build a `Stashed` with a compound type that has shared DAG structure (same child `Ptr` referenced from multiple parents). Build the same structure in a second stash. Assert fingerprints are equal — verifying the `FingerprintHasher` cache handles DAG sharing correctly.
- `stashed_eq_deep_tree`: build a `Stashed` with a deeply nested type (Ptr to struct containing Ptr to struct...). Assert two independent constructions are equal — exercises the recursive fingerprinting.
- `fingerprint_deterministic`: compute the fingerprint of the same `Stashed` value twice, assert identical. (Sanity check that the hasher is deterministic.)
- Existing `stashed_eq_*` and `stashed_slice_eq` tests continue to pass (behavior unchanged, implementation changed).

## Scope and non-goals

**In scope:**
- Hash-consing all stash allocations
- `StashHasher` trait and implementations (`InternHasher`, `FingerprintHasher`)
- `Fingerprint` type and `Stashed::PartialEq`/`Hash`/`Ord` rewrite
- Removal of `StashEq` (superseded by hash-consing + standard `PartialEq`)

**Out of scope (deferred to follow-up RFDs):**
- Debug-mode stash identity tagging on `Ptr`/`Slice` to catch cross-stash misuse — see [stash-safety](./stash-safety.md)
- Convenience APIs for cross-stash copying (`copy_into`, `instantiate_into`)
- Buffer-threaded collision chains — see [stash-faster-collision-chains](./stash-faster-collision-chains.md)
- Hardcoding a specific fingerprint algorithm — the design is algorithm-agnostic

## Open questions

1. **Fingerprint algorithm.** XXH3-128 is the initial choice (fast on small payloads, 128-bit collision resistance). If collision concerns arise, switching to BLAKE3-256 is a one-line change in `FingerprintHasher` and a size change in `Fingerprint`. The `StashHash` impls are unaffected.
