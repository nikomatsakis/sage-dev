# sage-arena

A type-erased heterogeneous arena for `Copy`-only data with thin handles.

## Motivation

Salsa interning doesn't pay off for syntax trees like `TypeRef` — they carry
span info, are per-item, and rarely duplicate across items. Instead, `sage-arena`
stores them densely in a flat byte buffer with thin `Copy` handles (`Ptr<T>`,
`Slice<T>`), avoiding heap allocation per node.

## Core design

### Storage

`Arena<'db>` owns a single `Vec<u8>` byte buffer. Each allocation appends data
(with alignment padding) and records an entry: `(TypeId, byte_offset, count)`.
Handles are just `u32` indices into the entry table.

```text
┌──────────────────────────────────────────────┐
│  buf: [padding] [T₀ bytes] [padding] [T₁ …] │
└──────────────────────────────────────────────┘
┌──────────────────────────────────────────────┐
│  entries: [(TypeId₀, off₀, 1), (TypeId₁, …)]│
└──────────────────────────────────────────────┘
```

### Two storage modes

A type implements exactly one of two traits:

- **`AllocArenaData`** — allocated, not deduplicated. Two `alloc` calls with
  the same value produce distinct `Ptr`s.
- **`InternArenaData`** — interned (deduplicated). Two `intern` calls with the
  same value return the same `Ptr`, so `Ptr` identity implies equality.

Both traits extend a sealed supertrait `ArenaData` that provides
`static_type_id()`, used for runtime type checking on retrieval.

### Handles

```rust
Ptr<T>   // thin handle to one value (u32 index)
Slice<T> // thin handle to a contiguous slice (u32 index)
```

Both are `Copy + Clone + Eq + Hash`. Equality is identity — same index means
same allocation. `Ptr` and `Slice` are 4 bytes each (plus zero-sized
`PhantomData<T>`).

### Retrieval via `Index`

```rust
let arena: Arena<'db> = Arena::new();
let p: Ptr<Point> = arena.alloc(Point { x: 1, y: 2 });
let val: &Point = &arena[p];

let s: Slice<Point> = arena.alloc_slice(&[Point { x: 3, y: 4 }]);
let vals: &[Point] = &arena[s];
```

The `Index` impl checks the stored `TypeId` against the expected type at
runtime. A type mismatch panics immediately.

### Interning lookup

On `intern(value)`:

1. Hash `value` to get `h`.
2. Probe `intern_map` with key `(type_id, h, 0)`, then `(type_id, h, 1)`, etc.
3. For each hit, compare the stored value with `==`.
4. On match → return existing `Ptr`. On miss → allocate, insert, return new `Ptr`.

The common case (no hash collision) is a single map lookup.

## Derive macros

```rust
#[derive(Copy, Clone, AllocArenaData)]
struct TypeRefData<'db> {
    kind: TypeRefKind<'db>,
    span: SpanIndices,
}

#[derive(Copy, Clone, Eq, PartialEq, Hash, InternArenaData)]
struct Name<'db> {
    id: u32,
}
```

The derive macros validate:
- No type or const generic parameters.
- At most one lifetime parameter, which must be named `'db`.

They generate `ArenaData` (with `static_type_id()` returning
`TypeId::of::<Self<'static>>()`) and the appropriate subtrait impl.

## Why `Copy` only

No `Drop`, no internal heap pointers — just flat data. Salsa IDs are `Copy`,
`SpanIndices` is `Copy`, so composed types like `TypeRefData<'db>` are `Copy`
when recursive cases use `Ptr`/`Slice` instead of `Vec`/`Box`.

## Lifetime trick

Given `p: Ptr<TypeRefData<'db>>` and `arena: Arena<'db>`, `arena[p]` returns
`&TypeRefData<'db>`. The `ArenaData<'db>` bound on the `Index` impl ties `T`'s
lifetime to the arena's. No GAT needed.

## Error conditions

| Scenario | Behavior |
|---|---|
| `Ptr<A>` used on arena that stored `B` at that index | **Panics** with "arena type mismatch" |
| `Slice<A>` used on arena that stored `B` at that index | **Panics** with "arena type mismatch" |
| `Ptr` index out of bounds | **Panics** with "index out of bounds" |
| `Ptr<A>` from arena₁ used on arena₂ (same type at that index) | **Silent wrong data** — no panic, reads arena₂'s value |

The last case is by design: handles are unscoped indices. Keeping them scoped
to a specific arena would require a lifetime or generative brand, which adds
complexity for no benefit in sage's use case (each item tree owns its arena).
