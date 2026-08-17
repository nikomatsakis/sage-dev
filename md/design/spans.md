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
[D17](./decisions.md#d17-nested-spans-are-relative-to-stable-item-provenance).

<a id="span-a1"></a>
> **SPAN-A1 — Source provenance is absolute; nested placement is relative.** A
> source-root item owns an `AbsoluteSpan` in one `ParseSource`. A nested
> definition is placed relative to its immediate represented owner, and its
> signature CST, body CST, and typed-body nodes use offsets relative to that
> definition's own start. Resolving a nested node composes every intervening
> relative range with the source-root absolute range.
>
> **Required verification:** Parser and elaboration fixtures cover attributes,
> signatures, bodies, associated items, zero-width/error nodes, and generated
> text; every nested span resolves to the expected source bytes before and
> after movement of the containing item or an earlier sibling.

Generated text has its own source identity. A `DeriveExpansion`, for example,
is interned by the stable source item, the derive occurrence, and the resolved
macro definition. Its text and current origin span are tracked data rather
than identity fields. A parsed generated impl's relative coordinates therefore
refer to its generated text without reminting its identity when the source item
merely moves.

<a id="span-a2"></a>
> **SPAN-A2 — Generated provenance has stable occurrence identity.** Each
> generated parse source is identified by its stable source input, generating
> operation and occurrence, while generated text and current origin
> coordinates are mutable facts. Moving the origin must not collapse distinct
> occurrences or mint a replacement source identity.
>
> **Required verification:** Repeated, duplicate-occurrence, moved-origin, and
> nested-expansion tests compare generated identities, text, origin links, and
> resolved coordinates across revisions.

## Incremental reuse

When a user adds an import at the top of a file, all following items shift in
absolute position, but relative offsets within each function body stay the
same. Salsa can therefore reuse semantic work whose inputs do not include the
changed absolute coordinate.

<a id="span-a3"></a>
> **SPAN-A3 — Offset-only movement does not change semantic item content.** A
> change outside an item which only moves its absolute range may update
> diagnostics and position lookup, but it leaves the item's relative CST and
> typed content equal and must not propagate into semantic consumers which did
> not read the absolute coordinate.
>
> **Required verification:** Edit-invalidation tests insert and remove text
> before otherwise unchanged plain and generated items, then separately assert
> updated resolved positions, stable semantic values, and reuse of downstream
> queries which observe only relative content.

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

## Current status

### Current frontier and evidence

Real source files, bang-macro output, and derive output carry distinct
`ParseSource` identities. Source-root CST and Typed IR use relative spans
within an item; diagnostics resolve them through the current absolute item
span.
`moving_source_item_preserves_derive_expansion_identity` verifies that moving
a derived item updates its origin coordinates without reminting the expansion,
and `duplicate_derive_occurrences_have_distinct_generated_source_identity`
verifies occurrence identity. This establishes the exercised generated-source
portion of [SPAN-A2](#span-a2) and supplies one movement case for
[SPAN-A3](#span-a3).

### Current limitations

- **Known deviation [KD-1](../implementation/known-deviations.md#kd-1-associated-item-cst-spans-use-the-owner-as-their-base):**
  associated-item CST currently remains relative to the enclosing trait or
  impl and uses `cst_base` to resolve diagnostics, rather than establishing an
  item-local base as SPAN-A1 requires.
- Generated-source diagnostics have less source-snippet support than real
  files, and tooling does not yet expose a provenance backtrace.
- Identity/edit evidence covers selected generated moves rather than every
  local item and nested expansion shape required by SPAN-A2 and SPAN-A3.
- The exhaustive parsing and resolution matrix required by SPAN-A1 is not yet
  collected as a chapter-level review packet.
- Lifetime `Dummy` is a semantic type representation and is unrelated to span
  provenance; meaningful lifetime tracking remains deferred.

### Related roadmap slices

The [Semantic Inspector and persistent edit-testing
slice](../implementation/roadmap.md#planned-slice-semantic-inspector-and-persistent-edit-testing)
will expose source/expansion provenance and identity changes across edits.
