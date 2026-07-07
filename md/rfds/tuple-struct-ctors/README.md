# RFD: Tuple Struct Constructors in the Memmap

**Status:** Implemented (Steps 1–3); Step 4 (Signature) deferred to type-signatures RFD

**Depends on:**
- [Module sym tree](./module-sym-tree.md) — `MemmapEntry`, `ExpandedModule`, `Symbol`, `SymbolData`

## Goal

Tuple structs and unit structs have implicit constructors that live in the value namespace. `struct Foo(i32)` introduces `Foo` in both the type namespace (the struct itself) and the value namespace (a callable constructor `fn(i32) -> Foo`). Today the memmap says "struct → Type and Value" but there is no actual constructor entity — the value-namespace binding is a fiction maintained by `item_in_namespace`. This RFD makes the constructor a real memmap entry.

## Problem

Without an explicit constructor entry:

- Name resolution in the value namespace for `Foo` finds the `StructAst`, but there's nothing to ask for a callable signature. The type signature layer would need special-case logic: "if the resolved symbol is a tuple struct, synthesize a `FnSig` from its fields."
- The same special case leaks into type checking: any call expression `Foo(42)` must check "is the callee a tuple struct? If so, treat it as a function call."
- Enum variant constructors (`Foo::Bar(42)`) have the same shape and would need the same special cases.

Making the constructor a first-class entity in the memmap means the signature and type checking layers see a callable symbol and don't need to know about tuple structs at all.

## Design

### New memmap entry

```rust
enum MemmapEntry<'db> {
    Item(ItemAst<'db>),

    /// Implicit constructor for a tuple struct or unit struct.
    /// Lives in the value namespace. The `StructAst` provides the
    /// field types from which a `FnSig` is derived.
    TupleStructCtor(LocalStructSym<'db>),

    MacroDef(LocalMacroDefSym<'db>),
    Redirect { name: Name<'db>, target: Path<'db> },
    Glob { path: Path<'db> },
    MacroUse(MacroUse<'db>),
}
```

The entry's name is the struct's name. It occupies `Namespace::Value` only — the struct's `Item(ItemAst::Struct(...))` entry continues to occupy `Namespace::Type` (and no longer claims `Value`).

### Emission during expansion

The memmap seeding phase already walks `syntax_items` and emits `MemmapEntry::Item(...)` for each item. When it encounters a `StructAst` whose fields are positional (tuple struct) or empty (unit struct), it emits an additional `TupleStructCtor(struct_ast)` entry. Braced structs (`struct Foo { x: i32 }`) do not get a constructor entry — their value-namespace presence is via struct literal syntax, not a callable.

Detecting tuple vs braced: a `StructKind` enum (`Tuple`, `Unit`, `Braced`) is stored as a tracked field on `StructAst`. The lowering sets it from the tree-sitter body node kind: `field_declaration_list` → `Braced`, no body → `Unit`, otherwise (e.g. `ordered_field_declaration_list`) → `Tuple`.

### Symbol representation

`TupleStructCtor` needs to be representable as a `Symbol` so it can appear in `Res::Def(...)` when a value-namespace path resolves to it.

**Chosen: Option A — new `SymbolData` variant.** The current `SymbolData` is flat (no per-kind wrappers yet), so the variant wraps `StructAst` directly:

```rust
pub enum SymbolData<'db> {
    Ast(ItemAst<'db>),
    TupleStructCtor(LocalStructSym<'db>),  // new
    Ext(SymExt),
}
```

This is explicit — callers that match on `SymbolData` see the constructor as a distinct kind. When per-kind symbol wrappers (`StructSymbol`, etc.) are introduced by the type-signatures RFD, the variant will wrap `StructSymbol<'db>` instead.

**Rejected: Option B (model as `FnSymbol`).** The constructor *is* a function, but `FnSymbol` wraps `FnAst | SymExt`, and there's no `FnAst` for a synthesized constructor. This would require a synthetic `FnAst` or a third arm, muddling the abstraction.

### Enum variant constructors

Enum variants with fields (`Foo::Bar(i32)`) have the same shape — a constructor in the value namespace. These are scoped under the enum, not at module level, so they don't appear as memmap entries. Instead, when resolving a path like `Foo::Bar`, the resolution walks into the enum and finds the variant. The variant constructor is modeled similarly — a `SymbolData::VariantCtor(EnumSymbol, VariantIndex)` or similar.

This is out of scope for this RFD but follows the same pattern. Noting it here so the `SymbolData` design leaves room.

### Namespace changes

Today `item_in_namespace` returns both `Type` and `Value` for structs. After this change:

- `Item(ItemAst::Struct(...))` → `Namespace::Type` only (for all structs)
- `TupleStructCtor(...)` → `Namespace::Value` only (for tuple/unit structs)
- Braced structs have no value-namespace entry (struct literals are not name-resolved as callables)

## Implementation plan

Every step follows **test-first**: write the tests, verify they fail (compile error or assertion failure), then write the implementation to make them pass. Tests use the existing `setup_single_file` / `setup_files` helpers and `resolve_name` with explicit `Namespace` arguments, matching the patterns in `memmap_tests.rs`.

### Step 1: Detect tuple vs braced structs ✓

**Tests first.** Write tests that parse tuple, unit, and braced structs and assert on the struct kind — these fail initially (the discriminant doesn't exist):
1. `struct Foo(i32, String);` → `StructAst` reports tuple struct.
2. `struct Bar;` → `StructAst` reports unit struct.
3. `struct Baz { x: i32 }` → `StructAst` reports braced struct.

**Implementation.** Added `StructKind` enum (`Tuple`, `Unit`, `Braced`) and a `kind: StructKind` tracked field on `StructAst`. The lowering (`lower_struct`) determines the kind from the tree-sitter body node: no body → `Unit`, `field_declaration_list` → `Braced`, otherwise → `Tuple`.

**Verify.** All three tests pass: `struct_kind_tuple`, `struct_kind_unit`, `struct_kind_braced`.

### Step 2: Emit `TupleStructCtor` during seeding ✓

**Tests first.** Write memmap tests that assert on the entries emitted for tuple vs braced structs — these fail initially (the `TupleStructCtor` variant doesn't exist):
1. `struct Foo(i32);` → the expanded module contains both `Item(Struct("Foo"))` and `TupleStructCtor(Struct("Foo"))`.
2. `struct Bar;` → same: both `Item` and `TupleStructCtor`.
3. `struct Baz { x: i32 }` → only `Item(Struct("Baz"))`, no `TupleStructCtor`.

**Implementation.** Added `TupleStructCtor(LocalStructSym<'db>)` to `MemmapEntry`. In `seed_from_items`, after emitting `MemmapEntry::Item` for a struct, the seeder checks `s.kind(db)` and emits an additional `TupleStructCtor(s)` for `Tuple` or `Unit` kinds. Updated `item_in_namespace` so `ItemAst::Struct` is `Namespace::Type` only (no longer claims `Value`). The `TupleStructCtor` entry handles `Namespace::Value` instead. Updated `collect_all_names` in `validate.rs` to include the constructor in the value namespace. No separate `entry_in_namespace` function was needed — the match arm in `resolve_member`'s walk handles it directly.

**Verify.** Tests pass: `tuple_struct_emits_ctor_entry`, `unit_struct_emits_ctor_entry`, `braced_struct_no_ctor_entry`. Existing snapshot tests updated (unit structs now show `TupleStructCtor` entries).

### Step 3: Name resolution ✓

**Tests first.** Write resolution tests — these fail initially (resolution doesn't know about `TupleStructCtor`):
1. `struct Foo(i32);` — resolve `Foo` in `Namespace::Value` → succeeds, returns a `TupleStructCtor` symbol.
2. `struct Foo(i32);` — resolve `Foo` in `Namespace::Type` → succeeds, returns the struct symbol.
3. `struct Bar { x: i32 }` — resolve `Bar` in `Namespace::Value` → fails (no value-namespace entry for braced structs).
4. `struct Bar { x: i32 }` — resolve `Bar` in `Namespace::Type` → succeeds.

**Implementation.** Added `TupleStructCtor(LocalStructSym<'db>)` variant to `SymbolData` and a `Symbol::tuple_struct_ctor(s)` constructor. In `resolve_member`'s `walk` function, added a match arm for `MemmapEntry::TupleStructCtor(s)` that checks `s.name(db) == name && matches!(ns, Namespace::Value)` and pushes a `Symbol::tuple_struct_ctor(*s)`. Per-kind symbol wrappers (like `StructSymbol`) are not yet implemented — the variant wraps `StructAst` directly, matching the current flat `SymbolData` design. Updated display/formatting in `display.rs` and test helpers in `common/mod.rs` to handle the new variant.

**Verify.** Tests pass: `tuple_struct_resolves_in_value_ns`, `tuple_struct_resolves_in_type_ns`, `braced_struct_not_in_value_ns`, `braced_struct_resolves_in_type_ns`. All existing resolution tests still pass.

**Note on test 5 (mini-redis integration):** Skipped as a separate test — the mini-redis snapshot tests already exercise the new code path since mini-redis contains unit structs whose memmap entries now include `TupleStructCtor`.

### Step 4: Signature (deferred)

Depends on the [type-signatures RFD](./type-signatures.md) — `FnSig`, `StructSig`, `Ty`, and the signature query infrastructure are not yet implemented.

**Tests first.** Write signature tests — these fail initially (the signature query for constructors doesn't exist):
1. `struct Foo(i32, String);` → the `TupleStructCtor` symbol's signature is `FnSig { params: [Int(I32), Adt(String, [])], ret: Adt(Foo, []) }`.
2. `struct Pair<A, B>(A, B);` → the constructor signature is `Binder { bound_vars: [Type, Type], value: FnSig { params: [BoundVar(0,0), BoundVar(0,1)], ret: Adt(Pair, [BoundVar(0,0), BoundVar(0,1)]) } }`.
3. `struct Unit;` → the constructor signature is `FnSig { params: [], ret: Adt(Unit, []) }`.

**Implementation.** The signature for a `TupleStructCtor` is a `FnSig` — params are the struct's field types (in order), return type is the struct type (with its generic args as `BoundVar`s, wrapped in the same `Binder` as the struct's own signature). This query derives the `FnSig` from the struct's `StructSig` — no separate lowering from `TypeRefAst`, just a transformation of already-resolved types.

**Verify.** All new tests pass.

## Scope

**In scope:**
- `TupleStructCtor` memmap entry for tuple and unit structs
- Value-namespace resolution returning a constructor symbol
- Signature derivation (FnSig from struct fields)

**Out of scope:**
- Enum variant constructors (same pattern, different scoping)
- Braced struct literals (not a constructor call)
