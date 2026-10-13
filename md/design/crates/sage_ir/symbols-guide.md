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
[Symbols form the uniform semantic identity family](../../../rfds/architectural-foundations/README.md#symbols-form-the-uniform-semantic-identity-family).

The specification is {spec}`crates.sage_ir.symbols.definition_identity`.

Review cases: Resolution and completed-output tests show that distinct paths to one definition produce the same symbol, while equal spellings in different scopes or namespaces remain distinct.

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

The specification is {spec}`crates.sage_ir.symbols.local_identity`.

Review cases: Per-kind edit matrices cover detail-only changes, sibling insertion and reordering, and genuine identity changes. They assert both symbol continuity and the execution or reuse of representative signature, member, and body queries.

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
[External providers supply facts, not Sage semantic answers](../../../rfds/architectural-foundations/README.md#external-providers-supply-facts-not-sage-semantic-answers).

The specification is {spec}`crates.sage_ir.symbols.external_identity`, {spec}`crates.sage_ir.symbols.external_fact_queries`.

Review cases: External-metadata traces identify the exact definition-keyed facts read by representative signature, body, proof, and normalization requests, reject unrelated fact reads, and show Sage solver queries executing on the imported values.

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

The specification is {spec}`crates.sage_ir.symbols.kind_wrappers`.

Review cases: API and compile-time tests cover lossless conversion from each supported kind into `Symbol`, checked recovery of the right kind, rejection of the wrong kind, and local/external dispatch through the corresponding kind wrapper.

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

The specification is {spec}`crates.sage_ir.symbols.associated_identity`, {spec}`crates.sage_ir.types.binder.owner_parameters`, {spec}`crates.sage_ir.symbols.associated_interfaces`.

Review cases: Trait and impl fixtures cover functions, types, and constants; compare owner and generic identities across repeated and edited requests; and use query traces to exclude sibling-body dependencies.

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

## Implementation and test evidence

The generated [anchor pages](../../anchor/crates/sage_ir/symbols.html) report implementation entries,
annotated tests, explicit TODO/N/A gaps, and saved claim/test reviews. These
associations do not themselves establish that a test ran.

The [earlier coverage snapshot](../../../rfds/implementation-sequencing/coverage-history.md#symbols-and-semantic-identity)
preserves the former capability descriptions, edit experiments, and limitations
as historical evidence. Current coverage comes from the generated entries;
implementation plans and decision rationale belong in the relevant RFDs.
