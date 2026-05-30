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
    Intrinsic(Intrinsic),
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
    Impl(ImplSymbol<'db>),
    Mod(ModSymbol<'db>),
    TypeAlias(TypeAliasSymbol<'db>),
    Const(ConstSymbol<'db>),
    Static(StaticSymbol<'db>),
    MacroDef(MacroDefAst<'db>),
    Use(UseGroupAst<'db>),
    MacroInvocation(MacroInvocationAst<'db>),
    Error(AbsoluteSpan),
    Unknown(SymExt),
}
```

Intrinsic primitives (`bool`, `i32`, `str`, etc.) are modeled as `SymbolData::Struct(StructSymbol::intrinsic(...))` — they are effectively magic structs.

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
    TupleStructCtor,
    Enum,
    Trait,
    Impl,
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
            ItemAst::Impl(i) => SymbolData::Impl(ImplSymbol::ast(i)),
            ItemAst::Error(span) => SymbolData::Error(span),
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
            SymExtKind::TupleStructCtor => SymbolData::TupleStructCtor(StructSymbol::ext(ext)),
            SymExtKind::Impl => SymbolData::Impl(ImplSymbol::ext(ext)),
            // ...
            SymExtKind::Other => SymbolData::Unknown(ext),
        };
        Self { data }
    }
}
```

`Symbol::tuple_struct_ctor(s: StructAst)` → `SymbolData::TupleStructCtor(StructSymbol::ast(s))`.

### `ModSymbol` integration

`ModSymbol` is already a per-kind wrapper with `Ast(ModAst)` / `Ext(ModExt)`. As part of this refactor, `ModExt` is removed and `ModSymbol` switches to storing `SymExt` (with `kind: SymExtKind::Mod`) like all other per-kind wrappers. This eliminates the historical asymmetry — `ModExt` was structurally identical to `SymExt` and predated the per-kind wrapper design.

The `SymbolData::Mod` variant wraps `ModSymbol` directly. The `From<ModSymbol> for Symbol` impl constructs `SymbolData::Mod(m)`.

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
| `Ctor(CtorOf::Struct, _)` | `TupleStructCtor` |
| `Enum` | `Enum` |
| `Trait`, `TraitAlias` | `Trait` |
| `Impl { .. }` | `Impl` |
| `Mod` | `Mod` |
| `TyAlias`, `AssocTy` | `TypeAlias` |
| `Const`, `AssocConst` | `Const` |
| `Static { .. }` | `Static` |
| `Macro(..)` | `MacroDef` |
| `Use` | `Use` |
| everything else | `Other` |

### `AllocStashData` for `Symbol`

`Symbol` derives `AllocStashData` today and implements `StashDirect`. After the refactor, `SymbolData` is larger (it now includes the per-kind wrapper which itself is an enum). This is fine — `Symbol` is still `Copy` (all arms are thin handles or small structs), and `StashDirect` remains valid since equality is structural.

The current `Symbol` is already ~24 bytes (driven by `ItemAst::Error(AbsoluteSpan)`). The proposed layout should be similar — per-kind wrappers are thin (discriminant + salsa ID or `SymExt`). Validate with `std::mem::size_of::<Symbol>()` before and after. Still small enough for value semantics.

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
    SymbolData::Error(span) => { ... }
    SymbolData::Unknown(ext) => { ... }
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

### Step 1: Add `SymExtKind` and extend `SymExt` ✅

Add the `SymExtKind` enum. Add a `kind` field to `SymExt` (default `Other` for back-compat). Add `kind` to `RawChild`. Update `ProxyTcxDb` to populate it from rustc's `DefKind`. Remove `ModExt` and switch `ModSymbol` to store `SymExt` (with `kind: SymExtKind::Mod`).

**Implemented.** The `sym_ext_kind_for_def_kind` mapping lives in `src/tcx_impl.rs` (alongside the existing `namespaces_for_def_kind`). `ModExt` is fully removed; `ModSymbol::external(cn, di)` now constructs `SymExt::new(cn, di, SymExtKind::Mod)` internally. The `From<ModExt> for SymExt` impl is removed since `ModSymbolData::Ext` now holds `SymExt` directly. No deviations from plan.

### Step 2: Add `ImplSymbol` and `StashDirect` impls ✅

The type-signatures RFD added `FnSymbol` through `StaticSymbol`. Add `ImplSymbol` (same `Ast`/`Ext` shape). `MacroDef`, `Use`, and `MacroInvocation` don't need wrappers — store the AST directly. All per-kind wrappers need `StashDirect` impls (trivial — they contain only salsa IDs and `SymExt` scalars).

**Implemented.** Added `ImplSymbol` via `define_kind_symbol!` macro. Added `StashDirect` impl to the macro itself so all per-kind wrappers (`FnSymbol`, `StructSymbol`, `EnumSymbol`, `TraitSymbol`, `TypeAliasSymbol`, `ConstSymbol`, `StaticSymbol`, `ImplSymbol`) get it automatically. Also added `StashDirect` for `SymExtKind`. No deviations from plan.

### Step 3: Rewrite `SymbolData` and `Symbol` construction ✅

Replace the three-variant `SymbolData` with the per-kind enum. Update `Symbol::ast()`, `Symbol::ext()`, `Symbol::tuple_struct_ctor()`, `From<ModSymbol>`, `From<ItemAst>`, etc. Update `AllocStashData` derive (may need manual impl if the enum is too complex for the macro).

**Implemented.** `SymbolData` now has 16 variants (per-kind wrappers + `MacroDef`/`Use`/`MacroInvocation` AST-only + `Intrinsic` + `Error` + `Unknown`). `Symbol::ast()` dispatches on `ItemAst` to create per-kind wrappers. `Symbol::ext()` dispatches on `SymExtKind`. Added `Symbol::as_ext()` convenience method used by `derive.rs` and display code. `AllocStashData` derive still works fine. Per-kind wrappers gained `salsa::Update` derive (needed because `SymbolData` derives it). No deviations from plan.

### Step 4: Migrate callers ✅

Mechanically update every `match sym.data()` across:
- `resolve.rs` — `symbol_to_module` and friends
- `display.rs` — `fmt_res`
- `body_resolve.rs` — if it dispatches on `SymbolData`
- `derive.rs` / `derive/builtins.rs`
- `memmap/` — any SymbolData matches
- Test files: `common/mod.rs`, `memmap_tests.rs`, `expand_tests.rs`

Add `Symbol::name()` and `Symbol::is_local()` convenience methods to reduce per-site boilerplate.

**Implemented.** Added `Symbol::name(db)`, `Symbol::is_local()`, and `Symbol::is_external()`. Caller migration was done as part of Step 3 (the two steps were effectively combined since all callers needed updating when `SymbolData` changed shape). `body_resolve.rs` did not dispatch on `SymbolData` — no changes needed there. `derive.rs` now uses `sym.as_ext()` instead of matching `SymbolData::Ext`.

### Step 5: Remove the old `Symbol::ast(ItemAst)` if desired

After all callers use per-kind constructors, the `Symbol::ast(item)` constructor that takes a full `ItemAst` could be removed in favor of direct `SymbolData::Fn(FnSymbol::ast(f))`. Or keep it as a convenience — it's a one-line dispatcher.

## Scope and non-goals

**In scope:**
- `SymExtKind` on `SymExt` and `RawChild`
- Per-kind `SymbolData` enum
- Migrating all `sym.data()` match sites
- Convenience methods (`name()`, `is_local()`)

**Out of scope:**
- Removing `ModSymbol` as a separate type (it has its own parent/file methods; only `ModExt` is removed)
- Per-kind ext data (`FnExt`, `StructExt` with signature info) — that's a separate concern tied to external signature loading
- Changing how `TupleStructCtor` works — it remains a `StructSymbol` wrapped in a dedicated variant

## Resolved decisions

- **Intrinsics** (`bool`, `i32`, `str`, etc.) are modeled as `SymbolData::Struct(StructSymbol::intrinsic(...))` — magic structs.
- **Impl blocks** get a first-class `SymbolData::Impl(ImplSymbol<'db>)` variant.
- **Local parse errors** (`ItemAst::Error(span)`) map to `SymbolData::Error(AbsoluteSpan)`.
- **Unmodeled external kinds** (`ExternCrate`, `GlobalAsm`, `OpaqueTy`, `Closure`, etc.) map to `SymbolData::Unknown(SymExt)`, preserving the handle for debugging.
- **External tuple struct ctors** arrive as rustc `DefKind::Ctor(CtorOf::Struct, _)` and map to `SymExtKind::TupleStructCtor` → `SymbolData::TupleStructCtor(StructSymbol::ext(...))`.
- **`ModExt` is removed** — `ModSymbol` stores `SymExt` like all other per-kind wrappers.
- **Enum variant symbols** are out of scope — tracked in a [separate RFD](./enum-variant-symbols.md).

## Resolved decisions (continued)

- **Macro/use wrappers:** No per-kind wrappers for `MacroDef`, `Use`, or `MacroInvocation`. These are only ever local, so `SymbolData` stores the AST directly (e.g., `MacroDef(MacroDefAst<'db>)`) without a `*Symbol` wrapper.
