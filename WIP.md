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

### TypeRef — syntax, not semantics

Unresolved type as written in source. Interned for fast comparison.

```rust
#[salsa::interned]
struct TypeRef<'db> { data: TypeRefData<'db> }

#[derive(Clone, PartialEq, Eq, Hash, salsa::Update)]
enum TypeRefData<'db> {
    Path(Path<'db>),
    Reference(TypeRef<'db>, Mutability),
    Slice(TypeRef<'db>),
    Tuple(Vec<TypeRef<'db>>),
    Never,
    Infer,
    // ...
}
```

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

1. Add `salsa` dependency, set up database
2. Define `Name`, `SourceFile`, `SpanTable`, `SpanIndices`, `Span`
3. Define item tracked structs (`FunctionItem`, `StructItem`, etc.)
4. Write `file_item_tree`: tree-sitter CST → tracked structs
5. Define `TypeRef` interned type
6. Module discovery: `mod` → file mapping
7. `crate_def_map`: name resolution against item trees + dep snapshot
8. `function_body`: on-demand body lowering
9. `resolve_body`: resolve names in a body

## Open decisions

- Exact representation of `Path<'db>` (segments as `Vec<Name<'db>>`? interned?)
- How `function_body` finds the body in the CST (by item index in file? by name?)
- Whether `Attr` needs its own tracked struct or is just data in a Vec
- How to model `impl` blocks (no natural `#[id]` — need synthetic identity)
