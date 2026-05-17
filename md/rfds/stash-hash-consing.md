# RFD: Hash-consed stash with fingerprinted equality

**Status:** Draft (revised)

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

## Current state (partially begun)

- The existing test `cross_stash_ptr_same_type_reads_wrong_data` documents current bad behavior (silently reading wrong data from the wrong stash).
- The `StashHash` trait exists with manual impls for `Ptr<T>`, `Slice<T>`, `Option<T>`, and blanket impls for `StashDirect` types. The derive macro does not yet generate `StashHash` impls for compound types — this is new work required by this design.

## Implementation plan

### Step 1: `StashHasher` trait and hash-consed allocation

Add the `StashHasher` trait extending `Hasher`. Implement `InternHasher<H>`. Update `StashHash` to use `impl StashHasher` instead of `impl Hasher`. Update the `AllocStashData` derive to generate `StashHash` impls against `StashHasher`.

Unify `alloc` and `intern` into a single hash-consing path. Add inline FxHash storage at the tail of each buffer entry. Add the collision chain structure. Enforce minimum 4-byte alignment in `push_raw`.

Remove `StashEq` and `StashOrd` — standard `PartialEq`/`Ord` on index-based `Ptr` equality is now correct.

Verify all existing tests pass.

### Step 2: `Fingerprint` and `Stashed` equality

Add `FingerprintHasher` using XXH3-128 with per-entry caching. Add the `Fingerprint` type. Compute fingerprints at `Stashed` construction. Change `Stashed::PartialEq`, `Stashed::Hash`, and `Stashed::Ord` to use the fingerprint exclusively.

Remove `Stash::PartialEq` (byte comparison) — it is no longer used.

## Scope and non-goals

**In scope:**
- Hash-consing all stash allocations
- `StashHasher` trait and implementations (`InternHasher`, `FingerprintHasher`)
- `Fingerprint` type and `Stashed::PartialEq`/`Hash`/`Ord` rewrite
- Removal of `StashEq` (superseded by hash-consing + standard `PartialEq`)

**Out of scope (deferred to a follow-up RFD):**
- Debug-mode stash identity tagging on `Ptr`/`Slice` to catch cross-stash misuse
- Convenience APIs for cross-stash copying (`copy_into`, `instantiate_into`)
- Hardcoding a specific fingerprint algorithm — the design is algorithm-agnostic

## Open questions

1. **Fingerprint algorithm.** XXH3-128 is the initial choice (fast on small payloads, 128-bit collision resistance). If collision concerns arise, switching to BLAKE3-256 is a one-line change in `FingerprintHasher` and a size change in `Fingerprint`. The `StashHash` impls are unaffected.
