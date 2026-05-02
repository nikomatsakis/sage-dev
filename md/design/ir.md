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

## Module resolution (`resolve.rs`)

- `module_items(module)` — items declared in a module (salsa tracked)
- `module_use_imports(module)` — flattened use imports (salsa tracked)
- `definition(module, name)` — find a child by name (salsa tracked)
- `resolve_name(module, source_root, crate_root, name, ns)` — full
  name resolution: items → named imports → globs → extern → std prelude
- `resolve_first_segment` / `symbol_to_module` — helpers for multi-segment paths

## Display (`display.rs`)

Item types use `impl Display` with `salsa::with_attached_database`.
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
