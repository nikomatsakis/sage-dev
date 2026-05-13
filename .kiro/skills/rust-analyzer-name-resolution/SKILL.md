---
name: rust-analyzer-name-resolution
description: "Summary of rust-analyzer's name resolution and macro expansion. Use when designing sage's name resolution, understanding how a salsa-based system handles the macro expansion + name resolution interaction, or comparing approaches."
---

# rust-analyzer Name Resolution

Source: `crates/hir-def/src/nameres/` and `crates/hir-expand/src/`.

## Overview

rust-analyzer builds a `DefMap` per crate (and per block scope) that
contains fully macro-expanded module contents. Unlike rustc's two-phase
model, r-a does everything in a single fixed-point loop: import
resolution, macro expansion, and item collection all interleave.

The `DefMap` is computed by a salsa query (`crate_def_map`), so it's
incrementally recomputed when source changes. Body-level resolution
happens separately via the `Resolver` struct, which reads from the
frozen `DefMap`.

**For sage**: r-a proves that a salsa-based fixed-point for macro
expansion + name resolution is viable. The key is that the `DefMap`
query does the fixed-point internally, and downstream queries see
only the final result.

## DefMap and ItemScope

`DefMap` (`nameres.rs` ~line 173) holds:
- `modules: ModulesMap` — tree of `ModuleData`
- `prelude` — the std/core prelude module
- `macro_use_prelude` — macros from `#[macro_use]` extern crates
- `derive_helpers_in_scope` — for derive helper attribute resolution

Each `ModuleData` (`nameres.rs` ~line 367) has:
- `scope: ItemScope` — the per-module name table
- `children: FxIndexMap<Name, ModuleId>` — child modules
- `parent: Option<ModuleId>`

`ItemScope` (`item_scope.rs`) stores names in three separate
`FxIndexMap`s: `types`, `values`, `macros` (one per namespace).
Plus `legacy_macros` for textual `macro_rules!` scoping.

**Priority rule**: non-glob replaces glob. When a non-glob import
arrives for a name that was previously glob-imported, the glob entry
is replaced. Tracked via `PerNsGlobImports` sets — if a `(module, name)`
is in the glob set and a non-glob arrives, the glob is evicted.
See `push_res_with_import` in `item_scope.rs` ~line 606.

**Watch out**: unlike rustc's two-slot system (which keeps both
non-glob and glob), r-a **replaces** the glob entry. The glob is
gone. This is simpler but means you can't later detect the ambiguity.

## DefCollector Algorithm

`collector.rs`, function `resolution_loop` (~line 415). Three nested
loops driving to a fixed-point:

```
'resolve_attr: loop {
    'resolve_macros: loop {
        'resolve_imports: loop {
            if resolve_imports() == FixedPoint::Yes { break }
        }
        if resolve_macros() == FixedPoint::Yes { break }
    }
    if reseed_with_unresolved_attribute() == FixedPoint::Yes { break }
}
```

**Inner loop** (`resolve_imports`): tries to resolve each pending
import. If the path resolves, the imported names are added to the
module's `ItemScope`. Returns `ReachedFixedPoint::Yes` when no
import made progress.

**Middle loop** (`resolve_macros`): tries to resolve each pending
macro invocation's path. If the path resolves, the macro is expanded
and new items are collected into the `DefMap`. Returns
`ReachedFixedPoint::Yes` when no macro made progress.

**Outer loop** (`reseed_with_unresolved_attribute`): if the
fixed-point stalls with unresolved attribute macros, picks one,
ignores the attribute, and re-collects the item as plain code. This
is a recovery mechanism for unresolved proc-macro attributes.

Limit: `FIXED_POINT_LIMIT = 8192` iterations before giving up.

**For sage**: this is the model to follow. The inner structure
(imports before macros) ensures glob-imported macros are available
when macro paths are resolved — same solution as rustc but explicit.

## Macro Expansion Integration

Unresolved macros are stored as `MacroDirective` entries with three
kinds: `FnLike`, `Attr`, `Derive`.

In `resolve_macros` (~line 1296):
1. For each unresolved macro, try to resolve its path via
   `def_map.resolve_path_fp_with_macro`
2. If the path resolves → expand the macro, collect new items from
   the expansion into the `DefMap` (same as initial collection)
3. If the path doesn't resolve → keep it in `unresolved_macros`
4. Return `ReachedFixedPoint::No` if any macro was resolved (triggers
   another round of import resolution)

**Key**: `resolve_path_fp_with_macro` returns `ReachedFixedPoint::No`
if the path *might* resolve later (e.g., a glob import hasn't been
processed yet). This is r-a's equivalent of rustc's `Undetermined`.

**For sage**: if you expand macros inside a salsa query, the
expansion results are automatically cached. Changing one file only
re-expands macros in affected modules.

## Resolution Order

`resolve_name_in_module` in `path_resolution.rs` (~line 635) defines
the lookup order for a name in a module:

```
1. legacy_macros (textual macro_rules! scope)
2. module scope (ItemScope — includes both direct items and imports)
3. builtin types (shadowed by modules in BuiltinShadowMode::Module)
4. extern prelude
5. macro_use prelude
6. std prelude
```

Combined via `from_legacy_macro.or(from_scope_or_builtin).or_else(extern_prelude).or_else(macro_use_prelude).or_else(prelude)`.

**Watch out**: there is no separate non-glob vs glob step here.
The `ItemScope` already has the final winner (non-glob replaced glob
during collection). This is simpler than rustc but means the
priority was baked in during `DefCollector`, not during lookup.

**For sage**: sage's current `resolve_name` has a similar structure
(items → named imports → globs → extern → std prelude). The main
addition needed is a macro expansion step.

## Body-Level Resolution

`Resolver` struct in `resolver.rs` (~line 47). Has a stack of
`Scope` variants:

- `BlockScope(ModuleItemMap)` — items from a block's `DefMap`
- `GenericParams { def, params }` — generic type/const params
- `ExprScope(ExprScope)` — local bindings from `ExprScopes`
- `MacroDefScope(MacroDefId)` — macro defs inside bodies

Resolution walks the scope stack innermost-out. Local bindings
(from `ExprScope`) shadow module-level names (from `BlockScope`).

Block scopes get their own `DefMap` (linked to the parent via
`BlockInfo`), which means items declared inside a block are resolved
through the same `DefMap` machinery as top-level items.

**For sage**: sage's `BodyResolver` with its scope stack is similar.
The main difference is r-a's block `DefMap`s — sage doesn't need
these unless it wants to support items declared inside function
bodies.

## macro_rules! Scoping

`legacy_macros: FxHashMap<Name, SmallVec<[MacroId; 2]>>` in
`ItemScope`. Separate from the normal `macros` map.

Rules:
- `macro_rules!` definitions are added to `legacy_macros` during
  collection, in textual order
- Unqualified macro calls (`foo!()`) check `legacy_macros` first
- Qualified macro calls (`crate::foo!()`) skip `legacy_macros`
- `legacy_macros` cannot be glob-imported
- `#[macro_use]` on `extern crate` adds to `macro_use_prelude`
  (separate from `legacy_macros`)

**For sage**: if sage only handles modularized macros (not
`macro_rules!`), the `legacy_macros` system is unnecessary.

## Key Differences from rustc

| Aspect | rustc | rust-analyzer |
|---|---|---|
| Phases | Two (early + late) | One (DefCollector does everything) |
| Fixed-point | Implicit via expansion loop | Explicit nested loops with `ReachedFixedPoint` |
| Glob vs non-glob | Two slots, both kept | Replacement — non-glob evicts glob |
| Ambiguity detection | `maybe_push_ambiguity` (early only) | Not done — non-glob silently wins |
| Block scopes | Flat | Separate linked `DefMap` per block |
| Salsa integration | None (mutable `Resolver`) | `crate_def_map` is a salsa query |
| Determinacy | `Determined`/`Undetermined` enum | `ReachedFixedPoint::Yes`/`No` |

**For sage**: r-a's approach is closer to what sage needs. The
single-phase model with explicit fixed-point is more natural for
salsa. The replacement-based priority (non-glob evicts glob) is
simpler than rustc's two-slot system and sufficient for correctness.

## Design Rules Summary

1. **Build the full DefMap before body resolution.** The DefMap query
   does the fixed-point internally; downstream queries see frozen
   module contents.
2. **Imports before macros.** The inner loop resolves all imports to
   a fixed-point before trying macros. This ensures glob-imported
   macros are available.
3. **Non-glob replaces glob.** No two-slot system needed. When a
   non-glob arrives, evict the glob.
4. **`ReachedFixedPoint::No` = "try again."** If path resolution
   can't resolve yet, signal that the fixed-point hasn't been
   reached. The loop will retry after more imports/macros resolve.
5. **Block scopes are separate DefMaps.** If you need items inside
   blocks, each block gets its own DefMap linked to the parent.
6. **Unresolved attribute macros → fallback.** If a proc-macro
   attribute can't be resolved, treat the item as plain code. Good
   UX for incomplete macro support.

## Key Source Files

| File | What to look for |
|---|---|
| `nameres.rs` | `DefMap`, `ModuleData`, `crate_def_map` salsa query |
| `nameres/collector.rs` | `DefCollector`, `resolution_loop`, `resolve_imports`, `resolve_macros`, `MacroDirective` |
| `nameres/path_resolution.rs` | `resolve_path_fp_with_macro`, `resolve_name_in_module` (resolution order), `ReachedFixedPoint` |
| `item_scope.rs` | `ItemScope`, `push_res_with_import` (priority logic), `legacy_macros`, `PerNsGlobImports` |
| `resolver.rs` | `Resolver`, `Scope` enum, body-level resolution |
| `body/` | Body lowering, `ExprScopes` for local bindings |
| `hir-expand/` | Macro expansion infrastructure, `MacroCallId`, `MacroDefId` |
