# Spans (`span.rs`)

Spans use a two-level model for incremental reuse:

- **`AbsoluteSpan { file: SourceFile, start: u32, end: u32 }`** —
  byte offset range within a file, together with the file identity.
  Stored on items. Editing whitespace before a function changes
  its `AbsoluteSpan` but semantic queries don't read it, so the
  incremental cache is preserved.

- **`RelativeSpan { start: u32, end: u32 }`** — byte offset range
  relative to the containing item's `AbsoluteSpan.start`. Stored
  on everything inside an item: signature types (`Path`, `TypeRef`,
  `Param`, `FieldDef`, `VariantDef`, `UseImport`, `Attr`,
  `TokenTree`), body nodes (`Expr`, `Stmt`, `Pat`, etc.), and
  resolved body nodes (`RExpr`, `RStmt`, `RPat`, etc.).

## Incremental reuse

When a user adds an import at the top of a file, all items shift
in absolute position, but relative offsets within each function
body stay the same. Salsa compares the body's `Stashed<…>` bundle
(which contains `RelativeSpan`s) and finds it unchanged — cached
`resolve_body` is reused.

## Lowering

`LowerCtx` tracks an `item_start: u32` that is set before lowering
each item. The item start is `min(first_attr_start, item_node_start)`
so that attribute spans (which precede the item node) are still
non-negative after subtraction.

- **Item-level:** `abs_span(node)` → `AbsoluteSpan` with file +
  raw byte offsets.
- **Within-item:** `rel_span(node)` → `RelativeSpan` with byte
  offsets minus `item_start`.

`BodyLowerCtx` receives the same `item_start` and produces
`RelativeSpan` for all body nodes.

## Resolution

`AbsoluteSpan::resolve(relative)` converts a body-relative span
back to an absolute file position (for hover/diagnostics):

```rust
impl AbsoluteSpan {
    pub fn resolve(&self, relative: RelativeSpan) -> AbsoluteSpan {
        AbsoluteSpan {
            file: self.file,
            start: self.start + relative.start,
            end: self.start + relative.end,
        }
    }
}
```

## Where each type is used

| Layer | Span type | Examples |
|-------|-----------|----------|
| Item-level | `AbsoluteSpan` | `FnAst::span`, `StructAst::span`, `ItemAst::Error(…)` |
| Signature types | `RelativeSpan` | `Path::span`, `TypeRef::span`, `Param::span`, `Attr::span` |
| Body nodes | `RelativeSpan` | `Expr::span`, `Stmt::span`, `Pat::span`, `Body::span` |
| Resolved body nodes | `RelativeSpan` | `RExpr::span`, `RStmt::span`, `RPat::span`, `RBody::span` |
