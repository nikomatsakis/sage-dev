# IR

The sage IR lives in `crates/sage-ir/`. It's built on salsa 0.26 and
has three layers: **items**, **syntactic bodies**, and **resolved bodies**.

## Items (`item.rs`)

Top-level declarations. Each kind is a salsa tracked struct; the `Item`
enum wraps them all (it's `Copy` — each variant is a salsa ID).

Tracked fields create incremental firewalls. A query reading `params`
won't re-execute when `body` changes.

Key item types: `FunctionItem`, `StructItem`, `EnumItem`, `TraitItem`,
`ImplItem`, `TypeAliasItem`, `ConstItem`, `StaticItem`, `ModItem`,
`UseGroup`.

## Types (`types.rs`)

`TypeRef` represents types as written in source (unresolved). Salsa
tracked structs, so `&Vec<String>` is just nested salsa IDs — `Copy`.

`Path` holds `Vec<Name>` segments. `Name` is salsa-interned (O(1) equality).

## Syntactic bodies (`body.rs`)

Function bodies live in a **Stash** — a flat byte buffer for `Copy`-only
data with thin handles (`Ptr<T>`, `Slice<T>`). This avoids per-node
salsa overhead.

`FunctionBody = Stashed<Ptr<Body>>`. The `Stashed` wrapper implements
`PartialEq` via byte comparison — if the body didn't change, salsa
skips downstream invalidation.

Key types: `Expr`/`ExprKind`, `Stmt`/`StmtKind`, `Pat`/`PatKind`.
Paths are unresolved `Path` values. Bindings are `Name` values.

Notable variants:
- `IfLet(pat, scrutinee, then, else)` / `WhileLet(pat, scrutinee, body)` —
  preserved as distinct nodes (not desugared to `match`)
- `MacroCall(Path, TokenTree)` — macro path + opaque tokens (no expansion)

## Resolved bodies (`resolved.rs`, `body_resolve.rs`)

The resolved IR mirrors the syntactic body 1:1. Same structure, but
paths become `Res` and bindings become `LocalId`.

### Res — what a path resolved to

```
Res::Def(Symbol)   — module-level or external definition
Res::Local(LocalId) — local variable (param, let, for, closure param)
Res::Err            — couldn't resolve
```

`Symbol` is salsa-interned with `SymbolSource::Local(Item)` or
`SymbolSource::External(CrateNum, DefIndex)`.

### LocalId and scopes

`LocalId(u32)` indexes into `RBody.locals: Slice<LocalVar>`.
The resolver tracks a scope stack (Vec of Vec of (Name, LocalId)).
Scopes are pushed/popped at: blocks, closures, for loops, match arms,
if-let, while-let.

### resolve_body — the entry point

`resolve_body` in `body_resolve.rs` is a `#[salsa::tracked(returns(ref))]`
function. It takes `(db, FunctionItem, Module, SourceRoot, crate_root)`
and returns `&ResolvedBody`.

The `BodyResolver` struct walks the syntactic body, reading from the
source `Stash` and writing to an output `Stash`. For each node:
- Expressions: resolve paths (value/type/macro namespace), recurse
- Statements: resolve init before pattern (let-binding ordering)
- Patterns: introduce bindings into current scope, resolve path patterns

### Path resolution algorithm

Single-segment value paths check locals first (innermost scope outward),
then delegate to `resolve_name` (module items → use imports → globs →
extern prelude → std prelude).

Multi-segment paths use `resolve_first_segment` (handles `crate`/`self`/
`super`/bare) then walk remaining segments via `definition(module, name)`.

### What stays unresolved

- **Method calls** — `receiver.method(args)` keeps the method `Name`
- **Field access** — `expr.field` keeps the field `Name`
- **Enum variants** — `Frame::Bulk` needs type-qualified lookup
- **Associated functions** — `Type::func()` needs impl-block lookup
- **Macro bodies** — tokens inside `MacroCall` are opaque
- **Type references** — `TypeRef` passes through unchanged

## Module resolution (`resolve.rs`, `memmap/`)

Module-level name resolution uses a two-faced **MEM-map** (Minimal
Expanded Members map) per module. A MEM-map holds the entries needed
to answer "what names does this module export?" — items, redirects
(`use foo::bar`), globs (`use foo::*`), and macro invocations.

### Data model

`MemmapEntry` has five variants:

- `Item(Item)` — a declared item (struct, fn, impl, mod, …).
  Namespace derived dynamically via `item_in_namespace`.
- `MacroDef(MacroDefItem)` — a `macro_rules!` definition, always in
  `Namespace::Macro(Bang)`.
- `Redirect { name, target }` — `use foo::bar [as baz]`. Namespace
  resolved dynamically by resolving `target` at lookup time.
- `Glob { path }` — `use foo::*`. Target module resolved dynamically
  at lookup time, so globs whose target is created by macro
  expansion are picked up correctly.
- `MacroUse(MacroUse)` — a macro invocation with resolution state
  (`Unresolved` / `Resolved(Vec<MacroCallee>)` /
  `Expanded(Vec<Expansion>)`).

Each `Expansion` pairs a callee with the entries it produced, so a
fan-out of multiple candidates becomes multiple branches inside a
single `MacroUse`.

### Two resolvers

The MEM-map is consumed by two resolvers with different semantic
guarantees:

- **Construction-time** (`memmap::resolve_path::resolve_macro_path`):
  used while `module_memmap` is building a module's entries to
  resolve macro invocation paths. Never errors; returns `Vec<Symbol>`
  of candidates. Safe to call from inside `module_memmap` — uses
  `definition` / `resolve_first_segment` (file_item_tree-backed) for
  first-segment lookups so it never re-enters the current module's
  memmap query.
- **Post-construction** (`resolve_name`): used by body resolution,
  display, and IDE-style queries. Returns exactly one `Symbol` or
  an error. Flattens the whole tree (entries at any
  `MacroUse::Expanded` depth count equally) and uses
  `definition_via_memmap` for path walking, so names introduced by
  macro expansion inside any module are visible to downstream
  callers.

Priority in `resolve_name`: named (items/redirects/macro defs) →
glob → extern prelude → std prelude.

### Inline modules

`ModuleSource` has three variants:

- `Local { file, parent, declaration }` — workspace module backed by
  a source file.
- `LocalInline { parent, mod_item }` — inline `mod foo { ... }`. Its
  `mod_item.items` tracked field feeds `module_items` and
  `module_memmap` directly.
- `External(CrateNum, DefIndex)` — dependency crate, queried via
  `TcxDb`.

`Module::containing_file` walks up `LocalInline` parents to find the
backing `SourceFile`, if any.

### Cycle handling

Path-walking helpers (`definition_via_memmap`,
`resolve_path_to_symbol`, `resolve_use_path_to_module_from_path`)
share a thread-local in-flight frame stack. Re-entering the same
`(module, name|path, kind)` triple short-circuits to `None`/`Err`,
so mutually cyclic globs and redirect chains terminate without stack
overflow.

### Macro expansion

`memmap::expand::expand_macro` produces `Vec<MemmapEntry>` by
creating a synthetic `SourceFile` for the macro body and calling
`file_item_tree` on it. Expanded items are real tracked structs —
`StructItem`, `FunctionItem`, inline `ModItem` with populated
`items`, etc. — so downstream queries work uniformly for both
source-level and macro-introduced items.

The fixpoint loop in `resolve_and_expand_macros` iterates until
every `MacroUse` converges. `module_memmap` uses salsa's
`cycle_initial = empty memmap` for cross-module cycles.

### Validation

`memmap::validate::memmap_errors` runs after convergence and
reports:

- `UnresolvedMacro { path }`
- `AmbiguousMacro { path }` (multiple candidate callees)
- `UnresolvedRedirect { name }` / `UnresolvedGlob { path }`
- `DuplicateName { name, ns }`
- `TimeTravelViolation { name, ns }` (a macro expansion introduces
  a name that would shadow a glob import)

### Legacy helpers

- `module_items(module)` / `module_use_imports(module)` — file-level
  queries used inside `module_memmap` seeding and by
  construction-time path walkers.
- `definition(module, name)` — items-based direct lookup.
- `resolve_first_segment` / `symbol_to_module` — first-segment
  helpers for construction-time path resolution.

## Display (`display.rs`)

Item types use `impl Display` with `salsa::with_attached_database`.
`FunctionItem::Display` prints the signature and body together.
Body types use a `PrettyPrint` trait (needs `&Stash` to deref handles).

Resolved body display uses `pretty_print_resolved(tcx, resolved)` which
sets a thread-local `TcxDb` reference so `fmt_res` can call `def_path`
for human-readable external symbol names.

Display format:
- `<def Get>` — local item
- `<ext std::prelude::v1::Ok>` — external def (via `TcxDb::def_path`)
- `<local:0>` — local variable reference
- `<bind:3>` — binding introduction in a pattern
- `<unresolved>` — resolution failed

## Spans (`span.rs`)

`SpanIndices { start: u32, end: u32 }` — byte offsets, 8 bytes, `Copy`.
`SpanTable` maps spans back to their `SourceFile`. Semantic queries that
don't need locations never read the span table.
