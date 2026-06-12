# RFD: IR Reshape — Symbols, Scope, and Two-Layer Architecture

**Status:** Draft

**Supersedes:**
- [Symbol-Level Signature Queries](./symbol-signatures.md) (partially implemented, to be reworked)

## Problem

The current IR has accumulated accidental complexity:

1. **`SourceRoot` threaded everywhere** — signature queries, body resolution, and type checking all take `(module, source_root)` as explicit parameters. Callers must compute the defining module themselves.

2. **Exposed intermediate body representation** — CST → resolved body (`RExpr`) → type checking. The resolved-but-untyped middle layer (`ResolvedBody`) is currently a public, separately-queryable artifact. No consumer wants it without types — it should be internal to a single body query.

3. **`ScopeSymbol` is underweight** — it's a single-variant enum wrapping `ModSymbol`. The original design called for items to know their scope, but the current implementation computes it ad-hoc.

## Goal

Symbols know their scope. Queries are single-keyed. The body pipeline
is two steps internally (resolve → infer) but one query externally.

```
TreeSitter file
    │
    ▼ (parse — mints per-item symbols with CST stashes)
LocalModSymbol + LocalStructSymbol / LocalFnSymbol / ...
    │                              │
    │                              ▼ (sig query: resolve types via scope)
    │                              │
    │                              ▼ (body query: resolve names → infer types)
    │                          Typed IR (Ty, FnSig, TypedBody)
    ▼
module tree (for name resolution)
```

From the outside, each symbol has a single-keyed `sig()` or `body()` query.
Internally, `body()` first builds a `ResolvedBody` and then runs type inference.
This keeps the two concerns decoupled in implementation without exposing the
intermediate representation to callers. A future RFD may fuse them into a single
walk.

## Design

### CST layer and symbol minting

TreeSitter parses source text. We walk the tree once, minting per-item
symbols with per-item CST stashes. Parsing runs in multiple queries —
once for each source file and once for each macro expansion — but
always produces symbols directly:

```rust
#[salsa::input]
struct SourceFile {
    path: String,
    text: String,
}

// Parse a source file → module symbol containing item symbols
#[salsa::tracked]
fn parse_file<'db>(db: &'db dyn Db, file: SourceFile, scope: ScopeSymbol<'db>) -> LocalModSymbol<'db> { ... }

// Parse macro expansion output → additional item symbols
#[salsa::tracked]
fn parse_expansion<'db>(db: &'db dyn Db, exp: MacroExpansion<'db>, scope: ScopeSymbol<'db>) -> Vec<LocalSymbol<'db>> { ... }
```

The `scope` parameter tells the parser which module/crate the parsed
items belong to. For the initial file parse, the driver supplies the
crate-level scope. For macro expansions, the expansion machinery
supplies the scope of the module where the macro was invoked.

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

- **`LocalFooSymbol`** — a salsa tracked struct, parsed from source. Owns its CST and scope.
- **`ExternalFooSymbol`** — a handle into rustc metadata (`CrateNum`, `DefIndex`).

These leaves are packaged into **grouping enums** at various levels of specificity:

```rust
// Per-kind grouping: local or external
enum StructSymbol<'db> {
    Local(LocalStructSymbol<'db>),
    External(ExternalStructSymbol<'db>),
}

enum ModSymbol<'db> {
    Local(LocalModSymbol<'db>),
    External(ExternalModSymbol<'db>),
}

// Provenance grouping: any local item, any external item
enum LocalSymbol<'db> {
    Struct(LocalStructSymbol<'db>),
    Fn(LocalFnSymbol<'db>),
    Enum(LocalEnumSymbol<'db>),
    Mod(LocalModSymbol<'db>),
    // ...
}

enum ExternalSymbol<'db> {
    Struct(ExternalStructSymbol<'db>),
    Fn(ExternalFnSymbol<'db>),
    // ...
}

// Top-level: anything
enum Symbol<'db> {
    Struct(StructSymbol<'db>),
    Fn(FnSymbol<'db>),
    Enum(EnumSymbol<'db>),
    Mod(ModSymbol<'db>),
    Intrinsic(Intrinsic),
    // ...
}
```

**Key design rules:**

- Grouping enums only expose methods that *all* their variants support. For enums bridging local/external (like `StructSymbol`), this means the shared interface is fully resolved, fully typed results — since that's all rustc metadata provides. CST, scope, and resolution machinery are internal to the `Local*` leaf types and never visible through the grouping enum.

- Use the most precise enum possible. A `LocalModSymbol`'s item list contains `LocalSymbol` values (never external items — those are parsed from source). A `pub use` redirect *resolves* to an external symbol at query time, but the module's own items are always local.

Derives generate:
- **`From` impls** — e.g., `LocalStructSymbol → StructSymbol → Symbol`, `LocalStructSymbol → LocalSymbol → Symbol`. Use `#[derive(FromImpls)]` (exists in `~/dev/dada`, `dada-util-procmacro`): generates `From<FieldType> for Self` for each single-field variant. `#[no_from_impl]` opts out specific variants.
- **Delegating methods** — match on the enum, forward to the method on each leaf. E.g., `StructSymbol::sig(db)` dispatches to `LocalStructSymbol::sig` or `ExternalStructSymbol::sig`. (Separate derive TBD.)

```rust
#[derive(FromImpls)]
enum StructSymbol<'db> {
    Local(LocalStructSymbol<'db>),
    External(ExternalStructSymbol<'db>),
}
```

### Module and symbol layer

**Leaf types:**

```rust
#[salsa::tracked]
struct LocalModSymbol<'db> {
    source: LocalModSource<'db>,
    scope: ScopeSymbol<'db>,
}

struct ExternalModSymbol<'db> {
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
struct LocalStructSymbol<'db> {
    scope: ScopeSymbol<'db>,
    cst: Stashed<StructCst<'db>>,
}

#[salsa::tracked]
struct LocalFnSymbol<'db> {
    scope: ScopeSymbol<'db>,
    cst: Stashed<FnCst<'db>>,
}
```

### `ScopeSymbol`: the resolution environment

```rust
enum ScopeSymbol<'db> {
    Crate(LocalCrateSymbol<'db>),
    Module(LocalModSymbol<'db>),
    Fn(LocalFnSymbol<'db>),
}
```

Resolution starts from the scope and walks upward through the module
tree. Since `LocalModSymbol` knows its own scope (its parent module or
crate), the resolver can chain upward without any external parameters.

The `Fn` variant is needed for items defined inside function bodies
(closures, inner functions). Most items have `ScopeSymbol::Module` or
`ScopeSymbol::Crate` as their scope.

### Signature and body queries

Queries are keyed on the symbol alone. The scope provides everything needed for resolution:

```rust
impl<'db> LocalStructSymbol<'db> {
    #[salsa::tracked]
    pub fn sig(self, db: &'db dyn Db) -> StructSig<'db> {
        // reads self.cst, resolves type paths via self.scope
    }

    #[salsa::tracked]
    pub fn fields(self, db: &'db dyn Db) -> StructFields<'db> {
        // reads sig, resolves field types
    }
}

impl<'db> LocalFnSymbol<'db> {
    #[salsa::tracked]
    pub fn sig(self, db: &'db dyn Db) -> FnSig<'db> { ... }

    #[salsa::tracked]
    pub fn body(self, db: &'db dyn Db) -> TypedBody<'db> {
        // internally: resolve names → then infer types
        // from outside: one query, one result
    }
}
```

The `body()` query internally runs resolution (producing a `ResolvedBody`) and then type inference sequentially. From the caller's perspective this is atomic — there is no separately-queryable intermediate representation. A future RFD may fuse these into a single walk for performance, but the external API is already correct.

### Typed IR output

`Ty` / `TyData` remain essentially unchanged — stash-allocated, same variants (`Adt`, `Ref`, `Param`, intrinsics, etc.).

`TypedBody` wraps the resolved body plus its type-check result:

```rust
struct TypedBody<'db> {
    resolved: ResolvedBody<'db>,
    diagnostics: Vec<Diagnostic<'db>>,
    stash: Stash,
}
```

Internally, `ResolvedBody` and the R-expression types (`RExpr`, `RStmt`, etc.) survive as implementation details of the body query. They are not exposed through the public symbol API. A future RFD may fuse resolution and inference into a single walk; until then they run sequentially inside the query.

### What gets deleted

- `*_defining_module` helpers in `scope.rs` — symbols know their scope
- `SourceRoot` parameter threading — absorbed into the module/crate hierarchy
- `SigLowerCtx` as a standalone struct — its logic folds into the sig query methods on `LocalStructSymbol`, `LocalFnSymbol`, etc. (the resolver construction and rib setup become local to each query)
- Public visibility of `resolve_body` — becomes internal to the `body()` query

## Open questions

- **Parent linkage on `LocalModSymbol`**: does a module store its parent explicitly, or is containment derived from the crate's module tree? (The `scope` field may already suffice if `ScopeSymbol::Module(parent)` or `ScopeSymbol::Crate(c)` is the parent.)

## Implementation plan

The implementation proceeds in three phases. Each phase keeps the system
functional with passing tests — no big-bang rewrite.

The implementation plan uses existing type names (e.g., `ModSymbol`,
`FnAst`) where it refers to the current codebase state, and target
names (e.g., `LocalModSymbol`, `LocalFnSymbol`) where it refers to new
types being introduced.

### Phase 1: Absorb `SourceRoot` into the crate/scope hierarchy

**Goal:** Queries derive `SourceRoot` from the scope chain rather than
receiving it as a parameter.

**Step 1a — Introduce `LocalCrateSymbol`:**

```rust
#[salsa::input]
struct LocalCrateSymbol<'db> {
    root_mod: ModAst<'db>,
    source_root: SourceRoot,
}
```

The driver creates one of these instead of holding separate
`ModSymbol` + `SourceRoot`. `SageContext` wraps a `LocalCrateSymbol`.
This is purely additive.

**Step 1b — Wire `ScopeSymbol::Crate` variant:**

```rust
enum ScopeSymbol<'db> {
    Crate(LocalCrateSymbol<'db>),
    Module(ModSymbol<'db>),       // existing type, migrated in Phase 2
}
```

Add `ScopeSymbol::source_root(db) -> SourceRoot` that either reads
directly from `LocalCrateSymbol` or walks `ModSymbol.parent` up to the
crate root. This is the key bridge.

The `Fn` variant from the target design is not needed yet — it arrives
in Phase 2 alongside `LocalFnSymbol`.

**Step 1c — Internal `source_root` in `Resolver`:**

Change `Resolver::new(db, source_root, scope)` →
`Resolver::new(db, scope)`. The resolver derives `source_root` from
`scope.source_root(db)`. Old callers temporarily construct a scope
that embeds the source root.

**Step 1d — Migrate queries one by one:**

For each query (`fn_signature`, `struct_signature`, `enum_signature`,
`resolve_body`, `expanded_module`), remove the `source_root` and
`module` parameters; derive them from the symbol's scope internally.
Tests pass between each migration.

**Step 1e — Delete `*_defining_module` helpers** once nothing needs
them.

### Phase 2: Items know their scope (`Local*Symbol` wrappers)

**Goal:** Each item is a salsa tracked struct that knows its enclosing
scope, enabling single-keyed queries.

**Step 2a — Introduce `LocalFnSymbol`, `LocalStructSymbol`, etc.:**

```rust
#[salsa::tracked]
struct LocalFnSymbol<'db> {
    scope: ScopeSymbol<'db>,
    ast: FnAst<'db>,   // existing CST — renamed to FnCst later (Phase 3)
}
```

These are thin wrappers. The existing `FnAst`, `StructAst`, etc.
remain as the CST/syntax layer. The new `Local*Symbol` types own the
scope linkage.

At this point, `ScopeSymbol` gains the `Fn(LocalFnSymbol)` variant
for items nested inside function bodies.

**Step 2b — Mint `Local*Symbol` during parsing:**

Parsing already walks items per file. The parser receives the owning
scope and mints `Local*Symbol` instances directly. For macro
expansions, the expansion query parses the expanded text and likewise
produces symbols scoped to the invoking module. The minting always
happens in a parse query — it just runs in multiple queries (file
parse, macro expansion parse).

**Step 2c — Migrate signature queries to single-keyed:**

```rust
#[salsa::tracked]
pub fn fn_signature(db, sym: LocalFnSymbol) -> FnSig { ... }
```

The query reads `sym.scope(db)` to construct the resolver. The
`SigLowerCtx` struct dissolves — its resolver construction and rib
setup become local to the query body.

**Step 2d — Migrate `body()` to single-keyed:**

```rust
impl<'db> LocalFnSymbol<'db> {
    #[salsa::tracked]
    pub fn body(self, db: &'db dyn Db) -> TypedBody<'db> {
        let resolved = resolve_body(db, self.ast(db), ...);
        let typed = type_check_body(db, &resolved, ...);
        typed
    }
}
```

Internally this calls existing resolution and inference code in
sequence. From outside, one query key, one result. `resolve_body`
becomes a private helper.

### Phase 3: Naming and taxonomy cleanup

**Goal:** Align naming with the target taxonomy.

- Optionally rename `FnAst` → `FnCst`, `StructAst` → `StructCst` (low priority — existing names are adequate)
- Introduce `#[derive(FromImpls)]` for grouping enum constructors
- Add delegating-method derives for the per-kind grouping enums
- Restructure `Symbol` / `SymbolData` to use the grouping-enum pattern from the Design section

This is cosmetic and can be done at any point after Phase 2.

### Sequencing

```
1a  LocalCrateSymbol (additive)
1b  ScopeSymbol::Crate variant
1c  Resolver stops taking source_root
1d  Migrate queries (one at a time, tests pass between each)
1e  Delete *_defining_module helpers
2a  Local*Symbol tracked structs (additive)
2b  Mint during parsing (file parse + macro expansion parse)
2c  Single-keyed signature queries (SigLowerCtx dissolves)
2d  Single-keyed body query (resolve + infer internally)
3   Naming/taxonomy cleanup (whenever)
```

Each numbered step is a commit or small PR. The system compiles and
tests pass at every boundary.
