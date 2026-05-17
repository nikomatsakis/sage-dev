# RFD: Faster intern collision chains

**Status:** Proposed

**Depends on:**
- [Hash-consed stash](./stash-hash-consing.md) — current hash-consing implementation

## Background

The hash-consing implementation uses a `HashMap<InternKey, EntryIndex>` for deduplication, where `InternKey` is:

```rust
struct InternKey {
    type_id: TypeId,
    content_hash: u64,
    collision: u32,
}
```

On hash collision, the code increments `collision` and performs successive HashMap lookups until it finds a vacant slot. This means N collisions for the same `(type_id, content_hash)` produce N separate HashMap entries, each carrying the full per-bucket overhead of the HashMap.

## Proposed improvement

Replace the collision counter with a singly-linked list threaded through the `Entry` table. The HashMap key becomes `(TypeId, u64)` — no collision field — and maps to a single `EntryIndex`. Collisions are resolved by following `next` links in the entries:

```rust
struct Entry {
    type_id: TypeId,
    offset: u32,
    count: u32,
    inline_hash: u64,
    next: Option<EntryIndex>,  // collision chain
}
```

On lookup: hash the value, find the HashMap entry, then walk the linked list comparing via `PartialEq` until a match is found or the chain ends.

On insert with collision: allocate the new entry, set its `next` to the current head, update the HashMap to point to the new entry (prepend to chain).

## Trade-offs

**Wins:**
- One HashMap entry per distinct hash, not per distinct value — reduces HashMap memory and probe cost when collisions occur.
- Removes the 32-bit `collision` field from the HashMap key, shrinking each key by 4 bytes.

**Costs:**
- Adds an `Option<EntryIndex>` (4 bytes, niche-optimized) to every `Entry`, even entries that never collide. Net: same 4 bytes saved from removing `collision` in the key, so roughly neutral per-entry.
- Slightly more complex insertion logic (linked-list prepend vs. incrementing a counter).

## Why deferred

The current design is correct and collisions are expected to be rare for the typical workload (small compiler IR nodes). The improvement matters most when the intern map grows large or collision rates are non-trivial. Worth revisiting if profiling shows the HashMap is a memory or lookup bottleneck.
