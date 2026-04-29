# WIP: Salsa-based IR with macro expansion

## Goal

Given a `#[test]` fn in a workspace (target: mini-redis), resolve all names
and expand macros in its body. Demand-driven — only pull in what the test
touches.

## Architecture

### Salsa inputs

```rust
#[salsa::input]
struct SourceFile {
    #[returns(ref)]
    text: String,
    #[returns(ref)]
    tree: tree_sitter::Tree,
}
```

`Tree` is `Clone + Send + Sync`. On edit: incremental tree-sitter re-parse,
then `file.set_tree(&mut db).to(new_tree)`. The `file_item_tree` tracked
function walks the tree directly — no re-parsing.

### Span design

One `SpanTable` per top-level item. Editing one item's body doesn't affect
other items' span tables.

```rust
#[derive(Copy, Clone, PartialEq, Eq, Hash)]
struct SpanIndices {
    start: u32,  // index into SpanTable.byte_offsets
    end: u32,
}

#[derive(Copy, Clone)]
struct Span<'db> {
    table: SpanTable<'db>,
    indices: SpanIndices,
}

#[salsa::tracked]
struct SpanTable<'db> {
    #[tracked]
    file: SourceFile,
    #[tracked]
    #[returns(ref)]
    byte_offsets: Vec<u32>,
}
```

- `SpanIndices` — 8 bytes, `Copy`, no `'db`. Stored densely in body nodes.
- `Span<'db>` — self-contained, carries the table. Used at item level.
- Semantic queries never read `byte_offsets`, so span changes don't invalidate them.

### Interned types

```rust
#[salsa::interned]
struct Name<'db> {
    #[returns(ref)]
    text: String,
}
```

Used for identifiers, paths — anything compared frequently.

### Item-level tracked structs

Created by `file_item_tree`. One per top-level item. Bodies NOT stored here.

```rust
#[salsa::tracked]
struct FunctionItem<'db> {
    #[id] name: Name<'db>,
    #[tracked] #[returns(ref)] params: Vec<Param<'db>>,
    #[tracked] #[returns(ref)] return_type: Option<TypeRef<'db>>,
    #[tracked] is_async: bool,
    #[tracked] #[returns(ref)] attrs: Vec<Attr<'db>>,
    #[tracked] span_table: SpanTable<'db>,
    #[tracked] span: SpanIndices,
}
```

`#[tracked]` on each semantic field = incremental firewall. Body edits don't
change params/return_type, so callers of those stay cached.

### `sage-arena` — type-erased heterogeneous arena (own crate)

Salsa interning doesn't pay off for syntax trees like `TypeRef` — they carry
span info, are per-item, and rarely duplicate across items. Instead, store
them densely in a `Copy`-only arena with thin handles.

**Core design:**

Two mutually exclusive traits determine how a type is stored:

```rust
/// Allocated (not deduplicated). Two `alloc` calls with the same value
/// produce distinct `Ptr`s.
///
/// # Safety
/// Same invariants as below.
///
/// Derive: `#[derive(AllocArenaData)]` — verifies at most one lifetime `'db`.
pub unsafe trait AllocArenaData<'db>: Copy {
    fn static_type_id() -> TypeId;
}

/// Interned (deduplicated). Two `intern` calls with the same value return
/// the same `Ptr`, so `Ptr` identity implies equality.
///
/// # Safety
/// - Only lifetimes in `Self` are `'db` or `'static`.
/// - `static_type_id()` returns `TypeId` of the `'static` version of Self.
/// - `Self: Copy + Hash + Eq`.
///
/// Derive: `#[derive(InternArenaData)]` — verifies at most one lifetime `'db`.
pub unsafe trait InternArenaData<'db>: Copy + Hash + Eq {
    fn static_type_id() -> TypeId;
}

pub struct Arena<'db> {
    buf: Vec<u8>,
    entries: Vec<(TypeId, u32, u32)>,  // (type_id, byte_offset, count)
    intern_map: HashMap<(TypeId, u64, u32), u32>,  // (static_type_id, content_hash, collision_idx) → entry index
    _marker: PhantomData<&'db ()>,
}
```

**Interning lookup:** on `intern(value)`, hash `value`, then probe
`(type_id, hash, 0)`, `(type_id, hash, 1)`, ... in `intern_map`. For each
hit, read the stored `T` from `buf` and compare with `==`. On match, return
the existing `Ptr`. On miss (key absent), alloc into `buf` and insert. The
common case (no collision) is a single map lookup.

/// Thin handle to one value. Equality is identity (same index).
#[derive(Copy, Clone, Eq, PartialEq, Hash)]
pub struct Ptr<T> { index: u32, _marker: PhantomData<T> }

/// Thin handle to a contiguous slice. Equality is identity.
#[derive(Copy, Clone, Eq, PartialEq, Hash)]
pub struct Slice<T> { index: u32, _marker: PhantomData<T> }
```

A type implements exactly one of the two traits. The derive macros validate
that the type has at most one lifetime parameter (`'db`).

**Arena API:**

```rust
impl<'db> Arena<'db> {
    pub fn alloc<T: AllocArenaData<'db>>(&mut self, value: T) -> Ptr<T>;
    pub fn alloc_slice<T: AllocArenaData<'db>>(&mut self, values: &[T]) -> Slice<T>;

    pub fn intern<T: InternArenaData<'db>>(&mut self, value: T) -> Ptr<T>;
    pub fn intern_slice<T: InternArenaData<'db>>(&mut self, values: &[T]) -> Slice<T>;
}

// Index works for both traits — a sealed marker trait or blanket bound
// unifies them for retrieval.
impl<'db, T: AllocArenaData<'db>> Index<Ptr<T>> for Arena<'db> {
    type Output = T;
}
impl<'db, T: InternArenaData<'db>> Index<Ptr<T>> for Arena<'db> {
    type Output = T;
}
// (In practice: use a sealed supertrait `ArenaEntry<'db>` that both
// AllocArenaData and InternArenaData extend, and write Index once.)
```

**Lifetime trick:** given `p: Ptr<TypeRefData<'db>>` and `arena: Arena<'db>`,
`arena[p]` returns `&TypeRefData<'db>` — the `AllocArenaData<'db>` bound on
the `Index` impl ties `T`'s lifetime to the arena's. No GAT needed.

**Why `Copy` only:** no `Drop`, no internal heap pointers — just flat data.
Salsa IDs are `Copy`, `SpanIndices` is `Copy`, so composed types like
`TypeRefData<'db>` are `Copy` when recursive cases use `Ptr`/`Slice` instead
of `Vec`/`Box`.

### TypeRef — syntax, not semantics

Unresolved type as written in source. Stored in an arena, not interned.

```rust
#[derive(Copy, Clone, AllocArenaData)]
enum TypeRefData<'db> {
    Path(Path<'db>),
    Reference(Ptr<TypeRefData<'db>>, Mutability),
    Slice(Ptr<TypeRefData<'db>>),
    Tuple(arena::Slice<TypeRefData<'db>>),
    Never,
    Infer,
    // ...
}
```

Function bodies, type trees, etc. each get their own `Arena<'db>` plus a root
`Ptr` or `Slice`. Cheap to rebuild on re-parse since it's just a flat buffer.

### Bodies — parsed on demand

```rust
#[salsa::tracked]
fn function_body(db: &dyn Db, func: FunctionItem<'_>) -> Body<'_> { ... }
```

`Body` contains an arena of expressions/statements with `SpanIndices`.
The query re-walks the CST to find the function by name/index and lowers
the body.

### Dep snapshot

Kept outside Salsa as an immutable side table on the database. Foreign items
from `TyCtxt` accessed via a method on the `Db` trait. No incrementality
needed — deps don't change within a session.

### Query graph

```
SourceFile (input: text + tree)
  │
  ▼
file_item_tree(file) → FunctionItem, StructItem, ImplItem, ...
  │                     (each with its own SpanTable)
  ▼
crate_def_map(krate) → name resolution (merges item trees + dep snapshot)
  │
  ▼
function_body(func) → Body (arena of exprs with SpanIndices)
  │
  ▼
resolve_body(func) → resolved names in body
```

### Module discovery

`crate_structure(krate)` walks `lib.rs`, finds `mod` declarations, resolves
them to files. Returns the module tree. `crate_def_map` merges all modules'
item trees.

## Language subset (see md/design/subsetting.md)

- No proc-macro crates defined in the workspace
- No glob imports from workspace modules
- Globs from external deps: supported (already resolved in dep snapshot)

## Implementation order

1. Create `sage-arena` crate with `ArenaData`, `Ptr`, `Slice`, `Arena`
2. Add `salsa` dependency, set up database
3. Define `Name`, `SourceFile`, `SpanTable`, `SpanIndices`, `Span`
4. Define item tracked structs (`FunctionItem`, `StructItem`, etc.)
5. Write `file_item_tree`: tree-sitter CST → tracked structs
6. Define `TypeRefData` in arena (not interned)
7. Module discovery: `mod` → file mapping
8. `crate_def_map`: name resolution against item trees + dep snapshot
9. `function_body`: on-demand body lowering into `Arena<'db>` + root `Ptr`
10. `resolve_body`: resolve names in a body

## Open decisions

- Exact representation of `Path<'db>` (segments as `Vec<Name<'db>>`? interned?)
- How `function_body` finds the body in the CST (by item index in file? by name?)
- Whether `Attr` needs its own tracked struct or is just data in a Vec
- How to model `impl` blocks (no natural `#[id]` — need synthetic identity)
