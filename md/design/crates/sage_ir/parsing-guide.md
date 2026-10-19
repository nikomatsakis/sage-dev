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
`ScopeSymbol`, the crate edition available through that scope, and tracked
compilation configuration. An unspecified configuration remains unknown, preserving
conservative expansion completeness for standalone semantic clients. The host supplies target cfg values and enabled
Cargo features before local demand. Recognized `cfg` predicates filter items
before creating their identities; unsupported attributes remain explicit
incomplete expansion work. Inline test fixtures use normal library configuration.

File-backed modules resolve from registered source inputs, including explicit
`#[path = "..."]` paths relative to the containing source file and enclosing inline module directories. Inline modules
retain explicit directory overrides. Logical paths
are normalized independently of materialized temporary directories. A missing
module source produces an error item rather than a host panic.

### Output

The output is an ordered `Vec<LocalModItemSym>`. A represented item has:

- a stable kind-specific local symbol;
- a per-item, stash-owned CST containing the syntax later phases need;
- an absolute item span and relative spans inside the CST; and
- its owning module/crate scope.

Unrecognized syntax or a malformed item subtree produces an explicit
`LocalModItemSym::Error` carrying its span. Parsing does not silently turn such
input into a valid definition.

### Guarantees

Downstream phases may use item symbols as definition identities and may read
their CST without retaining tree-sitter nodes. Source order is deterministic.
Names and paths remain syntax: the phase does not guarantee that they resolve,
that signatures are well-formed, or that bodies type-check.

{spec}`crates.sage_ir.modules.parse.syntax_identities`

Verification intent: Source and generated fixtures preserve item order,
ownership, CST, and provenance; malformed or unsupported top-level input
produces an error item; and a parse-only query trace contains no resolution,
signature, body, or solver work.

## Entry points

`unexpanded_items(db, module)` is the tracked phase boundary shown in the
[module-expansion chapter](expansion-guide.md#entry-points). For a
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
Identity](symbols-guide.md).

{spec}`crates.sage_ir.modules.parse.detail_firewall`

Verification intent: Persistent edit tests move an item, edit one body,
and insert or reorder siblings while checking symbol identity, per-item CST
fingerprints, and downstream reexecution separately for affected and
unaffected items, including generated items.

## Failure and terminal incompleteness

Invalid Rust, an unsupported top-level item kind, an unsupported form inside a
represented item, or a parser recovery node may produce an error item or an
error CST node. This is terminal for that parse result; another scheduling
turn does not supply missing syntax.

A recovered item can still provide a stable identity and usable substructure,
but consumers must not infer that the module contains no other definitions
when an error item remains. Macro expansion and semantic consumers are
responsible for carrying that incompleteness conservatively.

{spec}`crates.sage_ir.modules.parse.recovery_completeness`

Verification intent: Malformed and unsupported-item fixtures retain
safe sibling symbols while downstream exhaustive queries refuse false
negative conclusions attributable to the missing construct.

## Incremental dependencies

The tracked key is the local module. Parsing reads its body source, source
text, scope, and edition. It must not read resolved names, checked signatures,
other item bodies, or trait results.

Per-item CST stashes and relative spans are designed so that an offset-only
edit before an otherwise unchanged item does not change its semantic CST
content. Stable item identity and Salsa backdating can then prevent that edit
from propagating through unrelated semantic queries. Structural sibling-edit
stability is a destination guarantee whose current evidence is limited.

{spec}`crates.sage_ir.modules.parse.dependencies`

Verification intent: A cold parse trace names only those inputs, an
unchanged warm request executes no parse work, and edits to semantic facts
that do not change parse inputs leave the parse result reusable.

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
[struct-signature walkthrough](../../examples/struct-signature.md).

## Code map

| Path | Responsibility |
|---|---|
| `parse/mod.rs` | parser context, tree-sitter entry, and top-level dispatch |
| `parse/items.rs` | item CST construction and local symbol creation |
| `parse/types.rs`, `paths.rs`, `generics.rs`, `exprs.rs` | nested syntax lowering |
| `cst/` | stash-owned, tree-sitter-independent CST representation |
| `local_syms/` | kind-specific tracked identities created by parsing |
| `span.rs` | source/generated identity and absolute/relative provenance |

## Implementation and test evidence

The generated [anchor pages](../../anchor/crates/sage_ir/modules/parse.html) report implementation entries,
annotated tests, explicit TODO/N/A gaps, and saved claim/test reviews. These
associations do not themselves establish that a test ran.

The [earlier coverage snapshot](../../../rfds/implementation-sequencing/coverage-history.md#parsing-and-stable-symbol-creation)
preserves the former capability descriptions, edit experiments, and limitations
as historical evidence. Current coverage comes from the generated entries;
implementation plans and decision rationale belong in the relevant RFDs.
