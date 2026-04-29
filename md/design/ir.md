# IR

The sage IR lives in the `sage-ir` crate. It's built on salsa 0.26 and
represents Rust source code as a collection of tracked structs with
strategically placed incremental firewalls.

## Database

```rust
#[salsa::db]
pub trait Db: salsa::Database {}
```

All tracked structs and functions are parameterized over `&dyn Db`. The
concrete `Database` struct holds salsa's storage.

## Source files

```rust
#[salsa::input]
pub struct SourceFile {
    pub path: String,
    pub text: String,
}
```

One `SourceFile` per file in the workspace. This is the root input — when
the text changes, salsa knows to re-execute queries that depend on it.

## Names

```rust
#[salsa::interned]
pub struct Name<'db> {
    pub text: String,
}
```

Interned identifiers. Two `Name` values with the same text are the same
salsa ID, so equality is O(1). Used for all identifiers, path segments,
field names, etc.

## Items

Items are the top-level declarations in a file. Each item kind is its own
salsa tracked struct, wrapped in a thin `Item` enum:

```rust
enum Item<'db> {
    Function(FunctionItem<'db>),
    Struct(StructItem<'db>),
    Enum(EnumItem<'db>),
    Trait(TraitItem<'db>),
    Impl(ImplItem<'db>),
    TypeAlias(TypeAliasItem<'db>),
    Const(ConstItem<'db>),
    Static(StaticItem<'db>),
    Mod(ModItem<'db>),
    Use(UseItem<'db>),
    Error(SpanIndices),
}
```

The `Item` enum is `Copy` — each variant is just a salsa ID (a `u32`).

### Tracked fields and incrementality

Each item struct uses `#[tracked]` fields to create incremental firewalls.
For example, `FunctionItem`:

```rust
#[salsa::tracked]
pub struct FunctionItem<'db> {
    // Identity field (untracked) — used to match items across revisions.
    pub name: Name<'db>,

    // Each #[tracked] field is independently change-tracked.
    #[tracked] pub attrs: Vec<Attr<'db>>,
    #[tracked] pub params: Vec<Param<'db>>,
    #[tracked] pub ret_type: Option<TypeRef<'db>>,
    #[tracked] pub is_async: bool,
    #[tracked] pub is_unsafe: bool,
    #[tracked] pub body: FunctionBody<'db>,
    #[tracked] pub span_table: SpanTable<'db>,
    #[tracked] pub span: SpanIndices,
}
```

The key insight: **editing a function body doesn't invalidate readers of
its signature.** A query that only reads `params` and `ret_type` won't
re-execute when the body changes, because salsa tracks each field
independently.

The `name` field is *untracked* — it serves as the item's identity. When
`file_item_tree` re-executes after an edit, salsa matches the new
`FunctionItem` to the old one by comparing `name`. If the name matches,
salsa compares each tracked field individually and only marks changed
fields as dirty.

## Types

Type references are salsa tracked structs representing types as written in
source (unresolved):

```rust
#[salsa::tracked]
pub struct TypeRef<'db> {
    pub kind: TypeRefKind<'db>,
    pub span: SpanIndices,
}

enum TypeRefKind<'db> {
    Path(Path<'db>),
    Reference(TypeRef<'db>, Mutability),
    Slice(TypeRef<'db>),
    Array(TypeRef<'db>),
    Tuple(TupleTypeRef<'db>),
    Never,
    Infer,
    Error,
}
```

Recursive types (like `&Vec<String>`) use salsa IDs for the inner types —
`TypeRef<'db>` is just a `u32`, so `Reference(TypeRef<'db>, Mutability)` is
cheap and `Copy`.

`Path<'db>` holds a `Vec<Name<'db>>` of segments. For now, paths are
captured as a single segment with the full text (e.g., `std::collections::HashMap`
is one segment). Proper multi-segment path resolution comes later.

## Attributes

```rust
#[salsa::tracked]
pub struct Attr<'db> {
    pub kind: AttrKind,       // Normal or DocComment
    pub path: Path<'db>,      // e.g., "derive", "cfg", "doc"
    pub args: Option<TokenTree<'db>>,
    pub span: SpanIndices,
    pub is_inner: bool,       // #![...] vs #[...]
}
```

Doc comments (`///`, `//!`) are lowered as `DocComment` attrs and displayed
in their original syntax form, not as `#[doc = "..."]`. Regular attributes
capture the path and raw token tree arguments.

Every item struct carries `attrs: Vec<Attr<'db>>` as a tracked field, so
attribute changes are tracked independently from other item properties.

## Bodies

Function bodies are stored in a `Stash` — a type-erased flat buffer for
`Copy`-only data with thin handles (`Ptr<T>`, `Slice<T>`). This avoids
the overhead of creating a salsa tracked struct per expression node.

```rust
pub type FunctionBody<'db> = Stashed<Ptr<Body<'db>>>;

pub struct Body<'db> {
    pub root: Ptr<Expr<'db>>,
    pub span: SpanIndices,
}
```

`Stashed<T>` pairs a `Stash` with a root value. It implements `PartialEq`
by comparing the stash's raw bytes — if the body didn't change, the bytes
are identical and salsa skips downstream invalidation.

### Expressions

```rust
enum ExprKind<'db> {
    Literal(Literal),
    Path(Path<'db>),
    Block(Slice<Stmt<'db>>, Option<Ptr<Expr<'db>>>),
    Call(Ptr<Expr<'db>>, Slice<Expr<'db>>),
    MethodCall(Ptr<Expr<'db>>, Name<'db>, Slice<Expr<'db>>),
    Field(Ptr<Expr<'db>>, Name<'db>),
    Binary(Ptr<Expr<'db>>, BinaryOp, Ptr<Expr<'db>>),
    If(Ptr<Expr<'db>>, Ptr<Expr<'db>>, Option<Ptr<Expr<'db>>>),
    Match(Ptr<Expr<'db>>, Slice<MatchArm<'db>>),
    // ... and many more
    MacroCall(Path<'db>, TokenTree<'db>),
    Missing,
}
```

Body types mix stash handles (`Ptr`, `Slice`) for tree structure with
salsa IDs (`Name<'db>`, `Path<'db>`, `TypeRef<'db>`) for data that's
shared with the signature level. This means a `Cast` expression can
reference the same `TypeRef` tracked struct that appears in a function's
return type.

### Patterns

```rust
enum PatKind<'db> {
    Wildcard,
    Bind(Name<'db>, Mutability),
    Path(Path<'db>),
    Tuple(Slice<Pat<'db>>),
    Struct(Path<'db>, Slice<FieldPat<'db>>),
    TupleStruct(Path<'db>, Slice<Pat<'db>>),
    // ...
}
```

## Spans

```rust
pub struct SpanIndices {
    pub start: u32,
    pub end: u32,
}
```

Every node carries a `SpanIndices` — byte offsets into the source file.
These are 8 bytes, `Copy`, and don't carry a lifetime. A `SpanTable`
tracked struct maps a span back to its source file, but semantic queries
that don't need source locations never read the span table, so span
changes don't trigger re-analysis.

## The query graph

```
SourceFile (input)
  │
  ▼
file_item_tree(file) → Vec<Item>
  │                     Each item is a tracked struct with:
  │                     - identity (name)
  │                     - tracked signature fields (params, ret_type, ...)
  │                     - tracked body (Stashed<Ptr<Body>>)
  │                     - tracked attrs
  │
  ▼
crate_def_map(krate)                    [planned]
  │  Merges item trees + dep snapshot
  │  into a name resolution map.
  │
  ▼
Body analysis                           [planned]
  │  Resolves names, type-checks
  │  expressions in function bodies.
```

The critical property: each level only depends on the specific tracked
fields it reads. A name resolution query that reads function signatures
won't be invalidated by body edits. A body analysis query won't be
invalidated by changes to unrelated functions (salsa matches items by
name across revisions).
