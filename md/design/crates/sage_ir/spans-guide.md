# Spans (`span.rs`)

Spans use a compositional model for incremental reuse:

- **`AbsoluteSpan { source: ParseSource, start: u32, end: u32 }`** — a byte
  range within one parseable source, together with its identity. `ParseSource`
  distinguishes a real `SourceFile`, bang-macro output, and derive-generated
  output. Source-root items store absolute spans.
- **`RelativeSpan { start: u32, end: u32 }`** — a byte range relative to the
  immediate represented owner. Associated items use one to locate themselves
  within a trait or impl; signature CST, body CST, and typed body nodes use one
  relative to their own item. Resolution composes as many relative levels as
  the ownership hierarchy requires.

The anchors in this chapter are the representation consequences of
[Nested spans are relative to stable item provenance](../../../rfds/architectural-foundations/README.md#nested-spans-are-relative-to-stable-item-provenance).

The specification is {spec}`crates.sage_ir.types.spans.relative_composition`.

Review cases: Parser and elaboration fixtures cover attributes, signatures, bodies, associated items, zero-width/error nodes, and generated text; every nested span resolves to the expected source bytes before and after movement of the containing item or an earlier sibling.

Generated text has its own source identity. A `DeriveExpansion`, for example,
is interned by the stable source item, the derive occurrence, and the resolved
macro definition. Its text and current origin span are tracked data rather
than identity fields. A parsed generated impl's relative coordinates therefore
refer to its generated text without reminting its identity when the source item
merely moves.

The specification is {spec}`crates.sage_ir.types.spans.generated_identity`.

Review cases: Repeated, duplicate-occurrence, moved-origin, and nested-expansion tests compare generated identities, text, origin links, and resolved coordinates across revisions.

## Incremental reuse

When a user adds an import at the top of a file, all following items shift in
absolute position, but relative offsets within each function body stay the
same. Salsa can therefore reuse semantic work whose inputs do not include the
changed absolute coordinate.

The specification is {spec}`crates.sage_ir.types.spans.offset_reuse`.

Review cases: Edit-invalidation tests insert and remove text before otherwise unchanged plain and generated items, then separately assert updated resolved positions, stable semantic values, and reuse of downstream queries which observe only relative content.

## Parsing

The parser establishes an `item_start: u32` for each represented item boundary.
The item start is `min(first_attr_start, item_node_start)` so attribute spans
remain non-negative after subtraction. Entering an associated item establishes
a new base rather than continuing to measure its contents from the enclosing
trait or impl.

- **Source-root item:** `absolute_span(source, node, item_start)` produces an
  `AbsoluteSpan` with parse-source identity and raw byte offsets.
- **Nested item:** its placement is a `RelativeSpan` from its immediate owner's
  start.
- **Within any item:** parser helpers produce `RelativeSpan` values with byte
  offsets minus that item's own start.

## Resolution

`AbsoluteSpan::resolve(relative)` performs one composition step. A nested item
is resolved by applying each relative placement from the source root down to
the requested node. An associated method body therefore resolves as:

```text
owner absolute start + method relative start + body-node relative start
```

The primitive one-step operation is:

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
| Source-root item | `AbsoluteSpan` | source-written or generated module items, error items |
| Nested definition placement | `RelativeSpan` | associated function, type, or constant within a trait or impl |
| Signature CST | `RelativeSpan` | `TypeCst`, `ParamCst`, `AttrCst`, relative to its item |
| Body CST | `RelativeSpan` | `ExprCst`, `StmtCst`, `PatCst`, relative to its item |
| Typed body | `RelativeSpan` | `TyExpr`, `TyStmt`, `TyPat`, `TyBodyData`, relative to its item |

## Implementation and test evidence

The generated [anchor pages](../../anchor/crates/sage_ir/types/spans.html) report implementation entries,
annotated tests, explicit TODO/N/A gaps, and saved claim/test reviews. These
associations do not themselves establish that a test ran.

The [earlier coverage snapshot](../../../rfds/implementation-sequencing/coverage-history.md#spans-spanrs)
preserves the former capability descriptions, edit experiments, and limitations
as historical evidence. Current coverage comes from the generated entries;
implementation plans and decision rationale belong in the relevant RFDs.
