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

<a id="meta-a1"></a>
> **META-A1 — External metadata supplies owned facts, never Sage decisions.**
> The provider may state facts authored by a reachable dependency or the
> compiler, but it cannot resolve local syntax, select a Sage candidate, prove
> a Sage goal, normalize an alias, or type-check a Sage body. This is the typed
> metadata boundary established by
> [D18](../decisions.md#d18-external-providers-supply-facts-not-sage-semantic-answers).
>
> **Required verification:** The typed provider interface has no proof,
> normalization, method-selection, local-resolution, or checked-body operation;
> end-to-end tests demonstrate that Sage candidate and solver queries execute
> after raw metadata is returned rather than accepting a provider verdict.

Missing or incomplete metadata cannot justify a proof, negative result, or
fully checked interface. Callers preserve that distinction instead of
inventing defaults or asking rustc to answer the Sage goal.

<a id="meta-a2"></a>
> **META-A2 — Missing metadata remains explicit incompleteness.** A missing,
> unsupported, or conservatively incomplete external fact is never replaced by
> an inferred default, a successful interface, or exhaustive logical failure.
> This is the metadata consequence of
> [D16](../decisions.md#d16-incompleteness-is-an-explicit-terminal-outcome).
>
> **Required verification:** Fixture providers return missing, unsupported,
> and incomplete values at every semantic boundary; callers preserve the
> distinction in diagnostics and completeness, and solver tests prove none of
> those values can produce a false `Yes` or `No`.

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

<a id="meta-a3"></a>
> **META-A3 — External demand is decomposed by semantic product.** Identity
> enumeration, signatures, impl headers, and individual associated values are
> separate tracked products. Consumers enumerate identities before requesting
> selected details, and no external function body is available to checking.
> This is the metadata consequence of
> [D15](../decisions.md#d15-cross-item-dependencies-stop-at-semantic-interfaces).
>
> **Required verification:** Query traces for method lookup, trait proof, and
> normalization show identity enumeration followed only by selected signatures,
> applicable headers, or the requested associated value; they exclude sibling
> items and values, unrelated impls, and all callee/impl bodies.

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

<a id="meta-a4"></a>
> **META-A4 — Raw and lowered metadata share the same narrow reuse key.** Each
> lowered semantic fact observes only its corresponding owned provider fact.
> Repeating an unchanged request reuses the lowered value without another raw
> request, and changing an unrelated provider fact does not invalidate it.
>
> **Required verification:** Cold/warm traces pair every lowered query with its
> exact raw operation; the warm trace contains neither execution, relevant edits
> invalidate both layers, and unrelated-key edits preserve or backdate the
> lowered result.

## Code map

| Path | Responsibility |
|---|---|
| `tcx/mod.rs` | owned raw metadata model and `TcxDb` interface |
| `tcx/proxy.rs` | channel-based request forwarding for a rustc-owned session |
| `external_syms.rs` | tracked lowering into Sage symbols and semantic types |
| `symbol/mod.rs` | `SymExt` external definition identity |
| `sage-oracle-harness` | combined rustc/Sage session and exact conformance harness |

## Current status

### Current frontier

External module/name lookup, traits, associated items, cached well-known
operator trait and method identities, function methods, ADT
generics/defaults/predicates, inherent method candidates, relevant trait impls,
impl headers, associated type values, selected structural ADT properties, and
macro expansion facts support the implemented slices. Relevant-impl lookup
also represents the coarse local-nominal self head and reports explicit binary
operator impl sets as complete where rustc has no separate built-in candidate
source. Float self heads retain their width: represented `f32` and `f64`
queries can therefore exclude rustc's explicit `f16` and `f128` impls without
trying to lower their unsupported signatures or weakening completeness.

### Implemented capabilities and evidence

| Anchor | State | Implemented claim and evidence |
|---|---|---|
| [META-A1](#meta-a1) | Partial | The `architecture_external_metadata_interface` source anchor exposes typed owned facts rather than solver verdicts; `external_clone_contract_proves_the_derived_impl` exercises Sage proof after importing external data. A dedicated interface-surface audit is not yet checked in. |
| [META-A2](#meta-a2) | Partial | `external_adt_missing_required_argument_is_not_inferred` and `unsupported_external_default_is_metadata_unavailable_not_an_arity_error` preserve two unavailable-metadata cases without inventing defaults; the full boundary matrix is not present. |
| [META-A3](#meta-a3) | Partial | `external_clone_items_and_method_signature_are_typed_metadata` verifies separate trait-item enumeration and selected method-signature lowering. `operator_identity_queries_are_layered_cached_and_narrow` verifies that operator-method identity reads associated items and names without loading trait or function signatures or impls. `primitive_operators_use_core_impls_and_resolved_calls` then traces the independently demanded method signature, relevant impl identities, matching headers, and associated output values for ground `Add` and `Shl` bodies. `represented_float_operator_impls_resolve` checks the separate `Float32` and `Float64` keys, and `local_operator_impl_can_choose_distinct_rhs_and_output` observes the `LocalAdt` partition while Sage selects a local impl. `clone_method_body_has_a_narrow_reusable_semantic_query_trace` rejects callee-body reads, and `local_normalization_reads_one_keyed_value_without_impl_item_enumeration` covers keyed local associated-value demand. The full external product/isolation matrix is not assembled. |
| [META-A4](#meta-a4) | Implemented for selected products | `clone_method_body_has_a_narrow_reusable_semantic_query_trace` pins cold calls and warm reuse for the method slice. `external_adt_default_and_predicate_have_one_narrow_reusable_dependency` pins one ADT-signature read and warm reuse. `operator_identity_queries_are_layered_cached_and_narrow` separately pins layered trait/method lookup and a metadata-free warm repeat. `primitive_operators_use_core_impls_and_resolved_calls` extends cold/warm reuse evidence through the operator method-signature, impl-header, and associated-value products. |

### Current limitations

- The raw model covers only the Rust definition kinds and semantic details
  required by implemented slices.
- Unsupported defaults, const identities, predicates, receivers, or impl
  headers become unavailable/ineligible metadata rather than full Sage types.
- META-A1 lacks a mechanical interface-surface check, META-A2 lacks a complete
  unavailable-metadata matrix, and META-A3 lacks a focused external
  associated-value trace.
- External module children are broader than several desired semantic lookup
  keys; finer queries should be introduced when evidence shows unnecessary
  invalidation.
- `LocalAdt` is intentionally a coarse safe partition. Finer local nominal
  self-head keys and their full indexed-versus-exhaustive comparison remain
  part of trait-partitioned impl-discovery work.
- Float widths are intentionally finer than the other primitive families.
  Sage represents `f32` and `f64`, while the metadata provider classifies
  `f16` and `f128` as distinct rigid heads so their unsupported headers are
  provably irrelevant to represented float queries.
- The production frontend remains coupled to rustc's unstable private APIs
  through its driver implementation, though `TcxDb` isolates that coupling.

### Related roadmap slices

- [Trait-partitioned impl
  discovery](../../implementation/roadmap.md#planned-slice-trait-partitioned-impl-discovery)
  aligns the local boundary with the existing external trait/head key.
- [Semantic inspector and persistent edit
  testing](../../implementation/roadmap.md#implemented-slice-semantic-inspector-and-persistent-edit-testing)
  presents metadata calls alongside Salsa execution traces.
- [Shutdown::recv](../../implementation/roadmap.md#next-application-slice-shutdownrecv)
  will add only the metadata forms required by that vertical slice.
- [Numeric inference and operator
  soundness](../../implementation/roadmap.md#next-correctness-slice-numeric-inference-and-operator-soundness)
  uses cached well-known-item queries over ordinary path and associated-item
  resolution. Its first trait-first vertical slice traces ground primitive
  `core` impl discovery and local-nominal partitioning through this boundary;
  the generic and closure matrices remain open.
