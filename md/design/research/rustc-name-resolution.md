# Rustc Name Resolution: Research Report

This report documents how name resolution works in rustc, with focus on Rust 2024
edition semantics. It covers namespace structure, macro resolution (derives, attribute
macros, bang macros), cross-crate concerns, expansion ordering, globs, and preludes.

Source base: `~/dev/rust/compiler/`

## 1. Namespace Structure

**Source:** `rustc_hir/src/def.rs:687-700`

Rust has exactly **three namespaces**:

```rust
pub enum Namespace {
    TypeNS,   // structs, enums, unions, traits, mods, type aliases
    ValueNS,  // fns, consts, statics, local variables, enum variant ctors
    MacroNS,  // all macros (bang, attr, derive) + non-macro attributes
}
```

All macros — regardless of kind — live in `MacroNS`. This includes `macro_rules!`
items, `pub macro` items, proc-macro bang/attr/derive macros, and even inert
attributes like `#[inline]`.

## 2. Sub-Namespace Splitting Within MacroNS

**Source:** `rustc_resolve/src/macros.rs:85-102`

Although all macros share `MacroNS`, the resolver applies a **sub-namespace filter**:

```rust
/// Macro namespace is separated into two sub-namespaces, one for bang macros and
/// one for attribute-like macros (attributes, derives).
/// We ignore resolutions from one sub-namespace when searching names in scope for another.
pub(crate) fn sub_namespace_match(
    candidate: Option<MacroKinds>,
    requirement: Option<MacroKind>,
) -> bool {
    let (Some(candidate), Some(requirement)) = (candidate, requirement) else {
        return true;
    };
    match requirement {
        MacroKind::Bang => candidate.contains(MacroKinds::BANG),
        MacroKind::Attr | MacroKind::Derive => {
            candidate.intersects(MacroKinds::ATTR | MacroKinds::DERIVE)
        }
    }
}
```

Key insight: **Attr macros and Derive macros share one sub-namespace; Bang macros have
their own.** A derive macro will not shadow a bang macro of the same name, and vice
versa. However, an attr macro and a derive macro of the same name *can* shadow each
other.

The `MacroKinds` bitflags (`rustc_hir/src/def.rs:38-44`):

```rust
pub struct MacroKinds(u8);
bitflags! {
    impl MacroKinds: u8 {
        const BANG = 1 << 0;
        const ATTR = 1 << 1;
        const DERIVE = 1 << 2;
    }
}
```

A single definition can support multiple kinds simultaneously — e.g., a
`macro_rules!` macro can be usable as both a bang and an attr macro. The `DefKind`
stores `Macro(MacroKinds)` with the full bitflag set.

## 3. The `MacroKind` Enum

**Source:** `rustc_span/src/hygiene.rs:1145-1176`

```rust
pub enum MacroKind {
    Bang,   // foo!()
    Attr,   // #[foo]
    Derive, // #[derive(Foo)]
}
```

This is used as the "requirement" when resolving a macro invocation — it determines
which sub-namespace filter is applied.

## 4. How `macro_rules!` Macros Are Resolved

### Textual (Lexical) Scoping

**Source:** `rustc_resolve/src/macros.rs:50-83`

`macro_rules!` macros without `#[macro_export]` live in a **textual scope chain**,
not in the module's `MacroNS` bindings:

```rust
pub(crate) enum MacroRulesScope<'ra> {
    Empty,
    Def(&'ra MacroRulesDecl<'ra>),
    Invocation(LocalExpnId),
}
```

When a `macro_rules!` definition is encountered, it creates a
`MacroRulesScope::Def` linking back to the prior scope. This covers everything
*textually after* the definition within the same block/module.

**Source:** `rustc_resolve/src/build_reduced_graph.rs:1322-1375`

For non-exported `macro_rules!`:
- The macro is NOT placed into the module's `MacroNS` bindings.
- Instead, it's placed into the textual `MacroRulesScope` chain.
- It uses `SemiOpaque` transparency.

### Module Scoping via `#[macro_export]`

**Source:** `rustc_resolve/src/build_reduced_graph.rs:1339-1360`

When `#[macro_export]` is present:
- A `MacroExport` import is created with the **crate root** as the parent module.
- The macro becomes accessible via `use crate::macro_name;` from any edition.

### Rust 2024 Changes

The `OUT_OF_SCOPE_MACRO_CALLS` lint (deny-by-default, future-incompatible) prevents
calling `macro_rules!` macros that are textually in scope via the `MacroRulesScope`
chain when they are inside a module that contains an inert attribute
(`macros.rs:1159-1217`). This tightens scoping rules in Rust 2024.

## 5. How `pub macro` (Declarative Macros 2.0) Are Resolved

**Source:** `rustc_resolve/src/build_reduced_graph.rs:1376-1393`

`pub macro foo` items:
- Placed directly into the parent module's `MacroNS` via `define_local`.
- Subject to normal visibility rules.
- Resolved like any other named item in the module.
- Use `Opaque` transparency (definition-site hygiene).
- Can be imported via `use` normally.

## 6. How Proc-Macro Bang Macros Are Resolved

**Source:** `rustc_resolve/src/build_reduced_graph.rs:1261-1278`

`#[proc_macro] fn my_macro(...)` creates a `DefKind::Macro(MacroKinds::BANG)` item
in the defining crate's `MacroNS`. From external crates, it's discovered via crate
metadata and made accessible through `use crate_name::my_macro;`.

Resolution path for `my_macro!(...)` (`macros.rs:267-282`):
- Single-segment: uses `ScopeSet::Macro(MacroKind::Bang)`, walking the full scope
  chain with the bang sub-namespace filter.
- Multi-segment (`crate::my_macro!`): uses standard path resolution in `MacroNS`.

## 7. How Derive Macros Are Resolved

**Source:** `rustc_resolve/src/macros.rs:386-472`

When the compiler encounters `#[derive(MyDerive)]`:

1. `resolve_derives` is called for the entire `#[derive(...)]` container.
2. Each derive path is resolved via `resolve_derive_macro_path` (lines 773-790),
   using `MacroKind::Derive`.
3. Resolution blocks the container's expansion until ALL derives resolve —
   required because derive helper attributes must be in scope for the annotated item.
4. After resolution, helper attributes are collected and stored in `helper_attrs`,
   making them available as `Scope::DeriveHelpers`.

The derive name is resolved identically to other macro paths — as a single-segment
name through the full scope chain with `MacroKind::Derive` sub-namespace filter, or
as a multi-segment path in `MacroNS`.

### Derive Helper Attributes

When a derive macro declares helper attributes (e.g.,
`#[proc_macro_derive(Serialize, attributes(serde))]`), those helpers become available
as `Scope::DeriveHelpers` for the annotated item. They resolve as `NonMacroAttr`
items in the `MacroNS`.

## 8. How Attribute Macros Are Resolved

**Source:** `rustc_resolve/src/macros.rs:268-271`

```rust
InvocationKind::Attr { ref attr, derives: ref attr_derives, .. } => {
    derives = self.arenas.alloc_ast_paths(attr_derives);
    inner_attr = attr.style == ast::AttrStyle::Inner;
    (&attr.get_normal_item().path, MacroKind::Attr)
}
```

Attribute resolution uses `MacroKind::Attr` as the sub-namespace requirement. Since
attr and derive macros share a sub-namespace, they can shadow each other.

## 9. The Full Scope Search Order (for single-segment names)

**Source:** `rustc_resolve/src/ident.rs:50-237`

### Type Namespace

1. **Ribs** (type parameters, `impl` self types)
2. **Module non-glob bindings** (local definitions + named imports)
3. **Module glob bindings** (glob imports)
4. Walk up to **hygienic lexical parent** module, repeat steps 2-3
5. **Extern prelude items** (from `extern crate` declarations)
6. **Extern prelude flags** (from `--extern` command-line flags)
7. **Tool prelude** (from `#![register_tool]`)
8. **Standard library prelude** (edition-specific prelude module)
9. **Builtin types** (`u32`, `bool`, `str`, `f16`, `f128`, etc.)

### Value Namespace

1. **Ribs** (local variables, function parameters)
2. **Module non-glob bindings**
3. **Module glob bindings**
4. Walk up to **hygienic lexical parent** module, repeat steps 2-3
5. **Standard library prelude**

### Macro Namespace

1. **Derive helpers** (`Scope::DeriveHelpers`) — from derives on the current item
2. **Derive helpers compat** (`Scope::DeriveHelpersCompat`) — legacy support
3. **`macro_rules!` scopes** (`Scope::MacroRules`) — textual scope chain
4. **Module non-glob bindings**
5. **Module glob bindings**
6. Walk up to **hygienic lexical parent**, repeat steps 3-5
7. **`macro_use` prelude** (`Scope::MacroUsePrelude`) — from `#[macro_use] extern crate`
8. **Standard library prelude** (`Scope::StdLibPrelude`)
9. **Builtin attributes** (`Scope::BuiltinAttrs`) — `#[inline]`, `#[derive]`, etc.

Note: In Rust 2015, the `MacroUsePrelude` scope is always searched. In 2018+/2024,
it respects `#[no_implicit_prelude]` (`ident.rs:154`).

## 10. Cross-Crate Macro Resolution

### `#[macro_export]` Macros

**Source:** `rustc_resolve/src/build_reduced_graph.rs:1325-1360`

`#[macro_export] macro_rules!` items appear at the crate root in `MacroNS`.
External crates access them via `use crate_name::macro_name;` or
`crate_name::macro_name!()`.

When an external crate's module is first accessed, `build_reduced_graph_external`
(`build_reduced_graph.rs:350`) plants all children (including
`Res::Def(DefKind::Macro(..), _)`) into `MacroNS`.

### Proc Macros from External Crates

**Source:** `rustc_metadata/src/rmeta/decoder.rs:1059`, `rustc_metadata/src/creader.rs:643-728`

- Proc-macro crates are loaded as dylibs.
- The `ProcMacroClient` list is extracted via dlsym.
- Each proc macro is loaded lazily: when a `DefId` is first resolved to
  `Res::Def(DefKind::Macro(..), def_id)`, `get_macro_by_def_id` loads and caches
  the `SyntaxExtension`.
- From the consumer's perspective, proc macros are just names in `MacroNS` of
  the external crate root — resolved via `use`.

## 11. Determinacy and the Fixed-Point Expansion Loop

### The `Determinacy` Enum

**Source:** `rustc_resolve/src/lib.rs:97-106`

```rust
enum Determinacy {
    Determined,
    Undetermined,
}
```

- **`Determined`**: Resolution is conclusive — name found or definitively absent.
- **`Undetermined`**: A pending macro expansion might introduce the name. Must retry.

### The Expansion Loop

**Source:** `rustc_expand/src/expand.rs:473-616`

`fully_expand_fragment` drives the mutual fixed-point:

1. Collect all macro invocations, replace with placeholders.
2. Call `resolve_imports()` — eagerly resolve available imports.
3. Main loop:
   - Pop an invocation. Attempt resolution.
   - If `Ok(ext)`: expand, collect new sub-invocations, mark progress.
   - If `Err(Indeterminate)`: push to `undetermined_invocations`.
   - When queue empty: call `resolve_imports()` again.
   - If undetermined remain and no progress: enter **force mode**.
4. Force mode treats all `Undetermined` as `Determined` failures, allowing error
   recovery with dummy macro output.

### "Time Travelling" — `MacroRulesScope::Invocation`

**Source:** `rustc_resolve/src/macros.rs:67-83`, `ident.rs:592-598`

When a macro invocation is placed into a module, the resolver creates a
`MacroRulesScope::Invocation(invoc_id)` **barrier** in the textual scope chain.

During name lookup, hitting an unexpanded `Invocation` barrier returns
`Undetermined` — because that invocation *might* produce a `macro_rules!` definition
that introduces the name being sought.

After expansion, the invocation's output `macro_rules` scope is stored in
`output_macro_rules_scopes`, and path compression replaces the barrier with the
actual output for subsequent lookups.

### Import Resolution Fixed-Point

**Source:** `rustc_resolve/src/imports.rs:729-751`

`resolve_imports` is itself a fixed-point loop:
- Each iteration speculatively resolves indeterminate imports.
- Resolved imports are committed via `write_import_resolutions`.
- Loop terminates when `indeterminate_count` stops decreasing.

The two systems form a mutual fixed-point: imports may depend on macro expansion
(a macro expanding to a `use` statement), and macro resolution may depend on imports
(a `use` bringing a macro into scope).

### Finalization Consistency Check

**Source:** `rustc_resolve/src/macros.rs:900-1033`

After all expansion, `finalize_macro_resolutions` re-resolves every macro path and
checks consistency with the resolution used during expansion. If a resolution changed,
a delayed bug is emitted.

## 12. Glob Imports

### Resolution Mechanism

**Source:** `rustc_resolve/src/imports.rs:1749-1783`

When `use foo::*` resolves:
1. Iterate all `determined_decl()` entries in the source module.
2. For each accessible binding, create an import declaration.
3. Plant into the importing module via `try_plant_decl_into_local_module()`.

### Priority: Non-Glob Always Wins

**Source:** `rustc_resolve/src/imports.rs:284-321`

The `NameResolution` structure maintains two slots:
- `non_glob_decl: Option<Decl>` — item definitions and named imports
- `glob_decl: Option<Decl>` — glob imports

`best_decl()` returns `self.non_glob_decl.or(self.glob_decl)` — non-glob always wins.

Additionally, if there are pending `single_imports` that might define a name, the
glob result is blocked (`determined_decl()` returns `None`) until those resolve.

### Glob Determinacy and Macro Expansion

A glob binding is NOT determined when its source module has `unexpanded_invocations`
(`DeclData::determined()` at `lib.rs:1258`). This means during expansion,
`resolve_ident_in_module_globs_unadjusted()` returns `Err(Undetermined)` rather than
committing to a glob result that might change.

### Within-Module Scope Separation

Globs and non-globs are visited as separate scopes:
```
Scope::ModuleNonGlobs(module) => Scope::ModuleGlobs(module)
```
This structural separation enforces the priority rule at every step of the scope walk.

## 13. The Prelude System

### Standard Library Prelude (`Scope::StdLibPrelude`)

- Discovered via `#[prelude_import]` during `build_reduced_graph`
  (`build_reduced_graph.rs:728`).
- Stored as `self.prelude: Option<Module<'ra>>`.
- Gated by `use_prelude` (false for modules with `#[no_implicit_prelude]`).
- Lowest priority in the scope chain (after all module scopes and extern prelude).

### Extern Prelude

Split into two scopes for priority:
- **`ExternPreludeItems`**: from `extern crate` declarations (higher priority).
- **`ExternPreludeFlags`**: from `--extern` CLI flags (lower priority).
- Always includes `core` (unless `#![no_core]`) and `std` (unless `#![no_std]`).

### Tool Prelude (`Scope::ToolPrelude`)

- From `#![register_tool(foo)]`.
- Maps tool names to `Res::ToolMod`.
- Enables `#[rustfmt::skip]`, `#[clippy::allow]` etc.

### Macro Use Prelude (`Scope::MacroUsePrelude`)

- From `#[macro_use] extern crate foo;`.
- Only for `MacroNS`.
- In 2015: always searched. In 2018+/2024: respects `#[no_implicit_prelude]`.

### Builtin Types (`Scope::BuiltinTypes`)

- Primitive types: all `PrimTy` variants.
- Only for `TypeNS`, absolute lowest priority.

### Builtin Attributes (`Scope::BuiltinAttrs`)

- All items from `BUILTIN_ATTRIBUTES` (e.g., `derive`, `cfg`, `test`).
- Only for `MacroNS`, lowest priority in that namespace.

## 14. Edition 2024 Specifics

### New Prelude Items

**Source:** `library/core/src/prelude/mod.rs:56-72`, `library/std/src/prelude/mod.rs:158-172`

Rust 2024 prelude adds to the 2021 prelude:
- `core::future::Future`
- `core::future::IntoFuture`

### `gen` Keyword

**Source:** `rustc_span/src/symbol.rs:112,2960`

`gen` is reserved as an unused keyword in edition 2024+. Cannot be used as an
identifier without `r#gen`.

### Resolution Order Unchanged

The resolution priority ordering algorithm is **identical across all editions**. What
changes:
1. The **contents** of the stdlib prelude module (more items in 2024).
2. **Keyword** reservations (`gen`).
3. How `::path` is interpreted (crate root in 2015, extern prelude in 2018+).
4. `MacroUsePrelude` search behavior (unconditional in 2015, conditional in 2018+).

## 15. Implications for Sage

### Namespace Model

The current sage `Namespace` enum:
```rust
pub enum Namespace {
    Type,
    Value,
    Macro,
}
```

This is correct as the top-level structure. However, rustc's MacroNS has internal
sub-namespace filtering via `MacroKinds`. When resolving a name, the resolver must
know *what kind of macro* is being sought:

- Resolving `foo!()` → filter to `MacroKinds::BANG`
- Resolving `#[foo]` → filter to `MacroKinds::ATTR | MacroKinds::DERIVE`
- Resolving `#[derive(Foo)]` → filter to `MacroKinds::ATTR | MacroKinds::DERIVE`

A single `Namespace::Macro` variant (without sub-kind) works for module-level
storage. The sub-namespace filtering should happen at lookup time, not at storage
time.

Options for modeling this:
1. **Keep `Namespace::Macro` singular** but pass an optional `MacroKind` as a
   filter parameter to resolution functions. This matches rustc's approach.
2. **Split into `Namespace::Macro(MacroKind)`** with three variants. This over-splits
   because attr and derive share a sub-namespace, and would require storing macros
   in up to two namespace slots (e.g., a `macro_rules!` that works as both bang and
   attr).

Option 1 is recommended.

### Scope Chain Priorities

The scope walk for macro resolution must handle:
1. Derive helper attributes (contextual, on the current item)
2. Textual `macro_rules!` scope (linked list, not module-based)
3. Module non-glob bindings
4. Module glob bindings
5. Walk up through parent modules
6. Prelude(s)

### Determinacy

When a glob import's source module has pending macro expansions, the glob result is
indeterminate. This is essential for correctness of the fixed-point loop — a glob
cannot be committed while the source module might still grow.

## Key Source Files

| File | Role |
|------|------|
| `rustc_hir/src/def.rs` | `Namespace`, `MacroKind`, `MacroKinds`, `DefKind` |
| `rustc_resolve/src/ident.rs` | Scope visitor, resolution priority, sub-namespace filtering |
| `rustc_resolve/src/imports.rs` | Import resolution fixed-point, `NameResolution`, globs |
| `rustc_resolve/src/macros.rs` | `MacroRulesScope`, `sub_namespace_match`, macro resolution |
| `rustc_resolve/src/build_reduced_graph.rs` | Module tree building, prelude discovery, `#[macro_export]` |
| `rustc_resolve/src/lib.rs` | `Determinacy`, `Scope` enum, `AmbiguityKind` |
| `rustc_expand/src/expand.rs` | `fully_expand_fragment` — the expansion/resolution loop |
| `rustc_expand/src/base.rs` | `SyntaxExtensionKind`, `Indeterminate` |
| `rustc_metadata/src/rmeta/decoder.rs` | Loading proc macros from external crate metadata |
| `library/std/src/prelude/mod.rs` | Edition-specific prelude contents |
