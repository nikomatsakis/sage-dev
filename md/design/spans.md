# Spans (`span.rs`)

Spans use a two-level model for incremental reuse:

- **`AbsoluteSpan { source: ParseSource, start: u32, end: u32 }`** — a byte
  range within one parseable source, together with its identity. `ParseSource`
  distinguishes a real `SourceFile`, bang-macro output, and derive-generated
  output. Items store absolute spans.
- **`RelativeSpan { start: u32, end: u32 }`** — a byte range relative to the
  containing item's `AbsoluteSpan.start`. Signature CST, body CST, and typed
  body nodes store relative spans.

Generated text has its own source identity. A `DeriveExpansion`, for example,
is interned by the stable source item, the derive occurrence, and the resolved
macro definition. Its text and current origin span are tracked data rather
than identity fields. A parsed generated impl's relative coordinates therefore
refer to its generated text without reminting its identity when the source item
merely moves.

## Incremental reuse

When a user adds an import at the top of a file, all following items shift in
absolute position, but relative offsets within each function body stay the
same. Salsa can therefore reuse semantic work whose inputs do not include the
changed absolute coordinate.

## Parsing

The parser tracks an `item_start: u32` while constructing each item's CST. The
item start is `min(first_attr_start, item_node_start)` so attribute spans remain
non-negative after subtraction.

- **Item-level:** `absolute_span(source, node, item_start)` produces an
  `AbsoluteSpan` with parse-source identity and raw byte offsets.
- **Within-item:** parser helpers produce `RelativeSpan` values with byte
  offsets minus `item_start`.

## Resolution

`AbsoluteSpan::resolve(relative)` converts an item-relative span back to a
coordinate in the same parse source:

```rust,ignore
impl AbsoluteSpan<'_> {
    pub fn resolve(&self, relative: RelativeSpan) -> AbsoluteSpan {
        AbsoluteSpan {
            source: self.source,
            start: self.start + relative.start,
            end: self.start + relative.end,
        }
    }
}
```

Real source files support rich source-snippet diagnostics and source-position
queries. Generated sources retain provenance but currently use short
diagnostics; tooling may follow their origin link in the future.

## Where each type is used

| Layer | Span type | Examples |
|-------|-----------|----------|
| Item-level | `AbsoluteSpan` | `LocalFnSym::span`, `LocalStructSym::span`, `LocalModItemSym::Error(…)` |
| Signature CST | `RelativeSpan` | `TypeCst`, `ParamCst`, `AttrCst` |
| Body CST | `RelativeSpan` | `ExprCst`, `StmtCst`, `PatCst` |
| Typed body | `RelativeSpan` | `TyExpr`, `TyStmt`, `TyPat`, `TyBodyData` |
