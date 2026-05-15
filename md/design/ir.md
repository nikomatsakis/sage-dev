# IR

The sage IR lives in `crates/sage-ir/`. It's built on salsa 0.26 and
has three layers: **items**, **syntactic bodies**, and **resolved
bodies**. Above the IR sits a thin **symbol layer** that unifies
local AST handles with external definitions.

For the conceptual model behind the type names see
[overview](./overview.md).

## Symbols (`symbol.rs`, `module.rs`)

The symbol layer is the cross-source vocabulary: a `Symbol` may
refer to a workspace `ItemAst` or to an external `(CrateNum,
DefIndex)`. Most resolution APIs return `Symbol`.

### `Symbol` and `SymExt`

```rust
pub struct Symbol<'db> {
    data: SymbolData<'db>,
}

pub enum SymbolData<'db> {
    Ast(ItemAst<'db>),
    Ext(SymExt),
}

pub struct SymExt {
    pub crate_num: CrateNum,
    pub def_index: DefIndex,
}
```

`Symbol` is a plain `Copy` newtype; identity flows from the inner
data. Constructors: `Symbol::ast(item)`, `Symbol::ext(ext)`,
`Symbol::external(cn, di)`. Inspect via `.data()`.

### `ModSymbol` and `ModExt`

The same wrapper-of-enum pattern at the module-kind level:

```rust
pub struct ModSymbol<'db> {
    data: ModSymbolData<'db>,
}

pub enum ModSymbolData<'db> {
    Ast(ModAst<'db>),
    Ext(ModExt),
}

pub struct ModExt {
    pub crate_num: CrateNum,
    pub def_index: DefIndex,
}
```

`ModSymbol` carries dispatching methods: `containing_file(db)`,
`parent(db)`, `crate_root(db)`, plus the resolution methods
`resolve_member`, `resolve_path`, `resolve_path_to_module`.

`ModSymbol` and `Symbol` are *not* salsa-interned. Their identity
is structural over the inner data.

## Items (`item.rs`)

Top-level declarations. Each kind is a salsa tracked struct; the
`ItemAst` enum wraps them all (it's `Copy` — each variant is a
salsa ID).

Tracked fields create incremental firewalls. A query reading
`params` won't re-execute when `body` changes.

Key item types:

- `FnAst` — function (signature + body)
- `StructAst`, `EnumAst` — type definitions
- `TraitAst`, `ImplAst` — trait and impl blocks
- `TypeAliasAst`, `ConstAst`, `StaticAst`
- `ModAst` — a module (see below)
- `UseGroupAst` — a `use` declaration desugared to flat imports
- `MacroDefAst` — `macro_rules!` definition
- `MacroInvocationAst` — `m!()` at item position
- `ItemAst::Error(AbsoluteSpan)` — bare variant for unrecognized
  syntax; not a tracked struct

### `ModAst`

A module written in (or synthesized for) the local workspace.
`ModAst` carries both the syntactic data and the resolution context:

```rust
#[salsa::tracked(debug)]
pub struct ModAst<'db> {
    pub name: Name<'db>,
    pub parent: Option<ModSymbol<'db>>,
    pub file: Option<SourceFile>,
    #[tracked] #[returns(ref)] pub attrs: Vec<Attr<'db>>,
    #[tracked] #[returns(ref)] pub inline_unexpanded_items: Option<Vec<ItemAst<'db>>>,
    #[tracked] pub span: AbsoluteSpan,
}
```

The `parent`, `file`, and `inline_unexpanded_items` fields encode
all four kinds of local module:

| Kind                          | `parent`  | `file`       | `inline_unexpanded_items` |
| ----------------------------- | --------- | ------------ | ------------------------- |
| Crate root                    | `None`    | `Some(file)` | `None`                    |
| File-based child (`mod foo;`) | `Some(p)` | `Some(file)` | `None`                    |
| Inline child (`mod foo { … }`)| `Some(p)` | `None`       | `Some(items)`             |
| Raw declaration (lowering)    | `None`    | `None`       | `Some`/`None`             |

Lowering produces declaration-site ModAsts (last row); resolution
mints a *resolved* ModAst with parent and file context filled in.
The mint goes through a `#[salsa::tracked]` function so equal
inputs produce the same id (see `resolve_mod_tracked` in
`resolve.rs`).

Most callers don't read `inline_unexpanded_items` directly. The
helper `ModAst::unexpanded_items(db)` returns the pre-expansion
item list — inline body if present, otherwise the file's
`parse_source_file`, otherwise empty. Use `inline_unexpanded_items`
only when you need to distinguish inline syntax from file syntax
(e.g. `Display` for `mod foo { ... }` vs. `mod foo;`).

`ModAst::crate_root(db, file)` and `ModAst::synthetic_child(db,
name, parent, file)` are the test/builder entry points; both wrap
salsa-tracked constructors so they're safe to call outside another
tracked function.

## Types (`types.rs`)

`TypeRef` represents types as written in source (unresolved). Salsa
tracked structs, so `&Vec<String>` is just nested salsa IDs —
`Copy`.

`Path` holds `Vec<Name>` segments. `Name` is salsa-interned
(O(1) equality).

`UseImport` represents a single flattened use import, with `kind`
of `Named(Name)`, `Glob`, or `Unnamed`. `UseGroupAst` holds a
`Vec<UseImport>`.

## Syntactic bodies (`body.rs`)

Function bodies live in a [Stash](./stash.md) — a flat byte buffer
for `Copy`-only data with thin handles (`Ptr<T>`, `Slice<T>`).
`FunctionBody = Stashed<Ptr<Body>>`.

Key types: `Expr`/`ExprKind`, `Stmt`/`StmtKind`, `Pat`/`PatKind`.
Paths are unresolved `Path` values. Bindings are `Name` values.

Notable variants:

- `IfLet(pat, scrutinee, then, else)` /
  `WhileLet(pat, scrutinee, body)` — preserved as distinct nodes
  (not desugared to `match`).
- `MacroCall(Path, TokenTree)` — macro path + opaque tokens (no
  expansion at this layer).

## Resolved bodies (`resolved.rs`, `body_resolve.rs`)

The resolved IR mirrors the syntactic body 1:1. Same structure, but
paths become `Res` and bindings become `LocalId`.

### `Res` — what a path resolved to

```rust
pub enum Res<'db> {
    Def(Symbol<'db>),
    Local(LocalId),
    Err,
}
```

`Res::Def` carries the new `Symbol` wrapper-of-enum, so a single
match handles both local items (`SymbolData::Ast`) and external
defs (`SymbolData::Ext`).

### `LocalId` and scopes

`LocalId(u32)` indexes into `RBody.locals: Slice<LocalVar>`. The
resolver tracks a scope stack (`Vec` of `Vec<(Name, LocalId)>`).
Scopes are pushed/popped at: blocks, closures, for loops, match
arms, if-let, while-let.

### `resolve_body` — the entry point

`resolve_body` in `body_resolve.rs` is a
`#[salsa::tracked(returns(ref))]` function. It takes
`(db, FnAst, ModSymbol, SourceRoot)` and returns `&ResolvedBody`.

The `BodyResolver` struct walks the syntactic body, reading from
the source `Stash` and writing to an output `Stash`. For each
node:

- Expressions: resolve paths (value/type/macro namespace), recurse.
- Statements: resolve init before pattern (let-binding ordering).
- Patterns: introduce bindings into current scope, resolve path
  patterns.

### Path resolution algorithm

Single-segment value paths check locals first (innermost scope
outward), then delegate to `resolve_name` (module items → use
imports → globs → extern prelude → std prelude).

Multi-segment paths use `ModSymbol::resolve_path` (handles `crate`
/ `self` / `super` / leading `::` / bare) and then walk remaining
segments via `resolve_member`.

### What stays unresolved

- **Method calls** — `receiver.method(args)` keeps the method `Name`.
- **Field access** — `expr.field` keeps the field `Name`.
- **Enum variants** — `Frame::Bulk` needs type-qualified lookup.
- **Associated functions** — `Type::func()` needs impl-block lookup.
- **Macro bodies** — tokens inside `MacroCall` are opaque.
- **Type references** — `TypeRef` passes through unchanged.

These are the next milestones; they all hinge on type checking,
which sage doesn't yet do.

## Module resolution (`resolve.rs`, `memmap/`)

Module-level name resolution uses a two-faced **MEM-map** (Minimal
Expanded Members map) per local module. A MEM-map holds the
entries needed to answer "what names does this module export?" —
items, redirects (`use foo::bar`), globs (`use foo::*`), and macro
invocations.

The `expanded_module(ModAst, SourceRoot) -> ExpandedModule` query
is salsa-tracked and keyed on `ModAst`. The convenience wrapper
`module_memmap(ModSymbol, SourceRoot)` dispatches on
`ModSymbolData`: ast → `expanded_module(ast, …)`; ext → empty
placeholder (external module contents come from `TcxDb` directly).

### Data model

`MemmapEntry` has five variants:

- `Item(ItemAst)` — a declared item (struct, fn, impl, mod, …).
  Namespace derived dynamically via `item_in_namespace`.
- `MacroDef(MacroDefAst)` — a `macro_rules!` definition, always in
  `Namespace::Macro(Bang)`.
- `Redirect { name, target }` — `use foo::bar [as baz]`. Namespace
  resolved dynamically by resolving `target` at lookup time.
- `Glob { path }` — `use foo::*`. Target module resolved
  dynamically at lookup time, so globs whose target is created by
  macro expansion are picked up correctly.
- `MacroUse(MacroUse)` — a macro invocation with resolution state
  (`Unresolved` / `Resolved(Vec<MacroCallee>)` /
  `Expanded(Vec<Expansion>)`).

Each `Expansion` pairs a callee with the entries it produced, so a
fan-out of multiple candidates becomes multiple branches inside a
single `MacroUse`.

### Two resolvers

The MEM-map is consumed by two resolvers with different semantic
guarantees:

- **Construction-time** (`memmap::resolve_path::resolve_macro_path`):
  used while `expanded_module` is building a module's entries to
  resolve macro invocation paths. Never errors; returns a
  `Vec<MacroCallee>` of candidates. Safe to call from inside
  `expanded_module` because it never re-enters the current
  module's expanded-module query — it walks via items
  (`parse_source_file`) for first-segment lookups.
- **Post-construction** (`resolve_name`): used by body resolution,
  display, and IDE-style queries. Returns exactly one `Symbol` or
  an error. Flattens the whole tree (entries at any
  `MacroUse::Expanded` depth count equally) and uses
  `expanded_module`-aware path walking, so names introduced by
  macro expansion inside any module are visible to downstream
  callers.

Priority in `resolve_name`: named (items / redirects / macro defs)
→ glob → extern prelude → std prelude.

### Dispatch on `ModSymbol`

The user-facing entry points are methods on `ModSymbol`:

- `resolve_member(db, source_root, name, ns)` — walks the
  expanded-module entries (or `TcxDb::module_children` for ext
  modules).
- `resolve_path(db, source_root, path, final_ns)` — walks a
  multi-segment path; first-segment dispatch handles `crate` /
  `self` / `super` / leading `::` / bare identifiers.
- `resolve_path_to_module(db, source_root, path)` — same, but
  always returns a module (used for glob targets).

All three branch on `ModSymbol::data()`: ast arm consults the
expanded module, ext arm consults `TcxDb`.

### Cycle handling

Path-walking helpers share a thread-local in-flight frame stack.
Re-entering the same `(module_key, name|path, kind)` triple
short-circuits to `None` / `Err`, so mutually cyclic globs and
redirect chains terminate without stack overflow. Module keys are
encoded so ast and ext modules can share the same stack without
collision (high bit set for ext, payload packs `(cn, di)`).

### Macro expansion

`memmap::expand::expand_macro` produces `Vec<MemmapEntry>` by
creating a synthetic `SourceFile` for the macro body and calling
`parse_source_file` on it. Expanded items are real tracked structs —
`StructAst`, `FnAst`, inline `ModAst` with populated `items`,
etc. — so downstream queries work uniformly for both source-level
and macro-introduced items.

The fixpoint loop in `resolve_and_expand_macros` iterates until
every `MacroUse` converges. `expanded_module` uses salsa's
`cycle_initial = empty` for cross-module cycles.

### Validation

`memmap::validate::memmap_errors` runs after convergence and
reports:

- `UnresolvedMacro { path }`
- `AmbiguousMacro { path }` (multiple candidate callees)
- `UnresolvedRedirect { name }` / `UnresolvedGlob { path }`
- `DuplicateName { name, ns }`
- `TimeTravelViolation { name, ns }` (a macro expansion introduces
  a name that would shadow a glob import)

### Resolving `mod foo` to a `ModSymbol`

`resolve_mod(db, parent, decl, source_root)` takes a
declaration-site `ModAst` (from lowering, with `parent = None,
file = None`) and produces a resolved `ModSymbol` with parent and
file context. It's backed by a `#[salsa::tracked]` function so
equal `(parent, decl, source_root)` triples produce the same id.

For inline modules, the resolved ModAst inherits the declaration's
`items`. For file-based modules, it looks up `foo.rs` or
`foo/mod.rs` relative to the parent's file.

`symbol_to_module` is the inverse direction: convert a `Symbol`
that names a module (`SymbolData::Ast(ItemAst::Mod(...))` or
`SymbolData::Ext(...)` with `is_module = true`) into a
`ModSymbol`.

### Free helpers

- `module_items(module)` / `module_use_imports(module)` —
  flat-list views over a module's declared items / use imports.
  Used by display, tests, and the demo entry point.
- `definition(module, name)` / `definition_in_ns(module, name, ns)`
  — direct child lookup, items-based for local modules and
  `TcxDb::module_children` for external. The ns variant filters by
  namespace on the ext arm.
- `resolve_module_path(root, &[&str])` — convenience for walking a
  segment-list path from a starting module.
- `dump_expanded_module(root, source_root, "crate::foo::bar")` —
  the demo entry point that ties path-string parsing to
  `resolve_module_path` + `expanded_module`.

## Display (`display.rs`)

Item types use `impl Display` with `salsa::with_attached_database`.
`FnAst::Display` prints the signature and body together.
Body types use a `PrettyPrint` trait (needs `&Stash` to deref
handles).

Resolved body display uses `pretty_print_resolved(tcx, resolved)`
which sets a thread-local `TcxDb` reference so `fmt_res` can call
`def_path` for human-readable external symbol names.

Display format:

- `<def Get>` — local item (via `SymbolData::Ast`)
- `<ext std::prelude::v1::Ok>` — external def (via
  `SymbolData::Ext` and `TcxDb::def_path`)
- `<local:0>` — local variable reference
- `<bind:3>` — binding introduction in a pattern
- `<unresolved>` — resolution failed

## Spans

See [Spans](./spans.md).
