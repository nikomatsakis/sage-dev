# External Metadata

External metadata gives Sage authoritative facts about definitions in
reachable dependency crates. It is a typed boundary, not a delegation of Sage
semantics to rustc: rustc may report an external signature, impl identity, or
associated value, but Sage resolves local source, performs inference, selects
candidates, and solves trait/normalization goals itself.

## Contract

`TcxDb` exposes owned, `'static` raw values identified by crate number and
definition index. It has no Salsa database lifetime and can therefore be
implemented directly by a rustc-backed driver or through the channel-based
proxy used by combined oracle checking:

```{anchor}
architecture_external_metadata_interface
```

Each operation returns one semantically coherent fact at the narrowest stable
key. Examples include one module's children, one trait signature, one set of
associated-item identities, one function signature, relevant impl identities
for a fixed trait and optional rigid self head, one impl header, and one
associated type value.

{spec}`crates.sage_ir.metadata.provider_authority`

Verification intent: The typed provider interface has no proof,
normalization, method-selection, local-resolution, or checked-body operation;
end-to-end tests demonstrate that Sage candidate and solver queries execute
after raw metadata is returned rather than accepting a provider verdict.

Missing or incomplete metadata cannot justify a proof, negative result, or
fully checked interface. Callers preserve that distinction instead of
inventing defaults or asking rustc to answer the Sage goal.

{spec}`crates.sage_ir.metadata.explicit_incompleteness`

Verification intent: Fixture providers return missing, unsupported,
and incomplete values at every semantic boundary; callers preserve the
distinction in diagnostics and completeness, and solver tests prove none of
those values can produce a false `Yes` or `No`.

## Tracked lowering boundary

`external_syms.rs` wraps raw operations in Salsa queries and lowers them to
Sage symbols, binders, types, and stashes. For example, a trait signature is
loaded and lowered independently of its associated items:

```{anchor}
architecture_external_trait_signature_query
```

This split is architectural. Method name lookup may enumerate associated-item
identities, then load only the selected function signature. Trait candidate
discovery loads relevant impl identities, then matching candidate headers.
Associated-type normalization requests one associated value only after its
impl header matches. No operation reads an external function body.
Operator checking obtains its well-known trait identities through ordinary
Sage path resolution. Cached queries such as the addition-trait query resolve
fixed paths such as `core::ops::Add`, then associated-item lookup finds the
method. Those lookups consume this subsystem's ordinary external module and
item facts; the raw metadata provider has no operator-specific identity query.
Ordinary primitive operator impls defined in `core` use this same relevant-impl
path; the metadata boundary does not replace them with a compiler-authored
operator table or proof result. For the represented binary-operator traits,
the rustc-backed provider can mark the explicit impl set complete because
rustc has no additional built-in candidate source for those traits. That flag
is a candidate-source fact, not a proof verdict; Sage still filters headers and
solves the goal.

When the known `Self` is a nominal type declared in the crate Sage is checking,
the relevant-impl request uses the coarse `LocalAdt` head. An upstream impl
with a different rigid nominal head cannot match that local identity and may be
excluded, but blanket and otherwise unclassifiable upstream impls remain in
the result. This partition lets local user-defined operator impls use an
exhaustive external source without pretending that external nominal identities
could equal the local type.

{spec}`crates.sage_ir.metadata.product_boundaries`

Verification intent: Query traces for method lookup, trait proof, and
normalization show identity enumeration followed only by selected signatures,
applicable headers, or the requested associated value; they exclude sibling
items and values, unrelated impls, and all callee/impl bodies.

## Permitted and forbidden authority

The metadata provider may authoritatively supply facts owned by an external
crate, including compiler-built-in identities and structural traits which are
not enumerable as source impls. It may also conservatively report that a set
is incomplete.

It must not:

- resolve a Sage local path;
- select a Sage method candidate;
- prove a Sage trait obligation;
- normalize an alias on Sage's behalf;
- erase unsupported metadata into a convenient semantic answer; or
- provide callee bodies to body checking.

## Incremental boundary

Every lowered fact is keyed independently. A request for
`associated_items(Trait)` does not depend on every item signature;
`external_relevant_impls(Trait, Head)` does not depend on unrelated traits;
`external_associated_type_value(Impl, Item)` does not enumerate sibling values.

The database query log and tracing test providers make those dependencies
observable. An unchanged warm semantic request must reuse the lowered result
without issuing the raw metadata operation again.

{spec}`crates.sage_ir.metadata.keyed_reuse`

Verification intent: Cold/warm traces pair every lowered query with its
exact raw operation; the warm trace contains neither execution, relevant edits
invalidate both layers, and unrelated-key edits preserve or backdate the
lowered result.

## Code map

| Path | Responsibility |
|---|---|
| `tcx/mod.rs` | owned raw metadata model and `TcxDb` interface |
| `tcx/proxy.rs` | channel-based request forwarding for a rustc-owned session |
| `external_syms.rs` | tracked lowering into Sage symbols and semantic types |
| `symbol/mod.rs` | `SymExt` external definition identity |
| `sage-oracle-harness` | combined rustc/Sage session and exact conformance harness |

## Implementation and test evidence

The generated [anchor pages](../../anchor/crates/sage_ir/metadata.html) report implementation entries,
annotated tests, explicit TODO/N/A gaps, and saved claim/test reviews. These
associations do not themselves establish that a test ran.

The [earlier coverage snapshot](../../../rfds/implementation-sequencing/coverage-history.md#external-metadata)
preserves the former capability descriptions, edit experiments, and limitations
as historical evidence. Current coverage comes from the generated entries;
implementation plans and decision rationale belong in the relevant RFDs.
