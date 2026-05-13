---
name: rustc-name-resolution
description: "Summary of rustc's name resolution algorithm. Use when designing sage's name resolution, understanding how rustc handles macro expansion + name resolution interaction, or debugging resolution behavior differences between sage and rustc."
---

# rustc Name Resolution

Source: `compiler/rustc_resolve/src/` and `compiler/rustc_expand/src/expand.rs`.

## Two-Phase Model

rustc resolves names in two phases. If you're designing a
resolution system, this split is fundamental — it determines what
information is available when.

1. **Early resolution** — interleaved with macro expansion. Iterative
   fixed-point. Resolves macro paths, import paths, and builds the
   module graph. See `ResolverExpand` trait in `macros.rs`.
2. **Late resolution** — after all macros expanded, module contents
   frozen. Resolves paths in function bodies, types, expressions,
   patterns. See `late_resolve_crate` in `late.rs`.

**For sage**: the salsa query graph replaces the fixed-point loop.
The key question is what corresponds to "early" vs "late" — i.e.,
what information does macro path resolution need, and what can wait.

## Priority Rules

These rules govern which binding wins when multiple exist. They are
deterministic and order-independent.

**Non-glob always beats glob.** Each `(module, name, namespace)` has
two slots: `non_glob_decl` (higher priority) and `glob_decl` (lower
priority). `best_decl()` returns `non_glob_decl.or(glob_decl)`.
See `NameResolution` in `imports.rs`.

What goes where:
- Source-defined items → `non_glob_decl`
- Macro-expanded items → `non_glob_decl`
- Named (`use foo::bar`) imports → `non_glob_decl`
- Glob (`use foo::*`) imports → `glob_decl`

**Conflict rules** (in `try_plant_decl_into_local_module`, `imports.rs` ~line 427):
- Non-glob + glob for same name: both kept, non-glob wins
- Two non-globs for same name: **hard error** (duplicate definition)
- Two globs for same name: ambiguity (may be deferred)

**For sage**: if you implement a two-slot system, the priority is
simple and deterministic. No need for fixed-point iteration just to
get priority right.

## Scope Chain

`visit_scopes` in `ident.rs` (~line 100) defines the lookup order.
If you're debugging why a name resolves differently, check which
scope it's found in.

**MacroNS:**
```
DeriveHelpers → DeriveHelpersCompat → MacroRules(chain)
→ ModuleNonGlobs → ModuleGlobs
→ [parent module: NonGlobs → Globs → ...]
→ MacroUsePrelude → ExternPrelude → StdLibPrelude → BuiltinAttrs
```

**TypeNS / ValueNS:**
```
ModuleNonGlobs → ModuleGlobs
→ [parent module: NonGlobs → Globs → ...]
→ ExternPrelude (TypeNS only) → StdLibPrelude → BuiltinTypes
```

Key: NonGlobs always before Globs in the same module. Parent modules
reached via `hygienic_lexical_parent`.

**Watch out**: macros have extra scopes (DeriveHelpers, MacroRules)
before module scopes. `macro_rules!` uses textual scoping (a linked
list), not module scoping. See `MacroRulesScope` in `macros.rs`.

## The Expansion Loop

`fully_expand_fragment` in `rustc_expand/src/expand.rs` (~line 493).
If you're designing macro expansion for sage, this is the reference
algorithm.

```
1. Collect all macro invocations, replace with placeholders
2. resolve_imports()
3. Loop:
   - Try to resolve each pending invocation's macro path
   - Ok → expand, integrate items (build_reduced_graph),
     collect new invocations from output
   - Err(Indeterminate) → defer to retry list
   - When work list empty: resolve_imports(), retry deferred
   - No progress → force mode (emit errors)
```

**Key detail**: derive invocations are expanded BEFORE other
invocations collected from the same expansion output.

**Key callback**: `visit_ast_fragment_with_placeholders` (`macros.rs`
~line 198) integrates expanded items into the module graph and
removes the expansion from `module.unexpanded_invocations`.

## Determinacy

The mechanism that prevents premature "not found" answers. If you're
implementing demand-driven resolution, this is the hardest part to
replicate.

`resolve_ident_in_module_non_globs_unadjusted` (`ident.rs`):
1. Non-glob binding exists → **Determined**, return it
2. A single import could still define this name → **Undetermined**
3. `module.unexpanded_invocations` non-empty → **Undetermined**
4. Otherwise → **Determined** (name doesn't exist)

Rule: as long as a module has pending macro expansions, "not found"
is never final. The expansion might produce the name.

**For sage**: salsa doesn't have "undetermined." Instead, structure
queries so that macro expansion completes before name resolution
reads the results. The `module_expanded_items` query should expand
all macros, then downstream queries read from it.

## Import Resolution

`resolve_imports` in `imports.rs` (~line 607). Fixed-point loop that
retries indeterminate imports until no progress.

**Watch out for globs**: a glob import stays undetermined while its
target module has unexpanded macros. See `determined()` in
`lib.rs` ~line 1107:

```rust
fn determined(&self) -> bool {
    // Glob: undetermined if target module has pending expansions
    import.parent_scope.module.unexpanded_invocations.borrow().is_empty()
        && source_decl.determined()
}
```

This means glob-imported macros are resolvable — the fixed-point
retries after `resolve_imports()` populates glob bindings.

**For sage**: if you resolve all imports before expanding macros,
glob-imported macros are available for macro path resolution without
needing a fixed-point.

## First Segment Resolution

If you're implementing multi-segment path resolution:

- `crate` → crate root module
- `self` → current module
- `super` → parent module (chainable: `super::super::`)
- `$crate` → macro's defining crate (hygiene)
- Leading `::` → extern prelude (absolute path)
- Bare identifier → normal scope chain lookup

See `resolve_first_segment` usage in `ident.rs`.

## macro_rules! Scoping

`macro_rules!` uses **textual scoping**, not module scoping. This is
a separate system from normal name resolution.

`MacroRulesScope` (`macros.rs`) forms a linked list:
- `Empty` → root
- `Def(MacroRulesDecl)` → a definition, visible from here to end of
  enclosing module
- `Invocation(LocalExpnId)` → placeholder, patched after expansion
  (path compression)

**Key distinction**: `macro_rules!` cannot be glob-imported. Only
`#[macro_use]` on `extern crate`/`mod` propagates them. Modularized
macros (`pub macro`, `pub(crate) use name`) use normal module
scoping and *can* be glob-imported.

**For sage**: if you only support modularized macros (not
`macro_rules!`), you don't need the textual scope chain at all.

## Ambiguity Detection

`maybe_push_ambiguity` in `ident.rs` (~line 783). After finding the
first match, the scope visitor continues searching to detect
conflicts.

**Watch out**: ambiguity detection is **skipped during late
resolution** (`Stage::Late`). First match wins immediately
(`ident.rs` ~line 488). This means a name can be ambiguous during
macro resolution but silently resolved during body resolution.

Ambiguity kinds relevant to sage:
- `GlobVsExpanded` — macro-expanded item vs glob in same module.
  Has a FIXME: "too conservative and technically unnecessary now."
  The non-glob priority rule makes this deterministic.
- `GlobVsOuter` — glob import vs name in outer scope
- `MoreExpandedVsOuter` — expansion-order-dependent conflict

**For sage**: if you use the non-glob-beats-glob priority rule
consistently (for both macro and body resolution), you don't need
the `GlobVsExpanded` ambiguity — it's deterministic. Consider
reporting it as a warning rather than an error.

## Design Rules Summary

For quick reference when making sage design decisions:

1. **Non-glob beats glob, always.** Macro-expanded items are non-glob.
2. **Two non-globs = hard error.** No priority between them.
3. **Glob-imported macros work** because import resolution runs
   before/between macro expansion attempts.
4. **`macro_rules!` is textual, not modular.** Separate scoping system.
5. **Ambiguity detection is inconsistent** between early and late
   resolution in rustc. Sage can do better by being consistent.
6. **The fixed-point is for ordering, not priority.** Priority
   (non-glob > glob) is deterministic. The fixed-point handles the
   case where you don't yet know *what* names exist.

## Key Source Files

| File | What to look for |
|---|---|
| `ident.rs` | Scope chain (`visit_scopes`), per-module resolution (`resolve_ident_in_module_non_globs/globs`), ambiguity detection (`maybe_push_ambiguity`) |
| `imports.rs` | Import fixed-point (`resolve_imports`), resolution table (`NameResolution`, `try_plant_decl_into_local_module`) |
| `macros.rs` | Expansion ↔ resolution interface (`ResolverExpand`), macro path resolution (`smart_resolve_macro_path`), `MacroRulesScope` |
| `build_reduced_graph.rs` | How items enter the module graph (`define_local`), how expansions are integrated |
| `lib.rs` | `Resolver` struct, `Determinacy`, `Decl`, `determined()` method |
| `late.rs` | Body-level resolution after expansion |
| `expand.rs` (rustc_expand) | The expansion loop (`fully_expand_fragment`) |
