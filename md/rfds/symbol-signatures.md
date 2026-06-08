# RFD: Symbol-Level Signature Queries

**Status:** Proposed

**Depends on:**
- [Type Signatures](./type-signatures.md) — `Ty`, `Binder`, stash-allocated types
- [Per-kind symbol data](./per-kind-symbol-data.md) — `StructSymbol`, `FnSymbol`, etc.

## Problem

The current signature queries (`fn_signature`, `struct_signature`, `enum_signature` in `sig_lower.rs`) take AST nodes as keys:

```rust
#[salsa::tracked(returns(ref))]
pub fn struct_signature<'db>(
    db: &'db dyn Db,
    struct_ast: StructAst<'db>,
    module: ModSymbol<'db>,
    source_root: SourceRoot,
) -> Stashed<Binder<'db, StructSig<'db>>>;
```

This has two problems:

1. **External symbols have no AST.** The type checker must call `struct_sym.as_ast()` and bail with a `todo!()` for external structs. We need to serve signatures for external types (from rustc metadata) through the same interface.

2. **The caller must supply `module` and `source_root`.** These are needed internally to resolve type paths within the signature. But the caller (the type checker) passes *its own* module — not the module where the struct is defined. This is incorrect for cross-module references and will break as soon as we type-check code that uses structs from other modules.

## Goal

A single `#[salsa::tracked]` query keyed on the *symbol* (not the AST), that works for both local and external definitions:

```rust
#[salsa::tracked(returns(ref))]
pub fn struct_signature<'db>(
    db: &'db dyn Db,
    sym: StructSymbol<'db>,
    source_root: SourceRoot,
) -> Stashed<StructSignature<'db>>;
```

The type checker calls `struct_signature(db, struct_sym, source_root)` without needing to know whether the symbol is local or external.

## Design

### `ScopeSymbol`: the resolution environment for local symbols

Every AST-based symbol needs enough information to lazily reconstruct its name resolution environment (resolve type paths in field types, return types, etc.). Rather than passing `module` and `source_root` from the caller, we record the symbol's **enclosing scope** at creation time.

```rust
#[derive(Copy, Clone, Debug)]
pub enum ScopeSymbol<'db> {
    Module(ModSymbol<'db>),
    Impl(ImplSymbol<'db>),
    // Future: Function(FnSymbol<'db>), ...
}
```

`Module` is the scope for top-level items. `Impl` is the scope for methods and associated items inside an impl block — it provides the self-type and the impl's own generic parameters when constructing the resolver. Future variants (e.g., `Function` for items nested inside function bodies) can be added as needed.

**Key property:** `ScopeSymbol` knows how to produce a `Resolver` (see below).

Walking up parent scopes (e.g., a function inside a module) is inherent to constructing the resolver — it doesn't need to be a separate public method.

### Refactoring `Resolver` to own its scope

Currently, `Resolver` is scope-agnostic — every `resolve_xxx` method takes a `module` argument:

```rust
// Current interface
pub struct Resolver<'db> {
    db: &'db dyn Db,
    source_root: SourceRoot,
    in_flight: Vec<InFlightQuery<'db>>,
}

impl<'db> Resolver<'db> {
    pub fn resolve_name(&mut self, module: ModSymbol<'db>, name: Name<'db>, ns: Namespace) -> ...;
    pub fn resolve_segments(&mut self, module: ModSymbol<'db>, segments: &[Name<'db>], ns: Namespace) -> ...;
}
```

But in practice the module never varies within a single resolver's lifetime:
- `SigLowerCtx` stores `resolver` + `module` and always calls `self.resolver.resolve_segments(self.module, ...)`
- `BodyResolver` does the same — stores both, passes the same module every time
- `validate.rs` creates a fresh `Resolver` per module in a loop

The refactored interface moves the scope *into* the `Resolver`:

```rust
pub struct Resolver<'db> {
    db: &'db dyn Db,
    source_root: SourceRoot,
    scope: ScopeSymbol<'db>,
    in_flight: Vec<InFlightQuery<'db>>,
}

impl<'db> Resolver<'db> {
    pub fn new(db: &'db dyn Db, source_root: SourceRoot, scope: ScopeSymbol<'db>) -> Self { ... }

    // Public entry points — use self.scope to determine the starting module:
    pub fn resolve_name(&mut self, name: Name<'db>, ns: Namespace) -> Result<Symbol<'db>, ResolutionError> { ... }
    pub fn resolve_segments(&mut self, segments: &[Name<'db>], ns: Namespace) -> Result<Symbol<'db>, ResolutionError> { ... }
}
```

Note: the *internal* helpers (`resolve_member_impl`, `resolve_remainder`, `walk_entries`, etc.) continue to accept explicit `module` arguments — they need to traverse into different modules as they walk multi-segment paths. The scope only determines the *starting* module for the public entry points.

For `ScopeSymbol::Impl(impl_sym)`, the resolver derives the starting module from the impl's own scope (impls are always inside a module), and additionally makes the impl's self-type and generic parameters available for `Self` resolution. This replaces the current `self_type: Option<Ty>` parameter threaded through `lower_fn_sig`.

`ScopeSymbol` then becomes the entry point for constructing a resolver:

```rust
impl<'db> ScopeSymbol<'db> {
    pub fn resolver(&self, db: &'db dyn Db, source_root: SourceRoot) -> Resolver<'db> {
        Resolver::new(db, source_root, *self)
    }
}
```

This eliminates the `module` field from both `SigLowerCtx` and `BodyResolver` — they just hold a `Resolver` that already knows its scope. The `validate.rs` loop creates a fresh `Resolver` per module as before, just with the module baked in at construction.

### Storing the scope on AST nodes

The scope is a tracked field on the salsa-tracked AST structs (`StructAst`, `FnAst`, `EnumAst`, etc.) — not on the kind-symbol wrappers. The kind-symbol types (`StructSymbol`, `FnSymbol`, etc.) remain unchanged; they just wrap `StructAst | SymExt` as before.

```rust
#[salsa::tracked(debug)]
pub struct StructAst<'db> {
    pub name: Name<'db>,
    #[tracked]
    pub scope: ScopeSymbol<'db>,
    // ...existing fields...
}
```

The memmap/resolve phase sets this field when creating the AST node, since it knows the enclosing module at that point. The signature query accesses it via `sym.as_ast().unwrap().scope(db)`.

External symbols don't have AST nodes and don't need a scope — their signatures are pre-resolved in metadata.

### Signature vs. per-kind content

For every symbol kind, we distinguish:

- **Signature** — what the outside world needs to use the symbol ("outside the `{}`"). Every symbol kind has a signature containing at least generics and where-clauses. Some kinds include more (e.g., function signatures include parameter types and return type).
- **Per-kind content** — accessed through specialized queries named after what they return: a struct has *fields*, an enum has *variants*, a trait has *items*, a function has a *body*.

This separation matters for dependency ordering: you can reference a struct's signature (e.g., to check generic arity or instantiate it) without triggering resolution of its field types, which may be cyclic.

#### ADTs (structs and enums)

A struct is an ADT with a single variant; an enum is an ADT with multiple variants. They share the same types:

```rust
/// Shared by both StructSymbol and EnumSymbol.
/// (`WherePredicate` is defined in the [Trait System](./trait-system.md) RFD;
/// initially an always-empty slice until where-clause lowering is implemented.)
pub struct AdtSignature<'db> {
    pub where_clauses: Binder<'db, Slice<WherePredicate<'db>>>,
}

/// Shared by both StructSymbol and EnumSymbol.
pub struct AdtVariants<'db> {
    pub variants: Binder<'db, Slice<VariantDef<'db>>>,
}

pub struct VariantDef<'db> {
    pub name: Name<'db>,
    pub fields: Slice<FieldDef<'db>>,
}

pub struct FieldDef<'db> {
    pub name: Name<'db>,
    pub ty: Ptr<Ty<'db>>,
}
```

For a struct `Point { x: i32, y: i32 }`, `struct_variant` returns a single `VariantDef` whose `name` is `Point` and whose `fields` are `[x: i32, y: i32]`. For an enum, `enum_variants` returns all variants. `VariantDef.name` is always present — for structs it's the struct's own name, for enum variants it's the variant name.

Structs and enums share the same `AdtSignature`, `VariantDef`, and `FieldDef` types but have separate queries returning the appropriate shape.

#### Function

```rust
pub struct FnSignature<'db> {
    pub sig: Binder<'db, FnSigInner<'db>>,
}

pub struct FnSigInner<'db> {
    pub params: Slice<Ptr<Ty<'db>>>,
    pub ret: Ptr<Ty<'db>>,
    pub where_clauses: Slice<WherePredicate<'db>>,
}

// Body already exists as `ResolvedBody`.
```

`Binder` continues to carry the `generics: Slice<GenericParam<'db>>` and wraps whatever content is bound by those generics. Both the signature (where-clauses) and the variants are under the same binder — they share the same set of generic parameters.

Note: since `struct_signature` and `struct_variant` are separate queries with separate `Stashed` values, the `generics` slice is duplicated in each stash. However, both slices contain the **same `AstGenericParam` instances** (salsa-tracked, stable identity). A shared helper computes the `AstGenericParam` symbols once, and both queries call it to populate their binders.

### Queries

```rust
// Structs
fn struct_signature(db, sym: StructSymbol, source_root: SourceRoot) -> Stashed<AdtSignature>;
fn struct_variant(db, sym: StructSymbol, source_root: SourceRoot) -> Stashed<Binder<'db, VariantDef<'db>>>;

// Enums
fn enum_signature(db, sym: EnumSymbol, source_root: SourceRoot) -> Stashed<AdtSignature>;
fn enum_variants(db, sym: EnumSymbol, source_root: SourceRoot) -> Stashed<Binder<'db, Slice<VariantDef<'db>>>>;

// Functions
fn fn_signature(db, sym: FnSymbol, source_root: SourceRoot) -> Stashed<FnSignature>;
// fn body is already ResolvedBody
```

For local symbols, all queries use `sym.scope()` to construct a resolver internally. For external symbols, they load from crate metadata.

### Scope of this RFD

This pattern applies uniformly to all item kinds. Each query uses `sym.scope()` internally for local symbols and loads metadata for external ones.

## Implementation steps

### Step 1: Add `ScopeSymbol` type and store it on AST nodes

- Define `ScopeSymbol` enum in `sage-ir` (just the `Module` variant for now).
- Add `scope: ScopeSymbol<'db>` as a tracked field on `StructAst`, `FnAst`, `EnumAst`, etc.
- Update the parsing/memmap phase where these AST nodes are created to supply the enclosing module as the scope.
- The `define_kind_symbol!` macro and `StructSymbol`/`FnSymbol`/`EnumSymbol` types remain unchanged.
- Existing tests should continue to compile with minimal call-site changes (passing the scope where AST nodes are constructed).

### Step 2: Refactor `Resolver` to own its scope

- Add `scope: ScopeSymbol<'db>` field to `Resolver`.
- Change `Resolver::new` to accept a `ScopeSymbol`.
- Remove the `module` parameter from the **public** entry points (`resolve_name`, `resolve_segments`). These derive the starting module from `self.scope`.
- Internal helpers (`resolve_member_impl`, `resolve_remainder`, `walk_entries`, etc.) keep their explicit `module` arguments — they traverse into different modules during multi-segment path resolution.
- Update all call sites:
  - `SigLowerCtx`: drop `module` field, construct resolver with scope.
  - `BodyResolver`: same.
  - `validate.rs`: construct resolver with `ScopeSymbol::Module(m)` in the loop.
- Add `ScopeSymbol::resolver()` convenience method.
- All existing tests pass unchanged (same behavior, just different plumbing).

### Step 3: Introduce `AdtSignature`, `VariantDef`, `FieldDef` types

- Define the new types (`AdtSignature`, `VariantDef`, `FieldDef`, `FnSignature`, `FnSigInner`).
- These coexist with the old `StructSig`, `FnSig`, `EnumSig` temporarily.

### Step 4: New symbol-keyed queries

- Implement `struct_signature(db, StructSymbol, SourceRoot) -> Stashed<AdtSignature>`.
- Implement `struct_variant(db, StructSymbol, SourceRoot) -> Stashed<Binder<VariantDef>>`.
- Implement `enum_signature`, `enum_variants`, `fn_signature` similarly.
- Each query internally calls `sym.scope().resolver(db, source_root)` for local symbols and dispatches to metadata for external ones.
- Old AST-keyed queries (`fn_signature(db, FnAst, ...)`) remain temporarily for compatibility.

### Step 5: Migrate callers to new queries

- `check.rs` (`check_struct_lit`, `check_field_access`): replace `struct_sym.as_ast().unwrap()` + `struct_signature(db, ast, module, source_root)` with `struct_variant(db, struct_sym, source_root)`.
- `TestCrate` harness (`sage-test-harness`): call `fn_signature(db, fn_sym, source_root)` instead of `fn_signature(db, fn_ast, module, source_root)`.
- `body_resolve.rs`: construct resolver from scope instead of taking module as argument.
- Remove `TODO(symbol-signatures RFD)` comments.

### Step 6: Delete old queries and types

- Remove old `fn_signature(db, FnAst, ModSymbol, SourceRoot)`, `struct_signature(db, StructAst, ...)`, `enum_signature(db, EnumAst, ...)`.
- Remove `StructSig`, `FnSig`, `EnumSig` (replaced by the new types).
- Remove `lower_fn_sig` public API (internalize or reshape as needed for impl-block method lowering).

## Testing strategy

The existing test suites cover this refactoring well:

- **`sig_lower_query_tests.rs`** — directly tests signature lowering for functions, structs, enums. These will be migrated to call the new symbol-keyed queries instead of AST-keyed ones. Same assertions, different entry point.
- **`type_check_tests.rs`** — end-to-end tests (source → type check → errors). These should pass unchanged once the migration is complete, since only internal plumbing changes.
- **`sig_lower_tests.rs`** — tests type lowering internals, largely unaffected.
- **`ty_fold_tests.rs`** — tests `instantiate_struct_sig` which operates on `StructSig`/`FieldSig`. These will need migration to the new `VariantDef`/`FieldDef` types.

**New test needed:** a cross-module type check test. Currently all tests use a single file/module. The whole point of this RFD is correctness for cross-module references, so we should add at least one test where the struct's field types require resolution from the *defining* module's scope (not the caller's). Using only intrinsics like `u32` wouldn't catch the bug since those resolve without module context.

```rust
#[test]
fn cross_module_struct_field_access() {
    TestCrate::in_memory("mod other; fn f(w: other::Wrapper) -> other::Inner { w.value }")
        .file("other.rs", "pub struct Inner { pub x: u32 } pub struct Wrapper { pub value: Inner }")
        .check_ok();
}
```

This test would fail with the current architecture (caller passes its own module for resolution — `Inner` is not visible from the root module) and pass after the refactoring.
