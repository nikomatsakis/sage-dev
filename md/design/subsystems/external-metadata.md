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

Missing or incomplete metadata cannot justify a proof, negative result, or
fully checked interface. Callers preserve that distinction instead of
inventing defaults or asking rustc to answer the Sage goal.

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

External module/name lookup, traits, associated items, function methods, ADT
generics/defaults/predicates, inherent method candidates, relevant trait impls,
impl headers, associated type values, selected structural ADT properties, and
macro expansion facts support the completed mini-redis slices.

### Implemented capabilities and evidence

- `external_clone_items_and_method_signature_are_typed_metadata` verifies
  separate trait-item enumeration and selected method signature lowering.
- `clone_method_body_has_a_narrow_reusable_semantic_query_trace` pins the
  external calls needed by `DbDropGuard::db`, rejects callee-body reads, and
  verifies warm reuse.
- `local_normalization_reads_one_keyed_value_without_impl_item_enumeration`
  verifies the corresponding local associated-value boundary; the exact same
  separation is present in the external API.
- `external_adt_default_and_predicate_have_one_narrow_reusable_dependency`
  pins one ADT signature read and warm reuse.

### Current limitations

- The raw model covers only the Rust definition kinds and semantic details
  required by implemented slices.
- Unsupported defaults, const identities, predicates, receivers, or impl
  headers become unavailable/ineligible metadata rather than full Sage types.
- External module children are broader than several desired semantic lookup
  keys; finer queries should be introduced when evidence shows unnecessary
  invalidation.
- The production frontend remains coupled to rustc's unstable private APIs
  through its driver implementation, though `TcxDb` isolates that coupling.

### Related roadmap slices

- [Trait-partitioned impl
  discovery](../../implementation/roadmap.md#planned-slice-trait-partitioned-impl-discovery)
  aligns the local boundary with the existing external trait/head key.
- [Semantic inspector and persistent edit
  testing](../../implementation/roadmap.md#planned-slice-semantic-inspector-and-persistent-edit-testing)
  will present metadata calls alongside Salsa execution traces.
- [Shutdown::recv](../../implementation/roadmap.md#next-application-slice-shutdownrecv)
  will add only the metadata forms required by that vertical slice.
