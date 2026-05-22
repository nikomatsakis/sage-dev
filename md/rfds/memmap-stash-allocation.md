# RFD: Stash-allocated MEM-map entries

**Status:** Active

**Depends on:**
- [Hash-consed stash](./stash-hash-consing.md) — `Stash`, `Slice<T>`, `Stashed<T>`, `AllocStashData`

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
    TupleStructCtor(StructAst<'db>),
    MacroDef(MacroDefAst<'db>),
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

### Stash extensions needed

1. **`IndexMut` for `Slice<T>` and `Ptr<T>`** — allows the fixpoint loop to mutate entries in place (e.g., replacing a `MacroUse.expansions` slice handle with a new one).

2. **`Stash::append_one` (or similar)** — allocates a new slice consisting of an existing slice's contents plus one appended element. Used to grow `MacroUse.expansions` when a new callee is discovered.

3. **Empty slices are free** — `alloc_slice(&[])` is hash-consed, so all empty slices of the same type share one entry. This is already the case.

### Fixpoint loop strategy

The expansion loop operates on a `Stash` + root `Slice<MemmapEntry>`:

1. **Seeding** produces an initial `Stash` with all entries (paths already as `Slice<Name>`). `MacroUse` entries start with `expansions` pointing to an empty slice.

2. **Each pass** walks the entries via `IndexMut`, resolves macro paths, expands callees. When a new expansion is discovered:
   - Allocate the expansion's child entries into the stash → get a `Slice<MemmapEntry>`
   - Build an `Expansion { callee, entries }` and allocate a new expansions slice = old contents + new element via `append_one`
   - Mutate the `MacroUse` in place (via `IndexMut`) to point `expansions` at the new slice

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

## Implementation plan

1. Add `IndexMut` impls and `append_one` to `sage-stash`.
2. Update `MemmapEntry`, `MacroUse`, `Expansion` to stash-allocated `Copy` types.
3. Update `ExpandedModule` to hold `Memmap<'db>` (= `Stashed<Slice<MemmapEntry>>`).
4. Update seeding (`seed.rs`) to allocate into a `Stash`.
5. Update the fixpoint loop (`expand.rs`) to operate on `&mut Stash` + root slice.
6. Update consumers (`resolve/mod.rs`, `validate.rs`, `resolve_path.rs`) to thread a stash reference.
7. Update tests.

## Open questions

- Naming: `append_one` vs `push_slice` vs `extend_slice`?
- Should `Stash::append_one` be generic over "append N elements" or just one?
