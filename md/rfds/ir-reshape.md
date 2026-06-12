# RFD: IR Reshape — Symbols, Scope, and Two-Layer Architecture

**Status:** Draft

**Supersedes:**
- [Symbol-Level Signature Queries](./symbol-signatures.md) (partially implemented, to be reworked)

## Problem

The current IR has accumulated accidental complexity:

1. **`SourceRoot` threaded everywhere** — signature queries, body resolution, and type checking all take `(module, source_root)` as explicit parameters. Callers must compute the defining module themselves.

2. **Three-layer body pipeline** — CST → resolved body (`RExpr`) → type checking. The resolved-but-untyped middle layer (`ResolvedBody`) has no consumer that wants it without types. It exists only because resolution and inference were designed separately.

3. **`ScopeSymbol` is underweight** — it's a single-variant enum wrapping `ModSymbol`. The original design called for items to know their scope, but the current implementation computes it ad-hoc.

## Goal

Two layers, not three. Symbols know their scope. Queries are single-keyed.

```
TreeSitter file
    │
    ▼ (parse — one pass, per-item stashes)
LocalModSym + LocalStructSym / LocalFnSym / ...
    │                              │
    │                              ▼ (sig/body queries — resolve + type in one pass)
    │                          Typed IR (Ty, FnSig, TypedBody)
    ▼
module tree (for name resolution)
````

## Design

### CST layer

TreeSitter parses the whole file. We walk the tree once, minting per-item symbols with per-item CST stashes:

```rust
#[salsa::input]
struct SourceFile {
    path: String,
    text: String,
}

// One parse pass: file → module symbol with item list
#[salsa::tracked]
fn parse_file<'db>(db: &'db dyn Db, file: SourceFile) -> LocalModSym<'db> { ... }
```

CST nodes mirror TreeSitter structure. They live in per-item stashes with relative spans. Nothing smart — just syntax.

```rust
struct StructCst<'db> {
    name: Name<'db>,
    generics: Slice<GenericParamCst<'db>>,
    fields: Slice<FieldCst<'db>>,
    where_clauses: Slice<WhereClauseCst<'db>>,
}

struct FieldCst<'db> {
    name: Name<'db>,
    ty: Ptr<TypeCst<'db>>,  // syntax-level type reference
}

struct FnCst<'db> {
    name: Name<'db>,
    generics: Slice<GenericParamCst<'db>>,
    params: Slice<ParamCst<'db>>,
    ret: Option<Ptr<TypeCst<'db>>>,
    body: Ptr<ExprCst<'db>>,
    where_clauses: Slice<WhereClauseCst<'db>>,
}
```

### Naming conventions and symbol taxonomy

The symbol layer has two kinds of leaf type:

- **`LocalFooSym`** — a salsa tracked struct, parsed from source. Owns its CST and scope.
- **`ExternalFooSym`** — a handle into rustc metadata (`CrateNum`, `DefIndex`).

These leaves are packaged into **grouping enums** at various levels of specificity:

```rust
// Per-kind grouping: local or external
enum StructSym<'db> {
    Local(LocalStructSym<'db>),
    External(ExternalStructSym<'db>),
}

enum ModSym<'db> {
    Local(LocalModSym<'db>),
    External(ExternalModSym<'db>),
}

// Provenance grouping: any local item, any external item
enum LocalSym<'db> {
    Struct(LocalStructSym<'db>),
    Fn(LocalFnSym<'db>),
    Enum(LocalEnumSym<'db>),
    Mod(LocalModSym<'db>),
    // ...
}

enum ExternalSym<'db> {
    Struct(ExternalStructSym<'db>),
    Fn(ExternalFnSym<'db>),
    // ...
}

// Top-level: anything
enum Sym<'db> {
    Struct(StructSym<'db>),
    Fn(FnSym<'db>),
    Enum(EnumSym<'db>),
    Mod(ModSym<'db>),
    Intrinsic(Intrinsic),
    // ...
}
```

**Key design rules:**

- Grouping enums only expose methods that *all* their variants support. For enums bridging local/external (like `StructSym`), this means the shared interface is fully resolved, fully typed results — since that's all rustc metadata provides. CST, scope, and resolution machinery are internal to the `Local*` leaf types and never visible through the grouping enum.

- Use the most precise enum possible. A `LocalModSym`'s item list contains `LocalSym` values (never external items — those are parsed from source). A `pub use` redirect *resolves* to an external symbol at query time, but the module's own items are always local.

Derives generate:
- **`From` impls** — e.g., `LocalStructSym → StructSym → Sym`, `LocalStructSym → LocalSym → Sym`. Use `#[derive(FromImpls)]` (exists in `~/dev/dada`, `dada-util-procmacro`): generates `From<FieldType> for Self` for each single-field variant. `#[no_from_impl]` opts out specific variants.
- **Delegating methods** — match on the enum, forward to the method on each leaf. E.g., `StructSym::sig(db)` dispatches to `LocalStructSym::sig` or `ExternalStructSym::sig`. (Separate derive TBD.)

```rust
#[derive(FromImpls)]
enum StructSym<'db> {
    Local(LocalStructSym<'db>),
    External(ExternalStructSym<'db>),
}
```

### Module and symbol layer

**Leaf types:**

```rust
#[salsa::tracked]
struct LocalModSym<'db> {
    source: LocalModSource<'db>,
    scope: ScopeSym<'db>,
}

struct ExternalModSym<'db> {
    crate_num: CrateNum,
    def_index: DefIndex,
}

enum LocalModSource<'db> {
    File(SourceFile),
    Inline(ModItems<'db>),
}

struct ModItems<'db> {
    items: Stashed<Slice<ModItem<'db>>>,
}
```

Each item symbol is a salsa tracked struct that knows its enclosing scope:

```rust
#[salsa::tracked]
struct LocalStructSym<'db> {
    scope: ScopeSym<'db>,
    cst: Stashed<StructCst<'db>>,
}

#[salsa::tracked]
struct LocalFnSym<'db> {
    scope: ScopeSym<'db>,
    cst: Stashed<FnCst<'db>>,
}
```

### `ScopeSym`: the resolution environment

```rust
enum ScopeSym<'db> {
    Crate(LocalCrateSym<'db>),
    Module(LocalModSym<'db>),
    Fn(LocalFnSym<'db>),
}
```

Resolution starts from the scope and walks upward through the module tree. Since `LocalModSym` knows its own scope (its parent module or crate), the resolver can chain upward without any external parameters.

### Signature and body queries

Queries are keyed on the symbol alone. The scope provides everything needed for resolution:

```rust
impl<'db> LocalStructSym<'db> {
    #[salsa::tracked]
    pub fn sig(self, db: &'db dyn Db) -> StructSig<'db> {
        // reads self.cst, resolves type paths via self.scope
    }

    #[salsa::tracked]
    pub fn fields(self, db: &'db dyn Db) -> StructFields<'db> {
        // reads sig, resolves field types
    }
}

impl<'db> LocalFnSym<'db> {
    #[salsa::tracked]
    pub fn sig(self, db: &'db dyn Db) -> FnSig<'db> { ... }

    #[salsa::tracked]
    pub fn body(self, db: &'db dyn Db) -> TypedBody<'db> {
        // walks CST, resolves names + infers types in one fused pass
    }
}
```

### Typed IR output

`Ty` / `TyData` remain essentially unchanged — stash-allocated, same variants (`Adt`, `Ref`, `Param`, intrinsics, etc.).

`TypedBody` replaces both `ResolvedBody` and the type-check result. Every expression carries its resolved `Symbol` and inferred `Ty`. There is no intermediate "resolved but untyped" representation.

```rust
struct TypedBody<'db> {
    data: Stashed<TypedBodyData<'db>>,
}

struct TypedExpr<'db> {
    kind: TypedExprKind<'db>,
    ty: Ptr<Ty<'db>>,
}
```

### What gets deleted

- `ResolvedBody`, `RExpr`, `Res` — the resolved-but-untyped layer
- `*_defining_module` helpers in `scope.rs` — symbols know their scope
- `SourceRoot` parameter threading — absorbed into the module/crate hierarchy
- `SigLowerCtx` as a separate concern — folded into the sig queries

## Open questions

- **Parent linkage on `LocalModSym`**: does a module store its parent explicitly, or is containment derived from the crate's module tree? (The `scope` field may already suffice if `ScopeSym::Module(parent)` or `ScopeSym::Crate(c)` is the parent.)

## Implementation plan

TBD — this RFD captures the target architecture. Implementation will likely proceed incrementally, reshaping one layer at a time while keeping the system functional.
