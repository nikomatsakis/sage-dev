# Parsing and Stable Symbol Creation

Parsing is the first semantic phase. It turns one source-backed module into an
ordered set of local item identities, each paired with self-contained concrete
syntax and source provenance. It preserves syntax for later demand-driven
queries; it does not resolve names or check types.

## Contract

### Granularity

The demand boundary is one source-backed local module. Inline modules are
parsed while their parent item is lowered and receive their own specified
unexpanded-item result. Generated macro text reuses the same parsing machinery
with a generated `ParseSource`.

### Input

Parsing consumes a parse-source identity, its Rust text, the containing
`ScopeSymbol`, and the crate edition available through that scope.

### Output

The output is an ordered `Vec<LocalModItemSym>`. A represented item has:

- a stable kind-specific local symbol;
- a per-item, stash-owned CST containing the syntax later phases need;
- an absolute item span and relative spans inside the CST; and
- its owning module/crate scope.

Unrecognized or malformed top-level syntax produces an explicit
`LocalModItemSym::Error` carrying its span. Parsing does not silently turn such
input into a valid definition.

### Guarantees

Downstream phases may use item symbols as definition identities and may read
their CST without retaining tree-sitter nodes. Source order is deterministic.
Names and paths remain syntax: the phase does not guarantee that they resolve,
that signatures are well-formed, or that bodies type-check.

<a id="par-a1"></a>
> **PAR-A1 — Parsing publishes syntax-backed identities, not semantic
> conclusions.** A represented item has stable local identity, self-contained
> CST, ordering, ownership, and provenance. Resolution and checking remain
> downstream operations, and malformed or unsupported top-level syntax is
> represented explicitly rather than accepted as a definition. Tree-sitter is
> temporary parser machinery under
> [D3](../decisions.md#d3-tree-sitter-for-parsing), not the published identity
> or syntax representation.
>
> **Required verification:** Source and generated fixtures preserve item order,
> ownership, CST, and provenance; malformed or unsupported top-level input
> produces an error item; and a parse-only query trace contains no resolution,
> signature, body, or solver work.

## Entry points

`unexpanded_items(db, module)` is the tracked phase boundary shown in the
[module-expansion chapter](./module-expansion.md#entry-points). For a
file-backed module it invokes the parser core:

```{anchor}
architecture_parse_module_entry
```

## Construction

Tree-sitter produces a temporary concrete syntax tree. `Parser` walks the
top-level children in order, associates pending attributes with the next item,
and dispatches by item kind:

```{anchor}
architecture_parse_item_dispatch
```

Each item parser copies the required syntax into a fresh `Stash`, creates the
kind-specific tracked symbol, and discards the tree-sitter node after the
query. Nested type, path, generic, expression, and pattern CST nodes are stash
pointers with relative spans.

Stable identity and syntax content are intentionally separate. A local symbol
contains identity fields such as name and owner while its CST and absolute
span are tracked details. See [Symbols and Semantic
Identity](../infrastructure/symbols.md).

<a id="par-a2"></a>
> **PAR-A2 — Item identity is separate from tracked syntax detail.** Rebuilding
> a module may update an item's CST or absolute location without minting a new
> symbol when its semantic identity is unchanged. Every represented definition
> owns a separate CST stash so an equal sibling can be backdated independently.
> This applies [D5](../decisions.md#d5-symbols-form-the-uniform-semantic-identity-family)
> and [D2](../decisions.md#d2-stash-for-type-level-interning) at the parse
> boundary.
>
> **Required verification:** Persistent edit tests move an item, edit one body,
> and insert or reorder siblings while checking symbol identity, per-item CST
> fingerprints, and downstream reexecution separately for affected and
> unaffected items, including generated items.

## Failure and terminal incompleteness

Invalid Rust, an unsupported top-level item kind, an unsupported form inside a
represented item, or a parser recovery node may produce an error item or an
error CST node. This is terminal for that parse result; another scheduling
turn does not supply missing syntax.

A recovered item can still provide a stable identity and usable substructure,
but consumers must not infer that the module contains no other definitions
when an error item remains. Macro expansion and semantic consumers are
responsible for carrying that incompleteness conservatively.

<a id="par-a3"></a>
> **PAR-A3 — Parse recovery never makes absence conclusive.** An error item or
> error CST node is terminal for the current parse, but any consumer asking an
> exhaustive question must treat the surrounding represented set as
> incomplete unless it can prove the error irrelevant to that question. This
> is the parsing consequence of
> [D16](../decisions.md#d16-incompleteness-is-an-explicit-terminal-outcome).
>
> **Required verification:** Malformed and unsupported-item fixtures retain
> safe sibling symbols while downstream exhaustive queries refuse false
> negative conclusions attributable to the missing construct.

## Incremental dependencies

The tracked key is the local module. Parsing reads its body source, source
text, scope, and edition. It must not read resolved names, checked signatures,
other item bodies, or trait results.

Per-item CST stashes and relative spans are designed so that an offset-only
edit before an otherwise unchanged item does not change its semantic CST
content. Stable item identity and Salsa backdating can then prevent that edit
from propagating through unrelated semantic queries. Structural sibling-edit
stability is a destination guarantee whose current evidence is limited.

<a id="par-a4"></a>
> **PAR-A4 — The parse query depends only on parse inputs.** Parsing one module
> may read its source, scope, edition, and generated parse source, but never
> resolved names, checked interfaces, bodies of other items, or trait results.
>
> **Required verification:** A cold parse trace names only those inputs, an
> unchanged warm request executes no parse work, and edits to semantic facts
> that do not change parse inputs leave the parse result reusable.

## Worked example

For:

```rust
struct Pair<T> {
    first: T,
    second: T,
}
```

the phase creates one `LocalStructSym` and stores a `StructCstData` root whose
generic and field slices belong to the same per-item stash:

```{anchor}
example_struct_cst
```

Both field types are still path syntax spelled `T`; the semantic
`GenericParam` is created later by signature checking. Continue with the
[struct-signature walkthrough](../examples/struct-signature.md).

## Code map

| Path | Responsibility |
|---|---|
| `parse/mod.rs` | parser context, tree-sitter entry, and top-level dispatch |
| `parse/items.rs` | item CST construction and local symbol creation |
| `parse/types.rs`, `paths.rs`, `generics.rs`, `exprs.rs` | nested syntax lowering |
| `cst/` | stash-owned, tree-sitter-independent CST representation |
| `local_syms/` | kind-specific tracked identities created by parsing |
| `span.rs` | source/generated identity and absolute/relative provenance |

## Current status

### Current frontier

The parser represents the item, signature, expression, pattern, generic, and
attribute forms needed by the completed mini-redis slices and the existing
unit suite. Source-written and generated text enter the same symbol/CST model.

### Implemented capabilities and evidence

- **[PAR-A1](#par-a1), [PAR-A2](#par-a2) — Per-item CST and later
  parameter identity.** The [struct-signature
  walkthrough](../examples/struct-signature.md) shows the parsed stash and the
  separate checked signature.
- **[PAR-A1](#par-a1) — File-module scope.**
  `items_in_a_file_backed_module_use_that_module_as_their_scope` verifies that
  a child file's item is owned by the child module rather than the crate root.
- **[PAR-A1](#par-a1) — Generated-source parsing.** The [macro-generated struct
  walkthrough](../examples/macro-generated-struct.md) follows macro output
  parsed into an ordinary struct symbol.
- **[PAR-A2](#par-a2) — Offset-insensitive generated identity.**
  `moving_source_item_preserves_derive_expansion_identity` verifies stable
  generated identity when source text moves.
- **[PAR-A2](#par-a2) — Per-kind CST/detail firewall.**
  `detail_edits_preserve_local_symbol_query_keys` applies persistent edits to
  every top-level local item shape and proves through a Salsa observer query
  that CST detail does not replace the symbol. The companion
  `enum_detail_edits_preserve_variant_query_keys` covers variants and their
  constructors, including consumption of the revised variant CST after the
  edit. `moving_associated_items_preserves_symbol_query_keys` covers all three
  associated item kinds through both the trait and impl parsing paths while
  inserting, removing, and reordering siblings.

Run the focused ownership test with:

```bash
cargo test -p sage-test-harness items_in_a_file_backed_module_use_that_module_as_their_scope
```

### Current limitations

- Rust grammar coverage is incomplete; unsupported syntax can become explicit
  error nodes even when rustc accepts it.
- Tree-sitter recovery is not yet exposed as a structured phase-completeness
  result or a complete diagnostic set; PAR-A3 is therefore not established as
  one unified phase contract.
- Local symbol stability has a per-kind detail-edit matrix and associated-item
  insertion, deletion, and reordering coverage, but those structural edits and
  genuine-identity edits are not yet covered for every top-level item kind, so
  the full PAR-A2 matrix remains prospective.
- There is not yet a parse-only cold/warm query trace establishing PAR-A4's
  complete positive and negative dependency set.
- Signature parsing retains lifetime syntax, but later checking currently
  lowers every lifetime to `Lifetime::Dummy`.

### Related roadmap slices

[Semantic inspector and persistent edit
testing](../../implementation/roadmap.md#planned-slice-semantic-inspector-and-persistent-edit-testing)
will expose parsed symbols and edit-invalidation evidence. Future mini-redis
application slices extend grammar coverage only as required by an accepted
vertical target.
