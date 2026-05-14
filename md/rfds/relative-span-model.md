# RFD: Relative Span Model

![Status: Implemented](https://img.shields.io/badge/status-implemented-brightgreen)

## Goal

Replace `SpanIndices` / `SpanTable` with a two-level span model (`AbsoluteSpan` / `RelativeSpan`) that gives better incremental reuse and is simpler to work with.

## Problem

The current span infrastructure has two issues:

1. **`SpanIndices` stores raw byte offsets** despite the doc comment saying "Indices into a SpanTable's byte_offsets vec." The `SpanTable` per item holds a `byte_offsets` vec but it's only populated with `[item_start, item_end]` (2 elements). The indirection doesn't exist in practice.

2. **No incremental reuse for body spans.** Since body node spans are raw byte offsets, editing whitespace *before* a function (e.g., adding an import) changes every span inside that function's body, invalidating the cached body even though nothing inside the function changed.

## Design

### `AbsoluteSpan` and `RelativeSpan`

```rust
/// Byte offset range within the file. Stored on items.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub struct AbsoluteSpan {
    pub start: u32,
    pub end: u32,
}

/// Byte offset range relative to the containing item's start.
/// Stored on body nodes (expressions, statements, patterns).
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub struct RelativeSpan {
    pub start: u32,
    pub end: u32,
}

impl AbsoluteSpan {
    /// Resolve a body-relative span to an absolute file position.
    pub fn resolve(&self, relative: RelativeSpan) -> AbsoluteSpan {
        AbsoluteSpan {
            start: self.start + relative.start,
            end: self.start + relative.end,
        }
    }
}
```

### Where each type is used

**Items** (`FnAst`, `StructAst`, `ImplAst`, etc.) carry:
- `span: AbsoluteSpan` — file-global byte offsets (same values as today, just renamed)
- `source_file: SourceFile` — file identity (replaces `span_table: SpanTable`)

**Everything inside an item** uses `RelativeSpan` — byte offsets relative to the containing item's `AbsoluteSpan.start`. This includes:

- **Signature types** (`Path`, `TypeRef`, `Param`, `FieldDef`, `VariantDef`, `UseImport`, `Attr`, `TokenTree` in `types.rs`)
- **Body nodes** (`Body`, `Expr`, `Stmt`, `Pat`, `MatchArm`, `FieldInit`, `FieldPat`, `ClosureParam` in `body.rs`)
- **Resolved body nodes** (`RBody`, `LocalVar`, `RExpr`, `RStmt`, `RPat`, `RFieldInit`, `RFieldPat`, `RMatchArm`, `RClosureParam` in `resolved.rs`)

Signatures are part of the item too, so their spans are relative to the item start just like body spans. This avoids needing separate span types for `Path`/`TypeRef` depending on whether they appear in a signature or a body.

**Special case:** `ItemAst::Error(SpanIndices)` becomes `ItemAst::Error(AbsoluteSpan, SourceFile)` so it carries both fields like all other variants.

### Incremental reuse story

When a user adds an import at the top of a file, all items shift in absolute position. But the *relative* offsets within each function body stay the same. Salsa compares the body's `Stashed<...>` bundle (which contains `RelativeSpan`s) and finds it unchanged → cached `resolve_body` is reused.

The item's `AbsoluteSpan` does change, but `AbsoluteSpan` is only read by position-lookup code (hover), not by semantic queries (name resolution, body resolution). So the semantic cache is preserved.

### `SpanTable` is removed

Its two roles are taken over by simpler mechanisms:
- **File identity:** carried directly on items as `source_file: SourceFile`. (When macro expansion as a tracked query lands, this becomes `input_bytes: InputBytes<'db>`.)
- **Byte offset storage:** replaced by `AbsoluteSpan` (item-level) + `RelativeSpan` (everything inside an item: signatures, bodies, resolved bodies). No indirection table needed.

### Lowering changes

- `LowerCtx` produces two kinds of span depending on context:
  - **Item-level:** `AbsoluteSpan` — used for the item's own `span` field.
  - **Within-item:** `RelativeSpan { start: node.start_byte() as u32 - item_start, end: node.end_byte() as u32 - item_start }` — used for signature types (`Path`, `TypeRef`, `Param`, etc.) and attributes. `LowerCtx` gains the item's start byte when lowering within-item content.
- **Item start includes attributes.** Attributes are lowered *before* the item node is reached. The item's `AbsoluteSpan.start` is `min(first_attr_start, item_node_start)`. This way, attribute spans (which precede the item node) are still non-negative after subtracting `item_start`. Concretely: when `lower_items()` encounters the first attribute for an item, it records that byte offset; if there are no attributes, `item_start` is just the item node's start byte.
- `BodyLowerCtx` gains the same `item_start` (passed in when constructed). Its `span(node)` → `RelativeSpan` (same subtraction).
- Each `ItemAst` variant gets `source_file: SourceFile` (replacing `span_table: SpanTable`). `MacroDefAst` and `MacroInvocationAst` also get this field (they currently lack `span_table`).

### `ItemAst` helpers

```rust
impl ItemAst<'_> {
    pub fn absolute_span(&self, db: &dyn Db) -> AbsoluteSpan { ... }
    pub fn source_file(&self, db: &dyn Db) -> SourceFile { ... }
}
```

Simple match over all variants. All variants carry both fields after the migration.

## Migration

### Step 0: Remove `SpanTable`

Replace `span_table: SpanTable` with `source_file: SourceFile` on all `ItemAst` variants. Add `source_file` to `MacroDefAst` and `MacroInvocationAst` (which currently lack `span_table`). Update lowering to pass `self.file` directly instead of constructing a `SpanTable`. Update all callers that read `span_table` to read `source_file` instead. Delete `SpanTable`.

This is a standalone refactor — no span representation changes, no semantic changes. It removes the unused indirection (`byte_offsets` vec) and replaces it with the `SourceFile` that was already inside `SpanTable`.

Notable callers: `derive.rs` reads `span_table.file(db)` to get source text for derive expansion — replace with `source_file`. `derive/builtins.rs` creates synthetic `SpanTable`s for generated code — replace with synthetic `SourceFile`s (which the code already creates).

### Step 1: Introduce `AbsoluteSpan` and `RelativeSpan`

Add both types in `crates/sage-ir/src/span.rs`. Add `AbsoluteSpan::resolve(RelativeSpan) -> AbsoluteSpan`.

### Step 2: Migrate item-level spans

Rename `span: SpanIndices` → `span: AbsoluteSpan` on all `ItemAst` variants. Change `ItemAst::Error(SpanIndices)` → `ItemAst::Error(AbsoluteSpan, SourceFile)`. Pure rename — same values, new type.

### Step 3: Migrate within-item spans

Change `span: SpanIndices` → `span: RelativeSpan` on:
- **Signature types** in `types.rs`: `Path`, `TypeRef`, `Param`, `FieldDef`, `VariantDef`, `UseImport`, `Attr`, `TokenTree`.
- **Body nodes** in `body.rs`: `Body`, `Expr`, `Stmt`, `Pat`, `MatchArm`, `FieldInit`, `FieldPat`, `ClosureParam`.
- **Resolved body nodes** in `resolved.rs`: `RBody`, `LocalVar`, `RExpr`, `RStmt`, `RPat`, `RFieldInit`, `RFieldPat`, `RMatchArm`, `RClosureParam`.

### Step 4: Update lowering

`LowerCtx` item-level span → `AbsoluteSpan`. `LowerCtx` within-item span (signature types) and `BodyLowerCtx` → `RelativeSpan` (subtract item start from node byte offsets). Both contexts gain the item's start byte.

### Step 5: Delete `SpanIndices`

All uses are now `AbsoluteSpan` or `RelativeSpan`. Remove the old type.

### Step 6: Add `ItemAst` helpers

Add `ItemAst::absolute_span(db)` and `ItemAst::source_file(db)` helper methods.

## Future: `InputBytes`

When macro expansion as a tracked query lands, `source_file: SourceFile` on items becomes `input_bytes: InputBytes<'db>`. That's a mechanical replacement — the span model is unaffected.

## Dependents

- **RFD: Resolve at position** — layer 2 uses `AbsoluteSpan` + `SourceFile` for item containment checks; layer 3 resolves `RelativeSpan` via the function's `AbsoluteSpan`.
