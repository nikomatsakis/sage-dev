# RFD: Enum Variants as First-Class Symbols

**Status:** Completed

**Depends on:**
- [Per-Kind Symbol Data](./per-kind-symbol-data.md) — per-kind `SymbolData` enum
- [Type Signatures](./type-signatures.md) — per-kind symbol wrappers

## Goal

Make enum variants resolvable as first-class symbols. Today, variants are nested inside `EnumSigAst` as a `Slice<VariantDefAst>` and cannot be independently resolved. But Rust allows importing and referencing variants directly (`use Option::Some;`, `Some(42)`), so they need to be addressable through the symbol table.

## Problem

Enum variants are currently only accessible by looking up the parent enum's signature and searching through its variants. This means:

- `use Option::Some;` cannot resolve `Some` as a symbol — there's no symbol to find.
- Path resolution for `Option::Some(x)` must special-case "the last segment before the variant is an enum, look inside its signature." This leaks enum-specific knowledge into the resolver.
- External enum variants (from dependencies) are even harder — rustc's `module_children` on an enum's def_id returns its variants as `DefKind::Variant` (type+value namespace) and their constructors as `DefKind::Ctor(CtorOf::Variant, CtorKind::Fn)` (value namespace), but sage has no way to represent these.

## Current state

**Local enums:**
- `LocalEnumSym` is a salsa tracked struct; its CST (`EnumCstData`) contains `variants: Slice<VariantCst>` with name, fields, span.
- Variants are not separate symbols. They don't appear in namespace maps.
- The resolver doesn't support `Enum::Variant` path segments.

**External enums:**
- `SymExtKind` has `Enum` but no `Variant` or `VariantCtor`.
- `TcxDb::module_children()` is only called on modules. There is no `enum_children()` query.
- Rustc does expose variants via its `module_children` on enum def_ids, but sage never calls this.

**Resolver architecture:**
- `resolve_remaining_segments()` calls `s.module(db)` on each resolved symbol — only `ModSymbol` can be "entered" to look up children.
- This is the core bottleneck: the resolver has no concept of non-module containers.

## Design

### Approach: generalize the resolver to enter enums

The resolver's `resolve_remaining_segments` currently only accepts modules. We generalize it to handle any symbol that has "children" — initially just modules and enums, but the pattern extends naturally to traits (associated items) later.

Add a method on `Symbol` (or alongside `module()`):

```rust
impl<'db> Symbol<'db> {
    /// If this symbol can be entered for path resolution
    /// (modules, enums, traits), return its children.
    pub fn children(&self, db: &'db dyn Db) -> Option<&'db [Symbol<'db>]> {
        match self.data(db) {
            SymbolData::ModSymbol(m) => Some(m.expanded_module_items(db)),
            SymbolData::EnumSymbol(e) => Some(e.variants(db)),
            _ => None,
        }
    }
}
```

Then `resolve_remaining_segments` changes from filtering on `.module()` to using `.children()`:

```rust
fn resolve_remaining_segments(
    &mut self,
    stash: &Stash,
    symbols: Vec<Symbol<'db>>,
    rest: &[PathSegment<'db>],
    namespace: Namespace,
) -> Vec<Symbol<'db>> {
    symbols
        .into_iter()
        .flat_map(|s| {
            let children = s.children(self.db).unwrap_or_default();
            self.resolve_in_children(children, rest, namespace)
        })
        .collect()
}
```

This eliminates the module-specific codepath and makes the resolver naturally handle `Enum::Variant` without special-casing.

### New symbol kinds

**SymExtKind additions:**

```rust
pub enum SymExtKind {
    // ... existing ...
    Variant,        // enum variant (type namespace for struct variants, value for unit)
    VariantCtor,    // tuple variant constructor (value namespace, like TupleStructCtor)
}
```

**Local symbols:**

```rust
#[salsa::tracked(debug)]
pub struct LocalVariantSym<'db> {
    pub name: Name<'db>,
    pub parent_enum: LocalEnumSym<'db>,
    pub cst: VariantCst<'db>,
    pub span: RelativeSpan,
}

#[salsa::tracked(debug)]
pub struct LocalVariantCtorSym<'db> {
    pub variant: LocalVariantSym<'db>,
}
```

**SymbolData additions:**

```rust
pub enum VariantSymbol<'db> { Local(LocalVariantSym<'db>), Ext(SymExtKind::Variant) }
pub enum VariantCtorSymbol<'db> { Local(LocalVariantCtorSym<'db>), Ext(SymExtKind::VariantCtor) }
```

### Namespace placement

Following rustc's model, each variant shape produces different symbols:

- **Tuple variants** (e.g., `Some(T)`) — two symbols:
  - `VariantSymbol` in **type** namespace (for patterns, type contexts)
  - `VariantCtorSymbol` in **value** namespace (the callable constructor)
- **Struct variants** (e.g., `Error { msg: String }`) — one symbol:
  - `VariantSymbol` in **type** namespace (constructed with braces)
- **Unit variants** (e.g., `None`) — one symbol:
  - `VariantSymbol` in **value** namespace (it's a constant)

This mirrors how tuple structs work today: `Struct` in type-ns + `TupleStructCtor` in value-ns.

### EnumSymbol::variants() query

**Local enums:** A tracked function that iterates the CST's variant slice and mints symbols. Tuple variants produce both a `VariantSymbol` and a `VariantCtorSymbol`:

```rust
#[salsa::tracked(return_ref)]
pub fn enum_variants(db: &'db dyn Db, sym: LocalEnumSym<'db>) -> Vec<Symbol<'db>> {
    let mut symbols = Vec::new();
    for v in sym.cst(db).data(db).variants.iter() {
        let variant_sym = LocalVariantSym::new(db, v.name, sym, v, v.span);
        symbols.push(variant_sym.into());

        if v.is_tuple() {
            let ctor = LocalVariantCtorSym::new(db, v.name, variant_sym, v.span);
            symbols.push(ctor.into());
        }
    }
    symbols
}
```

**External enums:** Call `TcxDb::module_children(crate_num, def_index)` on the enum's DefId. Rustc's `module_children` works on enums and returns their variants — we just never called it for non-modules before.

```rust
impl<'db> EnumSymbol<'db> {
    pub fn variants(self, db: &'db dyn Db) -> &'db [Symbol<'db>] {
        match self {
            EnumSymbol::Local(sym) => enum_variants(db, sym),
            EnumSymbol::Ext(ext) => ext.expanded_module_items(db), // reuse existing infra!
        }
    }
}
```

The external case is elegant: `SymExt::expanded_module_items` already calls `module_children` and wraps results as `Symbol`. We just need to also call it for enum SymExts, not only module SymExts.

### Variant signatures

Variant symbols need signatures for type checking. The variant's "type" depends on its shape:

- **Unit variant `None`**: a constant of type `Option<T>` (the parent enum)
- **Tuple variant `Some(T)`**: a function `fn(T) -> Option<T>`
- **Struct variant `Error { msg: String }`**: no callable signature; constructed via struct literal syntax

This is downstream of this RFD — the immediate goal is *resolution*, not type-checking variants.

## Resolved questions

1. **Scope of variant visibility.** Variants are children of the enum only, not of the parent module. `use Enum::Variant` is supported by resolving `Enum` then looking up `Variant` in its children. Glob re-exports (`use Enum::*` bringing variants into the parent namespace) are a future concern.

2. **Constructor vs variant.** Separate symbols, mirroring the tuple-struct precedent. A tuple variant produces two symbols: a `VariantSymbol` in the type namespace (the variant as a type/pattern) and a `VariantCtorSymbol` in the value namespace (the callable constructor). This applies both locally and externally. Unit variants are value-namespace only (constants, no separate ctor). Struct variants are type-namespace only (brace-constructed, no callable).

3. **Unit variants.** They're values, not functions. `Symbol::name()` returns `Namespace::Value` for them. Their eventual signature is just the parent enum type (no parameters). No special representation needed beyond the namespace choice.

## Implementation plan

### Step 1: Local variant symbols and enum_variants query
- Define `LocalVariantSym` and `LocalVariantCtorSym` tracked structs
- Add `SymbolData::VariantSymbol` and `SymbolData::VariantCtorSymbol` variants with their kind-symbol types
- Implement `enum_variants()` tracked function (emits both variant + ctor for tuple variants)
- Wire `Symbol::name()`: VariantSymbol returns type-ns (struct variants) or value-ns (unit variants); VariantCtorSymbol always returns value-ns

### Step 2: SymExtKind::Variant for external enums
- Add `Variant` (and possibly `VariantCtor`) to `SymExtKind`
- Map rustc's `DefKind::Variant` and `DefKind::Ctor(CtorOf::Variant, _)` to these kinds
- Ensure `SymExt::expanded_module_items()` works for enum DefIds (it should already — `module_children` handles enums in rustc)

### Step 3: Generalize the resolver
- Add `Symbol::children()` method (returns variants for enums, items for modules)
- Refactor `resolve_remaining_segments` to use `children()` instead of `module()`
- Remove `resolve_remaining_segments_from_modules` or make it a thin wrapper

### Step 4: Tests and sage-emit updates
- Register enum and variant defs in sage-emit's `local_def_map` so oracle comparisons produce matching `NormalizedDef` values
- Add `rust-ref::DefKind::Variant` if needed for both oracle and sage to agree on variant DefPaths
- Write the test fixtures below

## Test fixtures

All fixtures are designed red/green: they fail before implementation (the resolver can't enter enums) and pass once the relevant steps land. Focus on **tuple variant calls** and **struct variant literals** for oracle comparison — the oracle doesn't handle unit variant paths yet.

### Baseline: enum in type position (should already pass)

```rust
// test-fixtures/oracle/enums/enum_type_position.rs
fn unwrap_or(opt: Option<u32>, default: u32) -> u32 {
    default
}
```

Verifies `Option<u32>` resolves in signatures before any variant work begins. If this fails, it reveals a pre-existing emit gap.

### B1: local tuple variant call (Steps 1+3)

```rust
// test-fixtures/oracle/enums/local_tuple_variant_call.rs
enum Wrapper {
    Val(u32),
}

fn wrap(x: u32) -> Wrapper {
    Wrapper::Val(x)
}
```

The canonical red test. `Wrapper::Val` requires the resolver to enter the enum and find the `VariantCtorSymbol` in value namespace. Exercises the full pipeline: parse → symbols → resolve → type-check → emit.

### B2: local struct variant literal (Steps 1+3)

```rust
// test-fixtures/oracle/enums/local_struct_variant_lit.rs
enum Message {
    Data { value: u32 },
}

fn make_message(v: u32) -> Message {
    Message::Data { value: v }
}
```

Struct variants are constructed with brace syntax. The resolver finds `Data` as a `VariantSymbol` in the type namespace inside `Message`.

### B3: use-import through enum (Steps 1+3)

```rust
// test-fixtures/oracle/enums/local_use_variant.rs
enum Container {
    Wrapped(u32),
}

use Container::Wrapped;

fn make_wrapped(x: u32) -> Container {
    Wrapped(x)
}
```

The `use Container::Wrapped` path must resolve through the enum. After the import, `Wrapped` is available unqualified in value namespace.

### C1: cross-module variant (Steps 1+3)

```rust
// test-fixtures/oracle/enums/cross_module_variant/src/lib.rs
mod types;

fn make_active(n: u32) -> types::Status {
    types::Status::Active(n)
}
```

```rust
// test-fixtures/oracle/enums/cross_module_variant/src/types.rs
pub enum Status {
    Active(u32),
}
```

Multi-segment resolution: `types` (module) → `Status` (enum) → `Active` (variant ctor). Tests that the generalized resolver handles the module→enum transition.

### A1: external enum variant (Steps 2+3)

```rust
// test-fixtures/oracle/enums/option_some_call.rs
fn wrap_value(x: u32) -> Option<u32> {
    Option::Some(x)
}
```

`Option::Some` resolves through an external enum. Requires `SymExtKind::Variant` and the `module_children` call on enum DefIds.

### D1: nonexistent variant error (Step 3)

```rust
// test-fixtures/oracle/enums/nonexistent_variant_error.rs
//# RUSTC ERROR
enum Color {
    Red,
}

fn bad() -> Color {
    Color::Blue //# ERROR
}
```

Once the resolver can enter enums, it should report an "unresolved name" error for a variant that doesn't exist.

### Caveats

- **sage-emit gap:** The emit layer doesn't register enum or variant defs in `local_def_map` today. Local-enum fixtures (B1, B2, B3, C1) require emit-layer updates in Step 4 for oracle comparison to match.
- **Unit variant paths:** The oracle's `ExprKind::Path` only handles `Fn | AssocFn` — unit variants like bare `None` fall through. Defer unit-variant oracle tests until the oracle is updated.
- **Prelude variants:** `Some(x)` without qualification works in Rust because the prelude re-exports variant constructors at module level. This depends on glob re-export logic, which is a non-goal of this RFD.

## Implementation deviations

1. **`LocalVariantSym` stores `cst: VariantCst<'db>` directly.** The plan sketched storing name + parent; we store the full CST value after deriving `salsa::Update` on `VariantCst`. All its fields (`Name`, `Slice`, `Option<Ptr<…>>`, `RelativeSpan`) already had `Update` impls.

2. **Single `expanded_module_items` for both modules and enums.** The plan showed a separate `expanded_enum_items`; in practice rustc's `module_children` works identically on both, so we removed the `assert_eq!(kind, Mod)` and reuse one tracked function.

3. **A1 test deferred.** `option_some_call.rs` requires external crate metadata but the test harness uses `NoopTcxDb`. The resolver infrastructure for external enum children is in place; testing it is blocked on the [Test Harness External Crates](./test-harness-external-crates.md) RFD.

4. **Enum-children resolution uses `resolve_in_children` (not `resolve_name_from_module`).** Modules need use-resolution and cycle detection; enum children are a flat list. The resolver has two paths rather than one generalized function.

5. **`TcxDb::structured_def_path` added.** Needed to produce proper `External(DefPath{krate, segments})` instead of `"?"` placeholders for external symbols in sage-emit. Not in the original plan but closes a pre-existing gap.

## Non-goals

- Changing how enum signatures are lowered to types (that stays as-is).
- Pattern matching / exhaustiveness — variants as symbols helps resolution, not match analysis.
- Glob re-exports (`use Enum::*`) — requires import expansion logic, separate concern.
- Variant type signatures for inference — downstream work, this RFD is about resolution.
