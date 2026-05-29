# RFD: Enum Variants as First-Class Symbols

**Status:** Draft

**Depends on:**
- [Per-Kind Symbol Data](./per-kind-symbol-data.md) — per-kind `SymbolData` enum
- [Type Signatures](./type-signatures.md) — per-kind symbol wrappers

## Goal

Make enum variants resolvable as first-class symbols. Today, variants are nested inside `EnumSigAst` as a `Slice<VariantDefAst>` and cannot be independently resolved. But Rust allows importing and referencing variants directly (`use Option::Some;`, `Some(42)`), so they need to be addressable through the symbol table.

## Problem

Enum variants are currently only accessible by looking up the parent enum's signature and searching through its variants. This means:

- `use Option::Some;` cannot resolve `Some` as a symbol — there's no symbol to find.
- Path resolution for `Option::Some(x)` must special-case "the last segment before the variant is an enum, look inside its signature." This leaks enum-specific knowledge into the resolver.
- External enum variants (from dependencies) are even harder — rustc's `module_children` on an enum's def_id returns its variants as `DefKind::Variant` (type+value namespace) and their constructors as `DefKind::Ctor(CtorOf::Variant, CtorKind::Fn)` (value namespace), but sage-dev has no way to represent these.

## Current state

**Local enums:**
- `EnumSigAst` contains `Slice<VariantDefAst<'db>>` with name, fields, and span.
- Variants are not memmap entries. They don't appear in namespace maps.
- The resolver doesn't support `Enum::Variant` path segments.

**External enums:**
- `TcxDb::module_children()` is only valid on modules. There is no `enum_variants()` or `enum_children()` query.
- Rustc does expose variants via its `module_children` on enum def_ids, but sage-dev never calls this.

## Design sketch

### New symbol type

```rust
pub struct VariantSymbol<'db> { /* Ast(VariantDefAst) | Ext(SymExt) */ }
```

Add a `SymbolData::Variant(VariantSymbol<'db>)` arm to the per-kind enum (from the per-kind-symbol-data RFD).

### TcxDb extension

Add an `enum_children` (or `enum_variants`) query:

```rust
fn enum_children(&self, crate_num: CrateNum, def_index: DefIndex) -> Vec<RawChild>;
```

This calls rustc's `module_children` on the enum's def_id and returns `DefKind::Variant` entries. Each variant gets `SymExtKind::Variant`.

### Local variants as symbols

When building the memmap for a local enum, emit variant entries alongside the enum itself. Variants live in both the type and value namespaces (mirroring rustc). Tuple variants additionally get a constructor entry in the value namespace (analogous to `TupleStructCtor` for structs).

### Path resolution

When resolving `Enum::Variant`:
1. Resolve `Enum` → get an `EnumSymbol`.
2. Look up `Variant` in the enum's children (local: from the memmap or signature; external: from `enum_children()`).

This replaces the current approach of searching through `EnumSigAst` fields.

## Open questions

1. **Scope of variant visibility.** Should variants appear as children of their parent module (like in Rust 2018+ for `pub` enums via glob re-exports)? Or only as children of the enum itself?

2. **Constructor vs variant.** Rustc distinguishes `DefKind::Variant` (the variant as a type) from `DefKind::Ctor(CtorOf::Variant, CtorKind::Fn)` (the callable constructor). Do we need both, or can `VariantSymbol` serve both roles (like how `TupleStructCtor` wraps a `StructAst`)?

3. **Unit variants.** Unit variants like `None` are `Ctor(CtorOf::Variant, CtorKind::Const)` in rustc. They're values, not functions. How should their signature be represented?

## Non-goals

- Changing how enum signatures are lowered to types (that stays as-is).
- Pattern matching / exhaustiveness — variants as symbols helps resolution, not match analysis.
