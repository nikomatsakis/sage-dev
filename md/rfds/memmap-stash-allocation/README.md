# RFD: Stash-allocated MEM-map entries

**Status:** Completed (historical; storage mechanism superseded)

**Depends on:**
- [Hash-consed stash](./stash-hash-consing.md) — `Stash`, `Slice<T>`, `Stashed<T>`, `AllocStashData`

> **Current disposition.** This RFD records an intermediate representation
> which was completed and later replaced. Its fixpoint originally mutated
> hash-consed stash entries in place. That mechanism is superseded by
> [D2](../../design/decisions.md#d2-stash-for-type-level-interning) and
> [STASH-A5](../../design/stash.md#stash-a5): allocated stash entries are now
> immutable. Current module expansion returns an ordinary ordered symbol
> sequence and lets Salsa perform fixed-point recovery; see
> [Module and Macro Expansion](../../design/pipeline/module-expansion.md).

## Background

The MEM-map (`ExpandedModule`) stores a module's resolved/expanded member entries. Currently it uses heap-allocated `Vec`s throughout:

- `ExpandedModule.entries: Vec<MemmapEntry<'db>>`
- `MemmapEntry::Redirect { target: Vec<Name<'db>> }`
- `MemmapEntry::Glob { path: Vec<Name<'db>> }`
- `MacroUse { path: Vec<Name<'db>>, expansions: Vec<Expansion<'db>> }`
- `Expansion { entries: Vec<MemmapEntry<'db>> }`

This is a recursive tree of heap allocations. Comparing two memmaps (needed by salsa for incremental reuse) requires deep traversal.

## Goal

Replace all `Vec`s with stash-allocated `Slice`s, making `MemmapEntry` a `Copy` type stored in a single `Stash`. The `ExpandedModule` becomes:

```rust
#[salsa::tracked(debug)]
pub struct ExpandedModule<'db> {
    #[returns(ref)]
    pub entries: Stashed<Slice<MemmapEntry<'db>>>,
}
```

Benefits:
1. `MemmapEntry` is `Copy` — no cloning during resolution walks.
2. `Stashed` equality is O(1) fingerprint comparison — salsa can cheaply detect unchanged memmaps.
3. All data is in one contiguous allocation — better cache locality.
4. Hash-consing deduplicates identical subtrees (e.g., common paths).

## Design

### Data types

```rust
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, AllocStashData)]
pub enum MemmapEntry<'db> {
    Item(ItemAst<'db>),
    TupleStructCtor(LocalStructSym<'db>),
    MacroDef(LocalMacroDefSym<'db>),
    Redirect { name: Name<'db>, target: Slice<Name<'db>> },
    Glob { path: Slice<Name<'db>> },
    MacroUse(MacroUse<'db>),
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, AllocStashData)]
pub struct MacroUse<'db> {
    pub path: Slice<Name<'db>>,
    pub input: MacroInput<'db>,
    pub expansions: Slice<Expansion<'db>>,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, AllocStashData)]
pub struct Expansion<'db> {
    pub callee: MacroCallee<'db>,
    pub entries: Slice<MemmapEntry<'db>>,
}

pub type Memmap<'db> = Stashed<Slice<MemmapEntry<'db>>>;
```

### Historical stash extensions

1. **Mutable indexing (removed)** — the intermediate implementation allowed
   the fixpoint loop to replace a `MacroUse.expansions` slice handle in place.
   The append-only stash contract now forbids this API.

2. **`Stash::append_one` (or similar)** — allocates a new slice consisting of an existing slice's contents plus one appended element. Used to grow `MacroUse.expansions` when a new callee is discovered.

3. **Empty slices are free** — `alloc_slice(&[])` is hash-consed, so all empty slices of the same type share one entry. This is already the case.

### Historical fixpoint loop strategy

The expansion loop operates on a `Stash` + root `Slice<MemmapEntry>`:

1. **Seeding** produces an initial `Stash` with all entries (paths already as `Slice<Name>`). `MacroUse` entries start with `expansions` pointing to an empty slice.

2. **Each pass** walked and mutated the entries, resolved macro paths, and
   expanded callees. When a new expansion was discovered:
   - Allocate the expansion's child entries into the stash → get a `Slice<MemmapEntry>`
   - Build an `Expansion { callee, entries }` and allocate a new expansions slice = old contents + new element via `append_one`
   - Mutate the `MacroUse` in place to point `expansions` at the new slice

3. **Convergence** — when no pass discovers new callees, wrap the stash as `Stashed::new(stash, root)`.

The common case (exactly one expansion per macro use) means one `append_one` call per macro — growing from empty to a 1-element slice. Multiple expansions (ambiguity) are rare and just do another `append_one`.

### Consumers

Code that reads from the memmap (`resolve_member_impl`, `walk_entries`, `validate`, `resolve_path`) currently takes `&[MemmapEntry]`. After this change, it will receive a `&Stash` plus a `Slice<MemmapEntry>` and index into the stash. The recursive walk pattern changes from:

```rust
for entry in entries { ... }
```

to:

```rust
for entry in &stash[slice] { ... }
```

Functions need access to the stash to dereference nested `Slice` fields (e.g., `Redirect.target`, `MacroUse.expansions`). The stash reference is threaded through or obtained from the `Stashed` wrapper.

## Historical implementation plan

1. Add mutable indexing and `append_one` to `sage-stash` (the mutable indexing
   portion was later removed by STASH-A5).
2. Update `MemmapEntry`, `MacroUse`, `Expansion` to stash-allocated `Copy` types.
3. Update `ExpandedModule` to hold `Memmap<'db>` (= `Stashed<Slice<MemmapEntry>>`).
4. Update seeding (`seed.rs`) to allocate into a `Stash`.
5. Update the fixpoint loop (`expand.rs`) to operate on `&mut Stash` + root slice.
6. Update consumers (`resolve/mod.rs`, `validate.rs`, `resolve_path.rs`) to thread a stash reference.
7. Update tests.

## Open questions

- Naming: `append_one` vs `push_slice` vs `extend_slice`?
- Should `Stash::append_one` be generic over "append N elements" or just one?
