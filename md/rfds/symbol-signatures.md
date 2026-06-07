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
pub fn struct_sig<'db>(
    db: &'db dyn Db,
    sym: StructSymbol<'db>,
    source_root: SourceRoot,
) -> Stashed<Binder<'db, StructSig<'db>>>;
```

The type checker calls `struct_sig(db, struct_sym, source_root)` without needing to know whether the symbol is local or external.

## Design questions

### Where does `module` come from for local symbols?

The existing `struct_signature` needs the module to resolve type paths in the struct's field types (e.g., `field: OtherStruct` must resolve `OtherStruct` in the struct's defining scope).

Options:

1. **Store the parent module on the symbol at creation time.** When the memmap creates a `StructSymbol`, it knows which module it belongs to. Add a tracked field or side-table mapping `StructAst → ModSymbol`.

2. **Derive from the AST's source file.** `StructAst` has a `span` → `SourceFile`. We could look up which module owns that file. But a file can back multiple modules (inline modules), so this is ambiguous.

3. **A separate tracked query** `module_of(db, ItemAst) -> ModSymbol` that the signature query calls internally. This query would be populated during module expansion (the memmap phase).

Option (3) feels cleanest — it separates concerns and doesn't require changing the `StructAst` tracked struct.

### External symbols

For external structs (from rustc metadata), the signature query would:
- Query `TcxDb` for the struct's fields and generic parameters
- Build a `Stashed<Binder<StructSig>>` from that data
- Cache via salsa as usual

This requires extending `TcxDb` with something like:
```rust
fn struct_fields(&self, crate_num: CrateNum, def_index: DefIndex) -> Vec<ExternalField>;
fn item_generics(&self, crate_num: CrateNum, def_index: DefIndex) -> Vec<ExternalGenericParam>;
```

### Scope of this RFD

This pattern applies to all item kinds:
- `fn_sig(db, FnSymbol, source_root)` — params + return type
- `struct_sig(db, StructSymbol, source_root)` — fields
- `enum_sig(db, EnumSymbol, source_root)` — variants + fields

The design should be uniform across all three.

## Current workaround

The type checker (`check.rs`) currently calls `struct_sym.as_ast().unwrap()` and passes its own module. This works for single-file in-memory tests but is architecturally wrong. The `as_ast()` call should be replaced once this RFD lands.
