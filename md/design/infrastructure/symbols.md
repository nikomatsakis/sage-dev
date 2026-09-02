# Symbols and Semantic Identity

Symbols are the semantic spine of Sage for top-level and associated
definitions. A symbol identifies such a definition; it does not eagerly
contain everything Sage may learn about it. Signatures, members, fields,
associated values, and bodies remain separate queries keyed by the symbol.

That separation gives downstream code a compact common language and gives
Salsa a precise unit of reuse. A body edit can change the result of
`function.body(db)` without changing the function symbol or forcing consumers
of `function.sig(db)` to treat it as a different definition.

## Contract

### Identity, names, and paths

For a definition represented in the symbol family, its symbol is its semantic
identity within one Sage database. Names and paths are not identities:

- multiple definitions can have the same name in different scopes or
  namespaces;
- imports and aliases can provide several paths to the same definition; and
- some definitions, such as impls, are anonymous.

Resolution consumes a path, scope, and namespace and returns symbols. Once a
symbol is known, later semantic queries use it directly rather than resolving
the spelling again.

Generic parameters form a separate `GenericParam` identity family. Fields use
an owner plus field index, and locals use body-local IDs. These scoped
identities participate in semantic IR without pretending to be variants of the
erased `Symbol` family.

This is the identity-model consequence of
[D5](../decisions.md#d5-symbols-form-the-uniform-semantic-identity-family).

<a id="sym-a1"></a>
> **SYM-A1 — Definition identity is not a name or path.** Resolution may use
> names, paths, scopes, and namespaces to discover a definition, but every
> successful downstream semantic reference records the resulting symbol.
> Renaming an import or choosing another path to the same definition must not
> mint a different definition identity.
>
> **Required verification:** Resolution and completed-output tests show that
> distinct paths to one definition produce the same symbol, while equal
> spellings in different scopes or namespaces remain distinct.

### Local symbols

Each represented local item kind has a Salsa tracked struct such as
`LocalFnSym`, `LocalStructSym`, or `LocalTraitSym`. Fields that participate in
tracked-struct identity describe which definition it is; `#[tracked]` fields
carry details that may change independently across revisions. The exact split
is per item kind and is part of its incremental contract.

For a function, the stable part includes its name, scope, and optional
trait/impl owner. The CST and spans are tracked separately:

```{anchor}
architecture_local_function_symbol
```

This is why `LocalFnSym::sig(db)` and `LocalFnSym::body(db)` can be independent
memoized operations over the same definition identity.

<a id="sym-a2"></a>
> **SYM-A2 — Local identity is narrower than local detail.** A local symbol's
> identity fields identify the definition; independently tracked CST, span,
> signature, members, and body facts do not become part of that identity.
> Editing one such fact may invalidate its readers without replacing the
> symbol or invalidating consumers of unchanged sibling facts.
>
> **Required verification:** Per-kind edit matrices cover detail-only changes,
> sibling insertion and reordering, and genuine identity changes. They assert
> both symbol continuity and the execution or reuse of representative
> signature, member, and body queries.

Generated items reenter the same representation. A function or impl parsed
from macro output is an ordinary local symbol whose `ParseSource` records the
expansion that produced it. Downstream signature and body checking do not need
a parallel “macro item” type.

### External symbols

Definitions in reachable dependency crates are represented by `SymExt`. The
handle carries the dependency crate number, definition index, and represented
definition kind:

```{anchor}
architecture_external_symbol
```

The `(crate_num, def_index)` pair is the logical external definition identity.
The current interned representation also includes `kind`; metadata adapters
must therefore provide one consistent kind for a definition. Names and
namespace edges are queried metadata and are not part of logical definition
identity.

External symbols are handles, not imported rustc IR. A keyed tracked query
asks `TcxDb` for one owned metadata fact and lowers it into Sage types. Rustc
may authoritatively report dependency signatures, impl headers, or associated
values, but it does not resolve local Sage paths or solve Sage trait goals.

This is the symbol-layer consequence of
[D18](../decisions.md#d18-external-providers-supply-facts-not-sage-semantic-answers).

<a id="sym-a3"></a>
> **SYM-A3 — External identity and external facts stay separate.** An external
> symbol identifies one dependency definition independently of the metadata
> facts requested about it. Sage imports those facts through owned, keyed
> queries; the metadata provider neither resolves Sage-local names nor performs
> Sage's trait or normalization operations.
>
> **Required verification:** External-metadata traces identify the exact
> definition-keyed facts read by representative signature, body, proof, and
> normalization requests, reject unrelated fact reads, and show Sage solver
> queries executing on the imported values.

### Erased and kind-specific wrappers

Module contents are heterogeneous, so `Symbol` is a small erased wrapper over
all represented definition kinds. Semantic operations that require a
particular kind use wrappers such as `FnSymbol`, `StructSymbol`,
`TraitSymbol`, and `ModSymbol`. Most kind-specific wrappers have `Local` and
`Ext` variants; intrinsically local concepts omit the external variant.

The family is declared together so conversions and classification stay
consistent:

```{anchor}
architecture_symbol_family
```

This yields two complementary APIs:

```text
Symbol                     heterogeneous membership and resolution result
FnSymbol / TraitSymbol     kind-checked local-or-external semantic operation
LocalFnSym / LocalTraitSym local source identity and local-only queries
SymExt                     external definition handle
```

`SymbolData` exposes classification when a consumer must branch by kind.
Callers should retain the narrower wrapper once the required kind is known.

<a id="sym-a4"></a>
> **SYM-A4 — Erasure is used only for heterogeneous membership.** `Symbol`
> permits mixed symbol collections and resolution results, while operations
> whose meaning requires a definition kind accept a kind-specific wrapper.
> Runtime classification must not replace the narrower type throughout the
> semantic API.
>
> **Required verification:** API and compile-time tests cover lossless
> conversion from each supported kind into `Symbol`, checked recovery of the
> right kind, rejection of the wrong kind, and local/external dispatch through
> the corresponding kind wrapper.

### Ownership and associated items

A scope anchors local lookup. `ScopeSymbol` identifies a crate or module and
allows a local symbol to recover its owning crate. Modules link to their
parent scope; the root module reaches the `LocalCrateSymbol` that carries the
edition.

Associated functions, types, and constants have their own stable symbols plus
a `LocalAssociatedOwner` identifying the trait or impl. Querying
`LocalTraitSym::items` or `LocalImplSym::items` mints those identities lazily.
An associated function signature opens the owner binder, reuses the owner's
generic parameter identities and `Self`, and then adds method generics. It does
not depend on a sibling associated body.

Generic parameters form a related identity family rather than variants of
`Symbol`:

- `AstGenericParam` is a tracked local parameter tied to its parent symbol and
  declaration index, with its source span tracked as mutable detail;
- `ExtGenericParam` is interned from dependency metadata; and
- `AlphaEquivParam` is an interned placeholder used when comparing binders.

Types refer to these parameter identities directly.

<a id="sym-a5"></a>
> **SYM-A5 — Associated items own stable identities under their binder.** A
> trait or impl item has its own symbol tied to a stable associated owner.
> Opening an associated signature reuses the owner's `Self` and generic
> parameter identities before introducing item generics, and never requires a
> sibling associated body.
>
> **Required verification:** Trait and impl fixtures cover functions, types,
> and constants; compare owner and generic identities across repeated and
> edited requests; and use query traces to exclude sibling-body dependencies.

### Symbols in types and Typed IR

Once resolution succeeds, semantic output records the identity, not only its
spelling:

- `Ty::Adt` stores the ADT `Symbol` and type arguments;
- alias types store a `TypeAliasSymbol`;
- trait references store a `TraitSymbol`;
- a resolved path in Typed IR stores `PathResolution::Def(Symbol)`; and
- a resolved call target stores its `FnSymbol` and dispatch information.

Fields are currently identified in a completed body by an owner symbol plus a
field index. Local variables have body-local IDs. These identities are scoped
to their owning semantic structure rather than being global `Symbol` values.

## Incremental guarantees

The destination invariant is that edits to details behind a definition do not
mint a replacement symbol when the definition's semantic identity is
unchanged. Consumers still reexecute when they observed a changed tracked
field or query result.

Examples:

- moving an item without changing its identity updates its absolute span while
  preserving queries that depend only on semantic content;
- editing a function body may invalidate its body query but not consumers of
  an unchanged signature; and
- changing one external metadata fact invalidates the keyed lowering queries
  that observed that fact, not every external symbol.

Identity preservation is distinct from value backdating. A tracked query may
reexecute after an edit and produce an equal output; Salsa can then prevent
that unchanged value from propagating further.

## Code map

| Path | Responsibility |
|---|---|
| `symbol/mod.rs` | erased `Symbol`, kind wrappers, external handles, local/external dispatch |
| `local_syms/` | per-kind local tracked identities and symbol-keyed semantic queries |
| `scope.rs` | local crate/module ownership and edition |
| `generic_param.rs` | local, external, and alpha-equivalent parameter identities |
| `ty.rs` | checked types, signatures, binders, and symbol references |
| `tytree/mod.rs` | resolved definitions, fields, and call targets in completed bodies |
| `external_syms.rs` | keyed lowering from owned external metadata to Sage representations |

## Current status

### Current frontier

Local and external item identities, kind-specific wrappers, associated-item
ownership, generic parameter identities, and symbol references in checked
types and the implemented Typed IR slices are operational.

### Implemented capabilities and evidence

- **[SYM-A2](#sym-a2) — Generated-source identity.** The test
  `moving_source_item_preserves_derive_expansion_identity` moves a derived
  source item, verifies that its expansion identity is unchanged, and verifies
  that the updated origin coordinate remains observable.
- **[SYM-A2](#sym-a2) — Distinct generated occurrences.** The test
  `duplicate_derive_occurrences_have_distinct_generated_source_identity`
  verifies that two derive occurrences on one item do not collapse to one
  generated source identity.
- **[SYM-A2](#sym-a2) — Local detail-edit matrix.**
  `detail_edits_preserve_local_symbol_query_keys` verifies the top-level item
  family, `enum_detail_edits_preserve_variant_query_keys` verifies variants
  and constructors while reading the updated self-contained variant CST, and
  `moving_associated_items_preserves_symbol_query_keys` verifies associated
  functions, types, and constants under both trait and impl owners across
  insertion, removal, and reordering of siblings.
- **[SYM-A2](#sym-a2) — One impl-item producer.**
  `local_normalization_reads_one_keyed_value_without_impl_item_enumeration`
  verifies that focused normalization and aggregate impl-item enumeration use
  the same per-item producer query without making normalization depend on the
  aggregate item list; its edit trace also separates cheap producer
  revalidation from requested-value reuse when a sibling value changes.
- **[SYM-A5](#sym-a5) — Associated ownership.** The test
  `trait_items_have_stable_symbols_and_owner_identity` checks function, type,
  and const item ownership and verifies that an associated method reuses its
  trait's generic identities.
- **[SYM-A1](#sym-a1)/[SYM-A4](#sym-a4) — Resolved output.** The
  [function body example](../examples/function-body.md)
  inspects a resolved field owner in Typed IR, and the [oracle-checked method
  example](../examples/oracle-checked-method.md) inspects external definition
  identities in exact conformance output.

The implementation entry points are the anchored local, external, and wrapper
definitions above. Run the focused harness evidence with:

```bash
cargo test -p sage-test-harness moving_source_item_preserves_derive_expansion_identity
cargo test -p sage-test-harness trait_items_have_stable_symbols_and_owner_identity
cargo test -p sage-test-harness detail_edits_preserve_local_symbol_query_keys
cargo test -p sage-test-harness moving_associated_items_preserves_symbol_query_keys
```

### Current limitations

- Local tracked identities are based on the identity fields and creation
  structure used by the current parser/lowering queries. Evidence covers the
  per-kind detail-edit matrix, offset movement, and associated-item sibling
  insertion, deletion, and reordering, but not those structural edits for
  every top-level item kind required by SYM-A2.
- `SymExt` interns the carried `kind` alongside crate number and definition
  index even though the logical definition identity is the latter pair; the
  representation therefore does not yet make the SYM-A3 split exact.
- Current tests exercise several wrapper conversions and external queries but
  do not yet provide the complete compile-time conversion matrix required by
  SYM-A4 or the external operation matrix required by SYM-A3.
- Current resolved-output examples show that definitions are recorded by
  symbol, but do not establish SYM-A1's distinct-path/same-symbol matrix.
- The associated-ownership test establishes owner and generic identity but
  does not yet include SYM-A5's sibling-body dependency assertion.
- Some fine-grained identities are owner-relative rather than global symbols:
  fields use owner plus index and locals use body-local IDs. The architecture
  does not yet promise stable field/local identity across structural edits.
- Unsupported external definition kinds can be retained as raw metadata
  children but cannot all be converted to a typed `SymbolData` variant.

### Related roadmap slices

- The [Semantic Inspector](../validation/semantic-inspector.md) exposes stable
  display paths and readable identity-bearing semantic output.
- The [Build-Out Roadmap](../../implementation/roadmap.md) tracks vertical
  slices that extend the set of symbol kinds and semantic queries exercised in
  completed bodies.
