# RFD: Parsing (tree-sitter → CST)

## Summary

Rebuild the `parse/` module: parse Rust source text via tree-sitter and
produce stash-allocated CST nodes plus salsa tracked symbols
(`LocalFnSym`, `LocalStructSym`, etc.). This is the front door of the
pipeline — everything downstream (MEM-map, resolution, checking) already
exists and expects `&[LocalModItemSym]` as input.

## Entry points

```
LocalModSym::unexpanded_items(db) -> &'db [LocalModItemSym]
  ├── ModSource::File(f)   → parse_str_to_cst(db, ParseSource::SourceFile(f), f.text(db), scope)
  └── ModSource::Inline(s) → borrow from stash (already parsed during parent's parse)
```

`unexpanded_items` is `#[salsa::tracked(returns(ref))]`. It is the
memoization boundary — no separate tracked parse query on `SourceFile`
or `MacroExpansion`. Salsa detects changes via the `SourceFile` input
dependency and tracked-struct identity stability.

Macro expansions re-enter through the same helper:

```rust
parse_str_to_cst(db, ParseSource::MacroExpansion(exp), exp.text(db), scope)
```

Called from `memmap/expand.rs` when an expansion produces new items.

## `parse_str_to_cst`

```rust
fn parse_str_to_cst<'db>(
    db: &'db dyn Db,
    source: ParseSource<'db>,
    text: &str,
    scope: ScopeSymbol<'db>,
) -> Vec<LocalModItemSym<'db>> {
    let parser = Parser { db, source, scope, text };
    let tree = tree_sitter_parse(text);
    parser.parse_item_list(tree.root_node())
}
```

This is a plain (non-tracked) helper. It:

1. Constructs a `Parser` with the shared context.
2. Parses `text` with `tree_sitter_rust::LANGUAGE` → `tree_sitter::Tree`.
3. Calls `parser.parse_item_list(root_node)` which iterates children,
   buffers `attribute_item` siblings, and dispatches each item node to
   the appropriate `self.parse_*` method (passing the collected attrs).
4. Each per-item method allocates a fresh `Stash`, parses the CST into
   it, wraps the result in `Stashed<Ptr<*CstData>>`, and mints the
   corresponding tracked struct.
5. Returns the collected `Vec<LocalModItemSym>`.

## tree-sitter node structure

Empirical findings from `tree-sitter-rust 0.24`. These inform the
dispatch logic and attribute handling.

### Attributes are preceding siblings

Outer attributes (`#[...]`) are **sibling** `attribute_item` nodes that
precede the item they annotate. They are NOT children of the item node.

```
source_file [0-53]
  attribute_item [0-6]        ← sibling, not child
  attribute_item [7-18]       ← sibling
  function_item [19-53]       ← the item
```

This holds at every level: inside `declaration_list` (mod/trait/impl
bodies), attributes are also preceding siblings of the item they
annotate.

**Implication:** `parse_item_list` must accumulate `attribute_item`
nodes in a buffer. When it encounters a non-attribute item node, it
drains the buffer and passes those attrs to the per-item parser. The
`AbsoluteSpan` starts at the first buffered attribute's `start_byte()`
(or the item's `start_byte()` if there are no attributes).

### Top-level node kinds → item dispatch

| tree-sitter `node.kind()` | Our item | Notes |
|----------------------------|----------|-------|
| `function_item` | `LocalFnSym` | Has `body: block` |
| `function_signature_item` | `LocalFnSym` | In traits; no body, ends with `;` |
| `struct_item` | `LocalStructSym` | `body: field_declaration_list` or `ordered_field_declaration_list` |
| `enum_item` | `LocalEnumSym` | `body: enum_variant_list` |
| `trait_item` | `LocalTraitSym` | `body: declaration_list` |
| `impl_item` | `LocalImplSym` | `body: declaration_list` |
| `mod_item` | `LocalModSym` | `body: declaration_list` (inline) or `;` (file-backed) |
| `use_declaration` | `LocalUseSym` | `argument:` is `scoped_identifier` / `use_list` / etc. |
| `const_item` | `LocalConstSym` | |
| `static_item` | `LocalStaticSym` | |
| `type_item` | `LocalTypeAliasSym` | |
| `expression_statement` containing `macro_invocation` | `LocalMacroInvocationSym` | Top-level macro call |
| `macro_definition` | `LocalMacroDefSym` | `macro_rules!` |
| `attribute_item` | (buffered) | Attached to following item |

### Inline vs file-backed modules

Both are `mod_item`. Distinguished by child structure:

```
// Inline: has body: declaration_list
mod_item [0-91]
  name: identifier "shapes"
  body: declaration_list [11-91]
    { ... items ... }

// File-backed: ends with ;
mod_item [0-10]
  name: identifier "utils"
  ; [9-10]
```

Check: `node.child_by_field_name("body").is_some()` → inline.

### Function signatures vs function items

In trait bodies, functions without a default body appear as
`function_signature_item` (not `function_item`):

```
function_signature_item [16-40]
  fn [16-18]
  name: identifier "fmt"
  parameters: parameters [22-29]
  -> [30-32]
  return_type: type_identifier "String"
  ; [39-40]
```

Both map to `LocalFnSym`. For `function_signature_item`, the `body`
field is `None` in `FnCstData`.

### Key field names by item kind

| Item kind | Named fields |
|-----------|-------------|
| `function_item` | `name`, `parameters`, `return_type` (optional), `body`, `type_parameters` (optional) |
| `struct_item` | `name`, `body` (field_declaration_list), `type_parameters` |
| `enum_item` | `name`, `body` (enum_variant_list), `type_parameters` |
| `trait_item` | `name`, `body` (declaration_list), `type_parameters` |
| `impl_item` | `type`, `body` (declaration_list), `type_parameters`, `trait` (optional) |
| `mod_item` | `name`, `body` (optional declaration_list) |
| `parameter` | `pattern`, `type` |
| `field_declaration` | `name`, `type` |
| `enum_variant` | `name`, `body` (optional) |
| `type_parameter` | `name` |

### Macro invocations

At the top level, macro invocations appear wrapped in
`expression_statement`:

```
expression_statement [0-35]
  macro_invocation [0-34]
    macro: identifier "define_shape"
    ! [12-13]
    token_tree [13-34]
      ( ... )
  ; [34-35]
```

The inner `macro_invocation` has fields `macro` (the path) and the
`token_tree` body.

### The `Parser` struct

A `Parser` holds the per-item-list context shared across all items:

```rust
struct Parser<'a, 'db> {
    db: &'db dyn crate::Db,
    source: ParseSource<'db>,
    scope: ScopeSymbol<'db>,
    text: &'a str,
}
```

Per-item methods are `&self` (or `&mut self` if we track ordering for
disambiguation). The stash is NOT on the struct — each item mints its
own fresh stash.

`parse_str_to_cst` constructs a `Parser`, runs tree-sitter, and
delegates to `parser.parse_item_list(root_node)`.

### Scope and span threading

Every tracked struct needs:
- `scope: ScopeSymbol` — lives on `self` (the parent module).
- `span: AbsoluteSpan` — computed from the tree-sitter node's byte
  range plus `self.source`.

For inline modules, the parser creates a child `Parser` with a new
scope and recurses.

### Per-item parse pattern

```rust
impl<'a, 'db> Parser<'a, 'db> {
    fn parse_fn(&self, node: Node, pending_attrs: &[Node]) -> LocalModItemSym<'db> {
        let mut stash = Stash::new();

        // The item's absolute start includes leading attributes.
        let item_start = pending_attrs
            .first()
            .map_or(node.start_byte(), |a| a.start_byte()) as u32;

        let name = Name::new(self.db, node_text(node, "name"));
        let attrs = self.parse_attr_nodes(&mut stash, pending_attrs, item_start);
        let generics = self.parse_generics(&mut stash, node, item_start);
        let params = self.parse_params(&mut stash, node, item_start);
        let ret = self.parse_return_type(&mut stash, node, item_start);
        let body = self.parse_body_expr(&mut stash, node, item_start);
        let where_clauses = self.parse_where_clauses(&mut stash, node, item_start);

        let span = RelativeSpan { start: 0, end: node.end_byte() as u32 - item_start };
        let cst_data = FnCstData { attrs, name, generics, params, ret, body, where_clauses, span };
        let root = stash.alloc(cst_data);
        let cst: FnCst = Stashed::new(stash, root);

        let abs_span = AbsoluteSpan {
            source: self.source,
            start: item_start,
            end: node.end_byte() as u32,
        };

        LocalModItemSym::Function(LocalFnSym::new(self.db, name, self.scope, cst, abs_span))
    }
}
```

`item_start` is the byte offset of the first attribute (or the item
keyword if there are no attributes). All `RelativeSpan`s within the
stash are relative to this anchor. The `AbsoluteSpan` on the tracked
struct spans from the first attribute through the item's closing brace.

### Inline modules

```rust
impl<'a, 'db> Parser<'a, 'db> {
    fn parse_mod(&self, node: Node, pending_attrs: &[Node]) -> LocalModItemSym<'db> {
        let item_start = pending_attrs
            .first()
            .map_or(node.start_byte(), |a| a.start_byte()) as u32;

        let name = Name::new(self.db, node_text(node, "name"));
        let abs_span = AbsoluteSpan {
            source: self.source,
            start: item_start,
            end: node.end_byte() as u32,
        };

        if let Some(body) = node.child_by_field_name("body") {
            // Inline mod: peek shows it has a body, so we know the source up front.
            // Mint the module, then recurse with a child Parser.
            let mut stash = Stash::new();
            let attrs = self.parse_attr_nodes(&mut stash, pending_attrs, item_start);

            let child_parser = Parser {
                db: self.db,
                source: self.source,
                scope: ScopeSymbol::from(mod_sym),
                text: self.text,
            };
            let children = child_parser.parse_item_list(body);
            let items = stash.alloc_slice(&children);

            let cst_data = InlineModCstData { attrs, name, items, span: RelativeSpan { start: 0, end: node.end_byte() as u32 - item_start } };
            let root = stash.alloc(cst_data);
            let cst = Stashed::new(stash, root);

            let mod_sym = LocalModSym::new(
                self.db, name, Some(self.scope),
                ModSource::Inline(cst), abs_span,
            );
            LocalModItemSym::Mod(mod_sym)
        } else {
            // File-backed mod: no body, resolve the file path.
            let file = resolve_mod_file(self.db, name, self.scope);
            let mod_sym = LocalModSym::new(
                self.db, name, Some(self.scope),
                ModSource::File(file), abs_span,
            );
            LocalModItemSym::Mod(mod_sym)
        }
    }
}
```

Key points:
- We peek at whether `body` exists before minting `LocalModSym`, so we
  know `ModSource` at construction time (no deferred field setting).
- The inline mod's CST (`InlineModCstData`) stores attrs, name, items,
  and span. Children are `LocalModItemSym` handles (tracked struct IDs),
  each with their own per-item stash.
- `ModSource::Inline` holds `Stashed<Ptr<InlineModCstData>>` — the
  complete inline mod CST including attrs.
- A child `Parser` is created with `scope` set to the new module. This
  means we need the `LocalModSym` before recursing — we solve this by
  constructing it eagerly (we already know all fields).

Note: there's a sequencing subtlety — we need `mod_sym` for the child
scope but also need the children to build the CST. The code above shows
a forward-reference to `mod_sym` for illustration; in practice, we can
construct `mod_sym` in two steps or use the scope directly. See open
questions.

## Shared parse helpers

Methods on `Parser` that parse tree-sitter nodes into stash-allocated
CST nodes. They take `&self`, a `&mut Stash` (the current item's
stash), and return `Ptr<T>` or `Slice<T>`:

| Method | Produces | Notes |
|--------|----------|-------|
| `self.parse_type(&mut stash, node, item_start)` | `Ptr<TypeCst>` | Dispatches on `type_identifier`, `reference_type`, `tuple_type`, etc. |
| `self.parse_path(&mut stash, node, item_start)` | `Ptr<PathCst>` | Segments + type args |
| `self.parse_expr(&mut stash, node, item_start)` | `Ptr<ExprCst>` | Full expression tree (recursive) |
| `self.parse_pat(&mut stash, node, item_start)` | `Ptr<PatCst>` | Pattern tree |
| `self.parse_generics(&mut stash, node, item_start)` | `Slice<GenericParamCst>` | Type params from `type_parameters` node |
| `self.parse_where_clauses(&mut stash, node, item_start)` | `Slice<WhereClauseCst>` | Where clause entries |
| `self.parse_attr_nodes(&mut stash, nodes, item_start)` | `Slice<AttrCst>` | From buffered `attribute_item` nodes |
| `self.parse_params(&mut stash, node, item_start)` | `Slice<ParamCst>` | Function parameters |

All spans produced by these helpers are relative to the item's start
byte (subtract `item_start` from each node's byte range).

## Module structure

```
parse/
  mod.rs          — Parser struct, parse_str_to_cst, parse_item_list, top-level dispatch
  items.rs        — Parser methods: parse_fn, parse_struct, parse_enum,
                    parse_trait, parse_impl, parse_const, parse_static,
                    parse_type_alias, parse_mod, parse_use,
                    parse_macro_def, parse_macro_invocation
  types.rs        — Parser::parse_type (tree-sitter type nodes → TypeCst)
  paths.rs        — Parser::parse_path (tree-sitter path nodes → PathCst)
  exprs.rs        — Parser::parse_expr, parse_stmt, parse_pat
  generics.rs     — Parser::parse_generics, parse_where_clauses
  attrs.rs        — Parser::parse_attr_nodes
  util.rs         — node_text, child helpers, span arithmetic
```

Also requires changes to `cst/inline_mods.rs` (renamed from `cst/mods.rs`):
`InlineModCstData` gains `name: Name` and `span: RelativeSpan` fields.

## Incrementality

The key incremental property: **body-only edits don't invalidate
signatures.** This works because:

1. A body edit changes `SourceFile::text` → `unexpanded_items` re-runs.
2. tree-sitter re-parses the full file (fast, <1ms for typical files).
3. The parser mints tracked structs with the same identity keys
   (name + scope). Salsa recognizes them as the same entities.
4. The CST stash for the edited function changes → its `sig()` or
   `body()` re-runs.
5. Unedited functions get identical CST stashes → salsa short-circuits
   downstream queries via fingerprint comparison.

This means there is no need for a separate "item-level parse" that
avoids re-parsing bodies. The combination of tree-sitter's speed and
salsa's per-item fingerprinting gives us fine-grained incrementality
without a two-phase parse.

## Implementation plan

### Phase 1: Skeleton + structs (get the pipeline flowing)

- `parse/mod.rs`: `parse_str_to_cst` with tree-sitter setup + dispatch
- `parse/items.rs`: `parse_struct` only (simplest item with generics + fields)
- `parse/types.rs`: `parse_type` (Path, Reference, Tuple, primitives)
- `parse/paths.rs`: `parse_path`
- `parse/generics.rs`: `parse_generics`
- `parse/attrs.rs`: `parse_attrs` (can start as no-op returning empty slice)
- `parse/util.rs`: helpers
- Wire up `LocalModSym::unexpanded_items`
- Verify: existing struct sig tests pass end-to-end

### Phase 2: Functions (sigs + bodies)

- `parse/items.rs`: `parse_fn`
- `parse/exprs.rs`: `parse_expr`, `parse_stmt`, `parse_pat`
- Verify: fn sig + body tests pass

### Phase 3: Remaining items

- `parse_enum`, `parse_trait`, `parse_impl`, `parse_const`,
  `parse_static`, `parse_type_alias`
- `parse_mod` (inline + file-backed)
- `parse_use`, `parse_macro_def`, `parse_macro_invocation`

### Phase 4: Macro expansion integration

- Wire up `parse_str_to_cst` call from `memmap/expand.rs`
- Verify: macro expansion tests pass

## Open questions

1. **`LocalModSym` sequencing for inline mods.** The child `Parser`
   needs `mod_sym` for its scope, but `mod_sym` construction needs the
   inline CST which includes the children. Resolution: construct
   `mod_sym` first (we know all fields except `source`), or factor so
   the scope is derived from `(name, parent)` without needing the full
   struct yet. The peek-at-`body` approach (shown above) avoids deferred
   field setting but introduces a forward reference.

2. **Identity stability for tracked structs.** Salsa needs stable
   identity across re-parses. The identity fields for `LocalFnSym` are
   `(name, scope)`. If two functions have the same name in the same
   scope (which Rust forbids but we might encounter in malformed input),
   we need a disambiguation strategy (e.g., ordinal suffix).

3. **Error recovery.** tree-sitter produces `ERROR` nodes for malformed
   input. Strategy: emit `LocalModItemSym::Error(span)` and continue.
   For partial items (e.g., function missing a body), parse what's
   available and use `None`/`Missing` for absent parts.

4. **`use` item parsing.** The current `LocalUseSym` stores a
   `Stashed` import tree. Need to understand the existing `UseKind` /
   import resolution shape before implementing.

5. **`ModSource::Inline` type.** Currently defined as
   `Stashed<Slice<LocalModItemSym>>` in the code. Should change to
   `Stashed<Ptr<InlineModCstData>>` to carry attrs/name/span alongside
   the child items. `unexpanded_items` would then open the stash and
   return the items slice.

---

## Appendix: tree-sitter → CST mapping reference

All dumps produced with `tree-sitter-rust 0.24`. Each section shows:
the Rust source, the tree-sitter node structure, and the target CST
representation we produce.

---

### A. Items

#### A.1 Function (with attributes)

```rust
#[foo]
#[bar(baz)]
fn hello(x: u32) -> bool { x > 0 }
```

**tree-sitter:**
```
source_file [0-53]
  attribute_item [0-6]
    attribute [2-5]
      identifier "foo"
  attribute_item [7-18]
    attribute [9-17]
      identifier "bar"
      arguments: token_tree [12-17]
  function_item [19-53]
    name: identifier "hello"
    parameters: parameters [27-35]
      parameter [28-34]
        pattern: identifier "x"
        type: primitive_type "u32"
    return_type: primitive_type "bool"
    body: block [44-53]
      binary_expression [46-51]
        left: identifier "x"
        operator: >
        right: integer_literal "0"
```

**CST produced:** `Stashed<Ptr<FnCstData>>` (item_start = 0)
```rust
FnCstData {
    attrs: [
        AttrCst { kind: Normal, path: ["foo"], args: None, is_inner: false, span: 0..6 },
        AttrCst { kind: Normal, path: ["bar"], args: Some("baz"), is_inner: false, span: 7..18 },
    ],
    name: Name("hello"),
    generics: [],
    params: [ParamCst { name: Some(Name("x")), ty: Ptr→TypeCst(Path→"u32"), span: 28..34 }],
    ret: Some(Ptr→TypeCst(Path→"bool")),
    body: Some(Ptr→ExprCst(Binary(x, Gt, 0))),
    where_clauses: [],
    span: 0..53,
}
```

**Tracked struct:** `LocalFnSym::new(db, Name("hello"), scope, cst, AbsoluteSpan { source, 0, 53 })`

Note: `item_start` = first attribute's start (byte 0), not the `fn` keyword (byte 19).

#### A.2 Function signature (in trait body)

```rust
fn fmt(&self) -> String;
```

**tree-sitter** (inside a `declaration_list`):
```
function_signature_item [16-40]
  name: identifier "fmt"
  parameters: parameters [22-29]
    self_parameter [23-28]
      & "self"
  return_type: type_identifier "String"
```

Same CST as A.1 but with `body: None`. The `self_parameter` becomes
`ParamCst { name: Some(Name("self")), ty: Ptr→TypeCst(Path→"Self"), ... }` with
a synthesized `&Self` type.

#### A.3 Struct (named fields)

```rust
#[derive(Debug)]
struct Pair<T> { first: T, second: T }
```

**tree-sitter:**
```
source_file [0-55]
  attribute_item [0-16]
    attribute [2-15]
      identifier "derive"
      arguments: token_tree [8-15]
  struct_item [17-55]
    name: type_identifier "Pair"
    type_parameters: type_parameters [28-31]
      type_parameter [29-30]
        name: type_identifier "T"
    body: field_declaration_list [32-55]
      field_declaration [34-42]
        name: field_identifier "first"
        type: type_identifier "T"
      field_declaration [44-53]
        name: field_identifier "second"
        type: type_identifier "T"
```

**CST:** `Stashed<Ptr<StructCstData>>`
```rust
StructCstData {
    attrs: [AttrCst { path: ["derive"], args: Some("Debug"), .. }],
    name: Name("Pair"),
    generics: [GenericParamCst::Type { name: Name("T"), bounds: [], .. }],
    fields: [
        FieldCst { name: Name("first"), ty: Ptr→TypeCst(Path→"T"), .. },
        FieldCst { name: Name("second"), ty: Ptr→TypeCst(Path→"T"), .. },
    ],
    where_clauses: [],
    span: 0..55,
}
```

#### A.4 Tuple struct

```rust
struct Point(f64, f64);
```

**tree-sitter:**
```
struct_item [0-23]
  name: type_identifier "Point"
  body: ordered_field_declaration_list [12-22]
    type: primitive_type "f64"
    type: primitive_type "f64"
```

**CST:** Same `StructCstData` — fields get synthesized numeric names
(`Name("0")`, `Name("1")`).
```rust
StructCstData {
    attrs: [],
    name: Name("Point"),
    generics: [],
    fields: [
        FieldCst { name: Name("0"), ty: Ptr→TypeCst(Path→"f64"), .. },
        FieldCst { name: Name("1"), ty: Ptr→TypeCst(Path→"f64"), .. },
    ],
    where_clauses: [],
    span: 0..23,
}
```

#### A.5 Unit struct

```rust
struct Unit;
```

**tree-sitter:**
```
struct_item [0-12]
  name: type_identifier "Unit"
```

No `body` field at all. **CST:** `StructCstData` with `fields: []`.

#### A.6 Enum

```rust
enum Option<T> { Some(T), None }
```

**tree-sitter:**
```
enum_item [0-32]
  name: type_identifier "Option"
  type_parameters: type_parameters [11-14]
    type_parameter [12-13]
      name: type_identifier "T"
  body: enum_variant_list [15-32]
    enum_variant [17-24]
      name: identifier "Some"
      body: ordered_field_declaration_list [21-24]
        type: type_identifier "T"
    enum_variant [26-30]
      name: identifier "None"
```

**CST:** `Stashed<Ptr<EnumCstData>>`
```rust
EnumCstData {
    attrs: [],
    name: Name("Option"),
    generics: [GenericParamCst::Type { name: Name("T"), .. }],
    variants: [
        VariantCst { name: Name("Some"), fields: [FieldCst { name: Name("0"), ty: Path→"T", .. }], discriminant: None, .. },
        VariantCst { name: Name("None"), fields: [], discriminant: None, .. },
    ],
    where_clauses: [],
    span: 0..32,
}
```

#### A.7 Trait

```rust
trait Display { fn fmt(&self) -> String; }
```

**tree-sitter:**
```
trait_item [0-42]
  name: type_identifier "Display"
  body: declaration_list [14-42]
    function_signature_item [16-40]
      name: identifier "fmt"
      parameters: parameters [22-29]
        self_parameter [23-28]
      return_type: type_identifier "String"
```

**CST:** `Stashed<Ptr<TraitCstData>>`
```rust
TraitCstData {
    attrs: [],
    name: Name("Display"),
    generics: [],
    where_clauses: [],
    items: [TraitItemCst::Fn(Ptr→FnCstData { name: "fmt", body: None, .. })],
    span: 0..42,
}
```

Note: trait items share the same stash as the parent `TraitCstData`.

#### A.8 Impl (inherent)

```rust
impl<T> Pair<T> { fn first(&self) -> &T { &self.first } }
```

**tree-sitter:**
```
impl_item [0-57]
  type_parameters: type_parameters [4-7]
    type_parameter [5-6]
      name: type_identifier "T"
  type: generic_type [8-15]
    type: type_identifier "Pair"
    type_arguments: type_arguments [12-15]
      type_identifier "T"
  body: declaration_list [16-57]
    function_item [18-55]
      name: identifier "first"
      parameters: parameters [26-33]
        self_parameter [27-32]
      return_type: reference_type [37-39]
        type: type_identifier "T"
      body: block [40-55]
```

**CST:** `Stashed<Ptr<ImplCstData>>`
```rust
ImplCstData {
    attrs: [],
    generics: [GenericParamCst::Type { name: Name("T"), .. }],
    self_ty: Ptr→TypeCst(Path→"Pair" with type_args [Path→"T"]),
    trait_path: None,
    where_clauses: [],
    items: [TraitItemCst::Fn(Ptr→FnCstData { name: "first", body: Some(..), .. })],
    span: 0..57,
}
```

#### A.9 Trait impl

```rust
impl Display for Pair<T> where T: Display { fn fmt(&self) -> String { todo!() } }
```

**tree-sitter:**
```
impl_item [0-81]
  trait: type_identifier "Display"
  type: generic_type [17-24]
    type: type_identifier "Pair"
    type_arguments: type_arguments [21-24]
      type_identifier "T"
  where_clause [25-41]
    where_predicate [31-41]
      left: type_identifier "T"
      bounds: trait_bounds [32-41]
        type_identifier "Display"
  body: declaration_list [42-81]
    function_item [44-79]
```

**CST:** Same `ImplCstData` with `trait_path: Some(Ptr→PathCst(["Display"]))`.

#### A.10 Const item

```rust
const MAX: u32 = 100;
```

**tree-sitter:**
```
const_item [0-21]
  name: identifier "MAX"
  type: primitive_type "u32"
  value: integer_literal "100"
```

**CST:** `Stashed<Ptr<ConstCstData>>`
```rust
ConstCstData {
    attrs: [],
    name: Name("MAX"),
    ty: Some(Ptr→TypeCst(Path→"u32")),
    value: Some(Ptr→ExprCst(Literal(Int))),
    span: 0..21,
}
```

#### A.11 Static item

```rust
static mut COUNTER: i32 = 0;
```

**tree-sitter:**
```
static_item [0-28]
  mutable_specifier "mut"
  name: identifier "COUNTER"
  type: primitive_type "i32"
  value: integer_literal "0"
```

**CST:** `Stashed<Ptr<StaticCstData>>`
```rust
StaticCstData {
    attrs: [],
    name: Name("COUNTER"),
    is_mut: true,
    ty: Some(Ptr→TypeCst(Path→"i32")),
    value: Some(Ptr→ExprCst(Literal(Int))),
    span: 0..28,
}
```

Detection: check for `mutable_specifier` child node.

#### A.12 Type alias

```rust
type Result<T> = std::result::Result<T, Error>;
```

**tree-sitter:**
```
type_item [0-47]
  name: type_identifier "Result"
  type_parameters: type_parameters [11-14]
    type_parameter [12-13]
      name: type_identifier "T"
  type: generic_type [17-46]
    type: scoped_type_identifier [17-36]
      path: scoped_identifier [17-28]
        path: identifier "std"
        name: identifier "result"
      name: type_identifier "Result"
    type_arguments: type_arguments [36-46]
      type_identifier "T"
      type_identifier "Error"
```

**CST:** `Stashed<Ptr<TypeAliasCstData>>`
```rust
TypeAliasCstData {
    attrs: [],
    name: Name("Result"),
    generics: [GenericParamCst::Type { name: Name("T"), .. }],
    ty: Some(Ptr→TypeCst(Path(["std", "result", "Result"], type_args=[Path→"T", Path→"Error"]))),
    where_clauses: [],
    span: 0..47,
}
```

#### A.13 Inline module

```rust
mod shapes {
    struct Circle { radius: f64 }
}
```

**tree-sitter:**
```
mod_item [0-47]
  name: identifier "shapes"
  body: declaration_list [11-47]
    struct_item [17-46]
      name: type_identifier "Circle"
      body: field_declaration_list [31-46]
        field_declaration [33-44]
```

Detection: `node.child_by_field_name("body").is_some()`

**CST:** `Stashed<Ptr<InlineModCstData>>` (from `cst::inline_mods`)
```rust
InlineModCstData {
    attrs: [],
    name: Name("shapes"),
    items: [LocalModItemSym::Struct(circle_sym)],
    span: 0..47,
}
```

**Result:** `LocalModSym::new(db, Name("shapes"), parent, ModSource::Inline(cst), span)`

The inline mod's CST stores child `LocalModItemSym` handles — the
children are recursively parsed and minted as tracked structs (each with
their own per-item stash).

#### A.14 File-backed module

```rust
mod utils;
```

**tree-sitter:**
```
mod_item [0-10]
  name: identifier "utils"
```

Detection: no `body` field. **Result:** `LocalModSym::new(db, ..., ModSource::File(resolved_file), span)`

#### A.15 Use declaration

```rust
use std::collections::HashMap;
use std::{collections::HashMap, io::{self, Read}};
use std::collections::*;
use std::io::Result as IoResult;
```

**tree-sitter (simple):**
```
use_declaration [0-30]
  argument: scoped_identifier [4-29]
    path: scoped_identifier [4-20]
      path: identifier "std"
      name: identifier "collections"
    name: identifier "HashMap"
```

**tree-sitter (nested group):**
```
use_declaration [0-50]
  argument: scoped_use_list [4-49]
    path: identifier "std"
    list: use_list [9-49]
      scoped_identifier [10-30]
        path: identifier "collections"
        name: identifier "HashMap"
      scoped_use_list [32-48]
        path: identifier "io"
        list: use_list [36-48]
          self
          identifier "Read"
```

**tree-sitter (glob):**
```
use_declaration [0-24]
  argument: use_wildcard [4-23]
    scoped_identifier [4-20]
      path: identifier "std"
      name: identifier "collections"
```

**tree-sitter (rename):**
```
use_declaration [0-32]
  argument: use_as_clause [4-31]
    path: scoped_identifier [4-19]
    alias: identifier "IoResult"
```

Produces `LocalUseSym` with `Stashed` import tree. See open question #4.

#### A.16 Macro invocation

```rust
define_shape!(Circle, radius, f64);
```

**tree-sitter:**
```
expression_statement [0-35]
  macro_invocation [0-34]
    macro: identifier "define_shape"
    ! [12-13]
    token_tree [13-34]
      ( ... )
```

Note: top-level macros are wrapped in `expression_statement`. Must
unwrap to find the `macro_invocation` inside.

**Result:** `LocalMacroInvocationSym` with the path (`["define_shape"]`)
and raw token tree text.

#### A.17 Macro definition

```rust
macro_rules! my_macro { ($x:expr) => { $x + 1 }; }
```

**tree-sitter:**
```
macro_definition [0-50]
  name: identifier "my_macro"
  macro_rule [24-47]
    left: token_tree_pattern [24-33]
    right: token_tree [37-47]
```

**Result:** `LocalMacroDefSym` with name and raw rule body.

---

### B. Types

tree-sitter type nodes → `TypeCst { kind: TypeCstKind, span }`:

#### B.1 Simple path types

| Source | tree-sitter node | `TypeCstKind` |
|--------|-----------------|---------------|
| `u32` | `primitive_type "u32"` | `Path(PathCst { segments: [("u32", [])] })` |
| `T` | `type_identifier "T"` | `Path(PathCst { segments: [("T", [])] })` |
| `String` | `type_identifier "String"` | `Path(PathCst { segments: [("String", [])] })` |

#### B.2 Scoped path type

```rust
std::result::Result<T, Error>
```

**tree-sitter:**
```
generic_type [17-46]
  type: scoped_type_identifier [17-36]
    path: scoped_identifier [17-28]
      path: identifier "std"
      name: identifier "result"
    name: type_identifier "Result"
  type_arguments: type_arguments [36-46]
    type_identifier "T"
    type_identifier "Error"
```

**CST:** `TypeCstKind::Path(PathCst { segments: [("std",[]), ("result",[]), ("Result",[T,Error])] })`

Key: `scoped_type_identifier` unrolls into path segments. `type_arguments` attach to the last segment.

#### B.3 Reference types

| Source | tree-sitter | `TypeCstKind` |
|--------|------------|---------------|
| `&T` | `reference_type { & type_identifier "T" }` | `Reference(Ptr→T, Shared)` |
| `&mut T` | `reference_type { & mutable_specifier type_identifier "T" }` | `Reference(Ptr→T, Mutable)` |
| `&'a str` | `reference_type { & lifetime('a) primitive_type "str" }` | `Reference(Ptr→str, Shared)` |

Detection of mutability: presence of `mutable_specifier` child.

#### B.4 Compound types

| Source | tree-sitter node | `TypeCstKind` |
|--------|-----------------|---------------|
| `[u8]` | `array_type { [ element: "u8" ] }` (no `;`) | `Slice(Ptr→u8)` |
| `[u8; 4]` | `array_type { [ element: "u8" ; length: 4 ] }` | `Array(Ptr→u8)` |
| `(u32, bool)` | `tuple_type { ( "u32" , "bool" ) }` | `Tuple([Ptr→u32, Ptr→bool])` |
| `!` | `never_type { ! }` | `Never` |

Slice vs array: distinguished by presence of `;` + `length:` child in `array_type`.

#### B.5 Function pointer

```rust
fn(u32) -> bool
```

**tree-sitter:**
```
function_type [45-60]
  parameters: parameters [47-52]
    primitive_type "u32"
  return_type: primitive_type "bool"
```

**CST:** `TypeCstKind::Fn([Ptr→u32], Some(Ptr→bool))`

#### B.6 Dyn / impl trait

| Source | tree-sitter | `TypeCstKind` |
|--------|------------|---------------|
| `dyn Display` | `dynamic_type { dyn trait: type_identifier "Display" }` | `Path(PathCst(["Display"]))` (for now) |
| `impl Clone` | `abstract_type { impl trait: type_identifier "Clone" }` | `Path(PathCst(["Clone"]))` (for now) |

These will need richer representation later (trait object / impl Trait),
but for initial implementation we can desugar to path.

#### B.7 Qualified path

```rust
<T as Trait>::Item
```

**tree-sitter:**
```
scoped_type_identifier [29-47]
  path: bracketed_type [29-41]
    qualified_type [30-40]
      type: type_identifier "T"
      alias: type_identifier "Trait"
  name: type_identifier "Item"
```

**CST:** `TypeCstKind::Error` initially (associated type projection is out of scope for phase 1).

---

### C. Generics and where clauses

#### C.1 Type parameters with bounds

```rust
<T: Clone + Send, U>
```

**tree-sitter:**
```
type_parameters [10-30]
  type_parameter [11-26]
    name: type_identifier "T"
    bounds: trait_bounds [12-26]
      type_identifier "Clone"
      + [20-21]
      type_identifier "Send"
  type_parameter [28-29]
    name: type_identifier "U"
```

**CST:**
```rust
[
    GenericParamCst::Type { name: Name("T"), bounds: [Trait(Path→"Clone"), Trait(Path→"Send")], .. },
    GenericParamCst::Type { name: Name("U"), bounds: [], .. },
]
```

#### C.2 Lifetime parameters

```rust
<'a, 'b: 'a>
```

**tree-sitter:**
```
type_parameters [9-21]
  lifetime_parameter [10-12]
    name: lifetime [10-12] ('a)
  lifetime_parameter [14-20]
    name: lifetime [14-16] ('b)
    bounds: trait_bounds [16-20]
      lifetime ('a)
```

**CST:**
```rust
[
    GenericParamCst::Lifetime { name: Name("a"), .. },
    GenericParamCst::Lifetime { name: Name("b"), .. },
]
```

Note: lifetime bounds are not yet represented in `GenericParamCst::Lifetime`.

#### C.3 Where clause

```rust
where U: Into<T>
```

**tree-sitter:**
```
where_clause [48-64]
  where_predicate [54-64]
    left: type_identifier "U"
    bounds: trait_bounds [55-64]
      generic_type [57-64]
        type: type_identifier "Into"
        type_arguments: type_arguments [61-64]
          type_identifier "T"
```

**CST:**
```rust
WhereClauseCst {
    subject: Ptr→TypeCst(Path→"U"),
    bounds: [TypeBoundCst::Trait(Ptr→PathCst(["Into"], type_args=[Path→"T"]))],
    span: ..,
}
```

---

### D. Expressions

#### D.1 Literals and identifiers

| Source | tree-sitter node | `ExprCstKind` |
|--------|-----------------|---------------|
| `42` | `integer_literal` | `Literal(Int)` |
| `3.14` | `float_literal` | `Literal(Float)` |
| `"hi"` | `string_literal` | `Literal(String)` |
| `true` | `boolean_literal { true }` | `Literal(Bool(true))` |
| `'c'` | `char_literal` | `Literal(Char)` |
| `x` | `identifier "x"` | `Path(PathCst(["x"]))` |

#### D.2 Binary / unary / assign

```rust
x > 0
x + 1
!flag
-n
x -= 1
```

**tree-sitter:**
```
binary_expression { left: identifier "x", operator: >, right: integer_literal "0" }
binary_expression { left: identifier "x", operator: +, right: integer_literal "1" }
unary_expression { operator: !, operand: identifier "flag" }
unary_expression { operator: -, operand: identifier "n" }
compound_assignment_expr { left: identifier "x", operator: -=, right: integer_literal "1" }
```

**CST:**
- `Binary(Ptr→x, Gt, Ptr→0)`, `Binary(Ptr→x, Add, Ptr→1)`
- `Unary(Not, Ptr→flag)`, `Unary(Neg, Ptr→n)`
- `Assign(Ptr→x, Ptr→Binary(x, Sub, 1))` (desugar compound assign)

#### D.3 Call / method call / field access

```rust
foo(x, y)
v.iter().map(f)
p.first
```

**tree-sitter:**
```
call_expression { function: identifier "foo", arguments: arguments { identifier "x", identifier "y" } }
call_expression {
  function: field_expression {
    value: call_expression { function: field_expression { value: identifier "v", field: "iter" }, arguments: () }
    field: "map"
  }
  arguments: arguments { identifier "f" }
}
field_expression { value: identifier "p", field: field_identifier "first" }
```

**CST:**
- `Call(Ptr→Path("foo"), [Ptr→Path("x"), Ptr→Path("y")])`
- `Call(Ptr→MethodCall(Ptr→Call(Ptr→MethodCall(Ptr→Path("v"), "iter", []), "map", [Ptr→Path("f")]))`
  Actually: method calls in tree-sitter appear as `call_expression` on a `field_expression`.
  We detect this pattern: `Call(field_expr, args)` where field_expr is `field_expression`
  → `MethodCall(base, field_name, args)`.
- `Field(Ptr→Path("p"), Name("first"))`

#### D.4 Block / if / match

```rust
{ let x = 1; x + 1 }
if cond { a } else { b }
match x { 0 => false, _ => true }
```

**tree-sitter:**
```
block { let_declaration { pattern: "x", value: 1 }, binary_expression { "x", +, 1 } }
if_expression { condition: "cond", consequence: block { "a" }, alternative: else_clause { block { "b" } } }
match_expression { value: "x", body: match_block { match_arm { pattern: 0, value: false }, match_arm { pattern: _, value: true } } }
```

**CST:**
- `Block([Stmt(Let(Ptr→Pat(Bind("x")), None, Some(Ptr→Lit(Int))))], Some(Ptr→Binary(x,Add,1)))`
- `If(Ptr→Path("cond"), Ptr→Block(.."a"..), Some(Ptr→Block(.."b"..)))`
- `Match(Ptr→Path("x"), [MatchArmCst { pat: Lit(Int), body: Lit(Bool(false)) }, MatchArmCst { pat: Wildcard, body: Lit(Bool(true)) }])`

#### D.5 Loops

| Source | tree-sitter | `ExprCstKind` |
|--------|------------|---------------|
| `loop { break 5; }` | `loop_expression { body: block { break_expression { integer_literal } } }` | `Loop(Ptr→Block(..))` |
| `while x > 0 { .. }` | `while_expression { condition: .., body: block }` | `While(Ptr→cond, Ptr→body)` |
| `for i in 0..10 { .. }` | `for_expression { pattern: "i", value: range_expression, body: block }` | `For(Ptr→Pat, Ptr→range, Ptr→body)` |

#### D.6 Closure

```rust
|x: u32| x + 1
```

**tree-sitter:**
```
closure_expression [22-36]
  parameters: closure_parameters [22-30]
    parameter [23-29]
      pattern: identifier "x"
      type: primitive_type "u32"
  body: binary_expression [31-36]
```

**CST:** `Closure([ClosureParamCst { pat: Ptr→Bind("x"), ty: Some(Ptr→u32), .. }], Ptr→Binary(x,Add,1))`

#### D.7 Struct literal

```rust
Point { x: 1, y: 2 }
```

**tree-sitter:**
```
struct_expression [46-66]
  name: type_identifier "Point"
  body: field_initializer_list [52-66]
    field_initializer { field: "x", value: integer_literal "1" }
    field_initializer { field: "y", value: integer_literal "2" }
```

**CST:** `StructLit(Ptr→PathCst(["Point"]), [FieldInitCst { name: "x", value: Ptr→Lit(Int) }, ...])`

#### D.8 Range / index / cast / try / await

| Source | tree-sitter | `ExprCstKind` |
|--------|------------|---------------|
| `0..10` | `range_expression { 0, .., 10 }` | `Range(Some(Ptr→0), Some(Ptr→10))` |
| `arr[0]` | `index_expression { "arr", [ 0 ] }` | `Index(Ptr→arr, Ptr→0)` |
| `x as u64` | `type_cast_expression { value: "x", type: "u64" }` | `Cast(Ptr→x, Ptr→TypeCst(u64))` |
| `foo()?` | `try_expression { call_expression { .. }, ? }` | `Try(Ptr→Call(..))` |
| `bar().await` | `await_expression { call_expression, .await }` | `Await(Ptr→Call(..))` |

---

### E. Patterns

#### E.1 Basic patterns

| Source | tree-sitter | `PatCstKind` |
|--------|------------|--------------|
| `_` | `_` | `Wildcard` |
| `x` | `identifier "x"` | `Bind(Name("x"), Shared)` |
| `mut x` | `mut_pattern { identifier "x" }` | `Bind(Name("x"), Mutable)` |
| `42` | `integer_literal` | `Literal(Int)` |

#### E.2 Structural patterns

```rust
Some(Point { x, y: _ })
```

**tree-sitter:**
```
tuple_struct_pattern [38-61]
  type: identifier "Some"
  struct_pattern [43-60]
    type: type_identifier "Point"
    field_pattern { name: shorthand_field_identifier "x" }
    field_pattern { name: field_identifier "y", pattern: _ }
```

**CST:**
```rust
PatCstKind::TupleStruct(
    Ptr→PathCst(["Some"]),
    [Ptr→Pat(Struct(Ptr→PathCst(["Point"]), [
        FieldPatCst { name: "x", pat: Ptr→Bind("x") },
        FieldPatCst { name: "y", pat: Ptr→Wildcard },
    ]))]
)
```

Note: `shorthand_field_identifier "x"` means both the field name and
binding are "x" — expand to `FieldPatCst { name: "x", pat: Bind("x") }`.

#### E.3 Or pattern and rest

```rust
None | Some(..)
```

**tree-sitter:**
```
or_pattern [68-83]
  identifier "None"
  tuple_struct_pattern [75-83]
    type: identifier "Some"
    remaining_field_pattern [80-82] ".."
```

**CST:** `PatCstKind::Or([Ptr→Path("None"), Ptr→TupleStruct(Path("Some"), [Ptr→Rest])])`

---

### F. Use declarations (node structure reference)

The `use_declaration` `argument:` field can be one of several node
kinds. Full tree-sitter dump for all forms:

**Simple named:**
```
use_declaration
  argument: scoped_identifier
    path: scoped_identifier { path: identifier "std", name: identifier "collections" }
    name: identifier "HashMap"
```

**Glob:**
```
use_declaration
  argument: use_wildcard
    scoped_identifier { path: identifier "std", name: identifier "collections" }
    *
```

**Nested group:**
```
use_declaration
  argument: scoped_use_list
    path: identifier "std"
    list: use_list
      scoped_identifier { path: "collections", name: "HashMap" }
      scoped_use_list { path: "io", list: use_list { self, identifier "Read" } }
```

**Rename:**
```
use_declaration
  argument: use_as_clause
    path: scoped_identifier { .. }
    alias: identifier "IoResult"
```

These are the four `argument:` variants the parser must handle.
Recursive processing of `use_list` produces the `Stashed` import tree
stored in `LocalUseSym`.
