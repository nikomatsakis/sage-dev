# RFD: Per-Kind SymbolData

**Status:** Draft

**Depends on:**
- [Type signatures](./type-signatures.md) — per-kind symbol wrappers (`FnSymbol`, `StructSymbol`, …)
- [Module sym tree](./module-sym-tree.md) — `Symbol`, `SymbolData`, `SymExt`, `ModSymbol`
- [Tuple struct constructors](./tuple-struct-ctors.md) — `TupleStructCtor` variant

## Goal

Make `Symbol::data()` return a per-kind `SymbolData` that tells callers "this is a function", "this is a struct", etc., regardless of whether the symbol is workspace-local or external. Today callers must first check `Ast` vs `Ext`, then (for `Ast`) branch on `ItemAst` — two levels of dispatch to answer one question. External symbols carry no kind information at all; callers that need it must query `TcxDb` separately.

## Current state

```rust
pub enum SymbolData<'db> {
    Ast(ItemAst<'db>),
    TupleStructCtor(StructAst<'db>),
    Ext(SymExt),
}
```

`SymExt` is `{ crate_num: CrateNum, def_index: DefIndex }` — an opaque handle with no kind. The type-signatures RFD added per-kind wrappers (`FnSymbol`, `StructSymbol`, …) with `Ast`/`Ext` inner enums, but `Symbol::data()` still returns the old three-variant enum.

Callers typically do:

```rust
match sym.data() {
    SymbolData::Ast(ItemAst::Function(f)) => { /* fn-specific */ }
    SymbolData::Ast(ItemAst::Struct(s)) => { /* struct-specific */ }
    // ...
    SymbolData::Ext(ext) => { /* no kind info, must query TcxDb or give up */ }
}
```

## Design

### `SymbolData` becomes per-kind

```rust
pub enum SymbolData<'db> {
    Fn(FnSymbol<'db>),
    Struct(StructSymbol<'db>),
    TupleStructCtor(StructSymbol<'db>),
    Enum(EnumSymbol<'db>),
    Trait(TraitSymbol<'db>),
    Mod(ModSymbol<'db>),
    TypeAlias(TypeAliasSymbol<'db>),
    Const(ConstSymbol<'db>),
    Static(StaticSymbol<'db>),
    MacroDef(MacroDefSymbol<'db>),
    Use(UseSymbol<'db>),
    MacroInvocation(MacroInvocationSymbol<'db>),
    Error,
}
```

`Symbol` remains a `Copy` newtype with a private `data` field. `Symbol::data()` returns the enum by value.

### `SymExt` gains a kind

```rust
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub struct SymExt {
    pub crate_num: CrateNum,
    pub def_index: DefIndex,
    pub kind: SymExtKind,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub enum SymExtKind {
    Fn,
    Struct,
    Enum,
    Trait,
    Mod,
    TypeAlias,
    Const,
    Static,
    MacroDef,
    Use,
    Other,
}
```

The `kind` is populated at the point of creation — `TcxDb::module_children` already knows the namespace and def kind of each child. The `RawChild` struct gains a `kind: SymExtKind` field, and the caller that interns `RawChild` into `Symbol` uses it to construct the correct `SymbolData` variant.

### `Symbol` construction

`Symbol::ast(item: ItemAst)` dispatches on `ItemAst` to create the matching variant:

```rust
impl<'db> Symbol<'db> {
    pub fn ast(item: ItemAst<'db>) -> Self {
        let data = match item {
            ItemAst::Function(f) => SymbolData::Fn(FnSymbol::ast(f)),
            ItemAst::Struct(s) => SymbolData::Struct(StructSymbol::ast(s)),
            ItemAst::Enum(e) => SymbolData::Enum(EnumSymbol::ast(e)),
            // ...
        };
        Self { data }
    }
}
```

`Symbol::ext(ext: SymExt)` dispatches on `ext.kind`:

```rust
impl<'db> Symbol<'db> {
    pub fn ext(ext: SymExt) -> Self {
        let data = match ext.kind {
            SymExtKind::Fn => SymbolData::Fn(FnSymbol::ext(ext)),
            SymExtKind::Struct => SymbolData::Struct(StructSymbol::ext(ext)),
            // ...
            SymExtKind::Other => SymbolData::Error,
        };
        Self { data }
    }
}
```

`Symbol::tuple_struct_ctor(s: StructAst)` → `SymbolData::TupleStructCtor(StructSymbol::ast(s))`.

### `ModSymbol` integration

`ModSymbol` is already a per-kind wrapper with `Ast(ModAst)` / `Ext(ModExt)`. The `SymbolData::Mod` variant wraps it directly. The `From<ModSymbol> for Symbol` impl constructs `SymbolData::Mod(m)`.

For external modules, `ModExt` doesn't carry `SymExtKind` (it doesn't need to — it's always a module). The conversion from `SymExt` → `ModSymbol` for the `Mod` variant extracts `crate_num`/`def_index`.

### TcxDb changes

`RawChild` gains a `kind` field:

```rust
pub struct RawChild {
    pub name: String,
    pub crate_num: CrateNum,
    pub def_index: DefIndex,
    pub namespace: Namespace,
    pub kind: SymExtKind,
}
```

The `ProxyTcxDb` implementation populates `kind` from rustc's `DefKind`. The mapping is straightforward:

| rustc `DefKind` | `SymExtKind` |
|---|---|
| `Fn`, `AssocFn` | `Fn` |
| `Struct` | `Struct` |
| `Enum` | `Enum` |
| `Trait`, `TraitAlias` | `Trait` |
| `Mod` | `Mod` |
| `TyAlias`, `AssocTy` | `TypeAlias` |
| `Const`, `AssocConst` | `Const` |
| `Static { .. }` | `Static` |
| `Macro(..)` | `MacroDef` |
| `Use` | `Use` |
| everything else | `Other` |

### `AllocStashData` for `Symbol`

`Symbol` derives `AllocStashData` today and implements `StashDirect`. After the refactor, `SymbolData` is larger (it now includes the per-kind wrapper which itself is an enum). This is fine — `Symbol` is still `Copy` (all arms are thin handles or small structs), and `StashDirect` remains valid since equality is structural.

The size of `Symbol` grows from ~16 bytes (tag + `ItemAst`/`SymExt`) to ~24 bytes (tag + per-kind wrapper with its own tag + payload). Still small enough for value semantics.

### Callers that just want ast-vs-ext

Some callers (display, debug) only need "is this local or external?" without caring about kind. Add convenience methods:

```rust
impl<'db> Symbol<'db> {
    pub fn is_local(&self) -> bool { ... }
    pub fn is_external(&self) -> bool { !self.is_local() }
    pub fn as_ext(&self) -> Option<SymExt> { ... }
}
```

These iterate the known variants. Alternatively each per-kind wrapper already has `as_ast()` / `as_ext()` — callers can match on `SymbolData` and then call those.

## Migration

The migration is mechanical but wide — every `match sym.data()` in the codebase changes shape.

### Before

```rust
match sym.data() {
    SymbolData::Ast(ItemAst::Function(f)) => { ... }
    SymbolData::Ast(ItemAst::Struct(s)) => { ... }
    SymbolData::Ast(_) => { ... }
    SymbolData::TupleStructCtor(s) => { ... }
    SymbolData::Ext(ext) => { ... }
}
```

### After

```rust
match sym.data() {
    SymbolData::Fn(fn_sym) => { ... }
    SymbolData::Struct(struct_sym) => { ... }
    SymbolData::TupleStructCtor(struct_sym) => { ... }
    // ...
    SymbolData::Error => { ... }
}
```

Callers that need the `FnAst` use `fn_sym.as_ast()`. Callers that need `SymExt` use `fn_sym.as_ext()`.

### Callers that group many kinds

Some callers (e.g. `item_name` in display) handle all named items. These get a wildcard with a helper:

```rust
impl<'db> Symbol<'db> {
    pub fn name(&self, db: &'db dyn Db) -> Option<Name<'db>> { ... }
}
```

Or they match the relevant variants and use `_` for the rest.

## Implementation plan

### Step 1: Add `SymExtKind` and extend `SymExt`

Add the `SymExtKind` enum. Add a `kind` field to `SymExt` (default `Other` for back-compat). Add `kind` to `RawChild`. Update `ProxyTcxDb` to populate it from rustc's `DefKind`.

### Step 2: Add missing per-kind wrappers

The type-signatures RFD added `FnSymbol` through `StaticSymbol`. Add the remaining: `MacroDefSymbol`, `UseSymbol`, `MacroInvocationSymbol` (or decide these don't need wrappers — they could stay as `SymbolData::MacroDef(MacroDefAst)` directly since they have no ext counterpart in practice).

### Step 3: Rewrite `SymbolData` and `Symbol` construction

Replace the three-variant `SymbolData` with the per-kind enum. Update `Symbol::ast()`, `Symbol::ext()`, `Symbol::tuple_struct_ctor()`, `From<ModSymbol>`, `From<ItemAst>`, etc. Update `AllocStashData` derive (may need manual impl if the enum is too complex for the macro).

### Step 4: Migrate callers

Mechanically update every `match sym.data()` across:
- `resolve.rs` — `symbol_to_module` and friends
- `display.rs` — `fmt_res`
- `body_resolve.rs` — if it dispatches on `SymbolData`
- `derive.rs` / `derive/builtins.rs`
- `memmap/` — any SymbolData matches
- Test files: `common/mod.rs`, `memmap_tests.rs`, `expand_tests.rs`

Add `Symbol::name()` and `Symbol::is_local()` convenience methods to reduce per-site boilerplate.

### Step 5: Remove the old `Symbol::ast(ItemAst)` if desired

After all callers use per-kind constructors, the `Symbol::ast(item)` constructor that takes a full `ItemAst` could be removed in favor of direct `SymbolData::Fn(FnSymbol::ast(f))`. Or keep it as a convenience — it's a one-line dispatcher.

## Scope and non-goals

**In scope:**
- `SymExtKind` on `SymExt` and `RawChild`
- Per-kind `SymbolData` enum
- Migrating all `sym.data()` match sites
- Convenience methods (`name()`, `is_local()`)

**Out of scope:**
- Removing `ModSymbol` as a separate type (it has its own parent/file methods)
- Per-kind ext data (`FnExt`, `StructExt` with signature info) — that's a separate concern tied to external signature loading
- Changing how `TupleStructCtor` works — it remains a `StructSymbol` wrapped in a dedicated variant

## Open questions

1. **Wrappers for macro/use items.** Should `MacroDefAst`, `UseGroupAst`, and `MacroInvocationAst` get per-kind wrappers with an ext arm? Today there are no external macro defs or use items in the symbol table. Options: (a) skip wrappers, store the AST directly in `SymbolData`; (b) add thin wrappers anyway for uniformity. Leaning toward (a) — these items don't have external counterparts and adding wrappers is pure boilerplate.

2. **`SymExtKind::Other`.** What external definitions don't map to our kind system? Likely: `ExternCrate`, `GlobalAsm`, `OpaqueTy`, `Closure`, `Impl`, etc. These would be `Other` and appear as `SymbolData::Error` after dispatch. Is `Error` the right name, or should there be an `Unknown(SymExt)` variant that preserves the handle?
