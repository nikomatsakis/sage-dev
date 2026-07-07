# RFD: Resolve at Position

**Status:** Draft

**Depends on:**
- [Macro expansion as a tracked query](./macro-expansion-tracked-query.md) — `ParseSource`, `MacroInput`, `expand_macro`
- [Relative span model](./relative-span-model.md) — `AbsoluteSpan`, `RelativeSpan`, `ItemAst::absolute_span`/`parse_source`

## Goal

Given a file path and a (line, column) position, resolve the identifier under the cursor to its definition. This is the foundation for go-to-definition, hover, and the integration tests we want to write ("what does `frame` on parse.rs:37 resolve to?").

Today sage can resolve names *by name* (`resolve_name`, `resolve_body`), but there's no way to go from a source location to the resolved symbol at that location. We need to build the bridge from position → node → resolution.

## Motivating examples

From `test-fixtures/mini-redis/src/parse.rs`:

| Line | Col | Token | Expected resolution |
|------|-----|-------|-------------------|
| 37 | 25 | `frame` | `Parse::new` param `frame` → `LocalId(0)` |
| 57 | 9 | `self` | `Parse::next_string` param `self` → `LocalId(0)` |
| 63 | 28 | `s` | pattern binding in `Frame::Simple(s)` → `LocalId(1)` |
| 64 | 28 | `str` | path `str` → `<ext std::str>` (the module, used as `str::from_utf8`) |
| 84 | 36 | `Bytes` | path `Bytes` → `<ext bytes::Bytes>` (via `use bytes::Bytes`) |

These should all be expressible as snapshot tests with dual snapshots (result + query log):

```rust
#[test]
fn hover_parse_rs_frame_param() {
    run_sage_with(mini_redis_dir(), &[], |sage| {
        sage.db.take_query_log();
        let result = sage.hover("parse.rs", 37, 25);
        expect![[r#"{"kind":"local","name":"frame","local_id":0,"function":"new"}"#]]
            .assert_eq(&result.unwrap().to_string());
        let log = sage.db.take_query_log();
        expect![[r#"...only parse.rs and lib.rs queries..."#]]
            .assert_eq(&log);
    });
}
```

## Design: Recursive dispatch

The overall shape is recursive dispatch. The top-level `hover` call receives a file path and position. Each layer narrows the search and delegates to the next. The return type is `serde_json::Value` with nested structure — each layer wraps its child's result with its own context.

```
hover("src/parse.rs", line=37, col=25)
  │
  ▼
SageContext::hover(path, line, col)          ← find crate by directory/filename
  │
  ▼
ModSymbol::hover(db, source_root, file, offset)  ← find item by span
  │
  ▼
FnAst::hover(db, module, source_root, offset)    ← find node in resolved body
  │
  ▼
serde_json::Value                            ← hover annotation
```

Each layer only knows about its own children. The crate doesn't know about body nodes; the module doesn't know about local variables. The recursion naturally limits the work performed at each level.

### Return type

`serde_json::Value` for now. Each layer wraps its child's result with its own context, producing a nested structure:

```json
{
  "module": "parse",
  "item": "impl Parse",
  "hover": {
    "kind": "local",
    "name": "frame",
    "local_id": 0,
    "function": "new"
  }
}
```

This way layer 2 adds `"module"` and `"item"`, layer 3 adds `"kind"`, `"name"`, etc. — no layer needs to receive context from its parent; each enriches the result on the way back up.

Other examples:

```json
{
  "module": "parse",
  "item": "impl Parse",
  "hover": {"kind": "def", "symbol": "<ext bytes::Bytes>", "path": "bytes::Bytes"}
}
```
```json
{
  "module": "parse",
  "item": "impl Parse",
  "hover": {"kind": "field", "name": "key"}
}
```

### Layer 1: `SageContext::hover(path, line, col)`

The entry point. Receives a source-relative path (e.g., `"src/parse.rs"` or just `"parse.rs"`).

1. Find the `SourceFile` by matching against `source_root.files(db)` paths.
2. Convert `(line, col)` to a byte offset by scanning the file text for newlines.
3. Find the `ModSymbol` that owns this file — walk the module tree from the crate root, looking for modules whose `containing_file()` matches.
4. Delegate to `ModSymbol::hover(db, source_root, file, byte_offset)`.

The line/col → byte offset conversion is a pure function, no salsa caching needed:

```rust
fn line_col_to_byte(text: &str, line: u32, col: u32) -> Option<u32>
```

`col` is a byte offset within the line (1-based), matching tree-sitter. UTF-16 code-unit columns (LSP protocol) are a separate conversion at the protocol layer, not here.

### Layer 2: `ModSymbol::hover(db, source_root, file, byte_offset)`

The module knows its expanded items. It walks the **expanded module** (memmap) entries — not the unexpanded `module_items` — so that macro-generated items (derive impls, proc-macro expansions) are visible.

For each `MemmapEntry::Item(item)` and items from `expand_macro` results, check the item's `parse_source(db)` and `absolute_span(db)`. **Filter by `ParseSource` first:** only check items whose `parse_source` is a `ParseSource::SourceFile` matching the target file. Macro-expanded items have `ParseSource::MacroExpansion` and should be skipped — they can't be pointed to by a source position. (In the future, hovering on a macro invocation could trace through the `MacroInput` to show expanded items.)

For nested containers:
- **`impl` / `trait` blocks:** recurse into sub-items to find the narrowest match.
- **Inline `mod foo { ... }` blocks:** recurse by span containment, then dispatch from the inner module's resolution scope.

Once it has an `ItemAst`:
- If the offset lands on the item's *name span* → return an `"item_def"` annotation directly.
- If the item is a `FnAst` → delegate to `FnAst::hover(db, module, source_root, byte_offset)`.
- If the item is an `Impl`/`Trait` containing functions → find the nested `FnAst` and delegate.
- Otherwise (struct fields, type aliases, etc.) → return a basic annotation from the item itself.
- If the offset falls between items (whitespace, comments) → return `None`.

Layer 2 wraps the child's result with module and item context:

```rust
let child_result = fn_ast.hover(db, module, source_root, byte_offset)?;
Some(json!({
    "module": module_name,
    "item": item_description,  // e.g., "impl Parse"
    "hover": child_result,
}))
```

### Layer 3: `FnAst::hover(db, module, source_root, byte_offset)`

The function knows its resolved body. It:

1. Calls `resolve_body(db, self, module, source_root)` to get the `ResolvedBody` (returned by reference via `returns(ref)`).
2. Gets the function's `AbsoluteSpan` for resolving body-relative spans. Walks the stash-allocated resolved body tree, resolving each node's `RelativeSpan` to an `AbsoluteSpan` via `fn_span.resolve(relative)`, and finds the innermost node whose resolved span contains the byte offset (narrowest span wins).
3. Classifies the node and returns a `serde_json::Value`.

Classification:

| Node kind | Annotation |
|-----------|-----------|
| `RExprKind::Path(Res::Def(sym))` | `{"kind": "def", ...}` |
| `RExprKind::Path(Res::Local(id))` | `{"kind": "local", ...}` |
| `RPatKind::Bind(id, _)` | `{"kind": "local", ...}` (definition site) |
| `RExprKind::Field(_, name)` | `{"kind": "field", ...}` |
| `RExprKind::MethodCall(_, name, _)` | `{"kind": "method", ...}` |
| `RExprKind::StructLit(Res::Def(sym), _)` | `{"kind": "def", ...}` (anywhere in the literal) |
| `RExprKind::MacroCall(Res::Def(sym), _)` | `{"kind": "def", ...}` |
| `Res::Err` | `None` |

### Finding the module for a file

Given a `SourceFile`, find its `ModSymbol` so we can dispatch to layer 2 with the right module scope. This requires walking the module tree from the crate root:

```rust
fn find_module_for_file<'db>(
    db: &'db dyn Db,
    root: ModSymbol<'db>,
    source_root: SourceRoot,
    file: SourceFile,
) -> Option<ModSymbol<'db>>
```

Walks the expanded module recursively, looking for `MemmapEntry::Item(ItemAst::Mod(mod_ast))` entries. For each, calls `resolve_mod` to get a resolved `ModAst` with its file set, then checks if `containing_file()` matches. This triggers `resolve_mod_tracked` (a salsa-tracked function) for each module declaration along the path — but these are cached by salsa, so the cost is paid once per session.

## Incremental behavior

This query chain is naturally demand-driven:

- `line_col_to_byte` is pure, no caching
- `expanded_module` is already cached per module
- `expand_macro` is memoized per `(callee, input)` pair
- `resolve_body` is already cached per function
- The position-lookup walk is a cheap tree traversal over cached data

Body nodes use `RelativeSpan` (offsets relative to the containing item's start). Editing a *different* item in the same file doesn't change relative offsets within this item, so salsa can reuse the cached body. Only editing within the item itself (changing its text, shifting its internal offsets) invalidates the body cache.

## Scope and non-goals

**In scope:**
- Resolving identifiers that appear as paths in expressions and patterns
- Resolving local variable references and bindings (params, let bindings)
- Resolving struct literal names, macro call paths
- Items generated by macro expansion (visible via expanded module)
- Returning "unresolved" for things we can't resolve yet (field access, method calls)
- Integration tests using mini-redis positions

**Out of scope (future work):**
- Field access resolution (needs type inference)
- Method resolution (needs type inference + trait solving)
- Type annotations (e.g., `x: Foo` — resolving `Foo`)
- Resolving identifiers in `use` statements (these live at the module level, not in bodies)
- LSP protocol integration (that's a layer above this)

## Testing strategy

Every integration test produces **two snapshots**:

1. **Resolution result** — what the identifier resolved to (symbol, local, field, etc.)
2. **Query log** — the salsa queries that fired, captured via `db.take_query_log()`

The query log snapshot is a regression test for demand-driven behavior. Resolving `frame` on parse.rs:37 should only touch `lib.rs` (module tree), `parse.rs` (item tree + body), and the extern prelude lookups needed for types in scope — it should *not* touch `cmd/get.rs`, `connection.rs`, or any other file. If an unrelated file appears in the log, something is wrong.

```rust
#[test]
fn hover_parse_rs_37_25_frame() {
    run_sage_with(mini_redis_dir(), &[], |sage| {
        sage.db.take_query_log(); // clear setup queries

        let result = sage.hover("parse.rs", 37, 25);
        expect![[r#"{"module":"parse","item":"impl Parse","hover":{"kind":"local","name":"frame","local_id":0,"function":"new"}}"#]]
            .assert_eq(&result.unwrap().to_string());

        let log = sage.db.take_query_log();
        expect![[r#"
            ...only parse.rs and lib.rs queries...
        "#]].assert_eq(&log);
    });
}
```

This dual-snapshot pattern matches the existing tests in `body_resolve_tests.rs` and `expand_tests.rs`.

## Implementation plan

### Step A: `line_col_to_byte` utility

Pure function, add to `crates/sage-ir/src/span.rs`. Unit-testable independently.

### Step B: `find_module_for_file`

Walk expanded module tree from crate root to find the `ModSymbol` owning a given file. Needed so we can dispatch into layer 2.

### Step C: `FnAst::hover` — innermost layer

Walk the resolved body stash tree, resolving each node's `RelativeSpan` via the function's `AbsoluteSpan`, to find the tightest node at a byte offset. Classify it, return `serde_json::Value`. This is the most interesting layer and can be tested in isolation given a `FnAst` + module + offset. Note: `resolve_body` returns `&ResolvedBody` (via `returns(ref)`), so the stash is borrowed.

### Step D: `ModSymbol::hover` — middle layer

Walk expanded module entries for the item containing the byte offset (checking each item's `parse_source` and `absolute_span`). Filter by `ParseSource` kind to skip macro-expanded items. Recurse into `impl`/`trait`/inline-`mod` sub-items. Delegate to `FnAst::hover` for functions. Wrap child result with module/item context.

### Step E: `SageContext::hover` — top layer

Wire together: path → file, line/col → byte offset, file → module, then delegate to `ModSymbol::hover`.

### Step F: Integration tests

Tests against mini-redis parse.rs positions (the motivating examples), each with dual snapshots: resolution result + query log.

## Open questions

1. **Column encoding:** Byte offset within the line is simplest and matches tree-sitter. LSP uses UTF-16 code units. Do we handle both from the start, or add UTF-16 support when we build the LSP layer?

2. **Items outside function bodies:** The recursive dispatch naturally handles "cursor is on a top-level item name" at layer 2. But `use` paths, struct field types, and function signatures are module-level and not covered by `resolve_body`. Layer 2 could return basic annotations for these (the item name and kind) and we add deeper resolution later.

3. **Multiple items at the same position:** Macro expansions can produce overlapping spans. Prefer the original source span over expanded spans.

4. **JSON schema evolution:** `serde_json::Value` is maximally flexible but untyped. Should we define a Rust struct and derive `Serialize`, or stay loose until the shape stabilizes?
