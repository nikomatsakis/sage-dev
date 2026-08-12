# Incrementality and Query Boundaries

Sage uses Salsa to compute semantic results on demand and reuse them across
requests and source revisions. Incrementality is part of the architecture:
the identity fields, tracked fields, query keys, and facts read by a query
determine which edits cause semantic work to execute again.

## Semantic boundaries

Sage uses four complementary forms:

- `#[salsa::input]` for mutable external inputs such as source text;
- `#[salsa::tracked]` structs for stable local semantic identities whose
  details may change across revisions;
- interned values for structural identities such as names and external
  handles; and
- tracked functions for reproducible semantic operations keyed by symbols,
  modules, traits, or canonical goals.

A useful query returns the narrowest stable semantic value a consumer needs.
Examples are one module's expanded direct items, one item signature, one body,
one trait-keyed impl candidate set, and one canonical solver result. Mutable
inference steps and method candidates remain inside the body query because
they are not stable public facts.

## Stable identity, equality, and backdating

Stable identity answers “is this the same definition?” independently of
whether a tracked field changed. [Symbols](./symbols.md) define that boundary.
Value equality answers whether a recomputed semantic result changed. A query
may reexecute after an observed input changes but produce an equal output;
Salsa backdating then prevents that equal value from propagating to its
dependents.

`Stashed<T>` supports this by comparing deterministic content rather than
arena addresses. Relative spans keep offset-only movement out of semantic CST
hashes. These mechanisms do not automatically make a query fine-grained: a
query which reads a whole map or module vector may still reexecute, but an
equal keyed result can stop the invalidation there.

## Designing a query firewall

For every query, document:

1. its semantic key and result;
2. facts it may read;
3. facts it must not read;
4. relevant edits which must change the result; and
5. unrelated edits which may cause a cheap lookup to rerun but must not reach
   expensive consumers.

The planned local impl index is the canonical example. A crate-owned tracked
struct may privately hold a changed map. Its `for_trait(T)` lookup may rerun
after an unrelated bucket edit, but if the returned bucket for `T` is equal,
Salsa must backdate it before impl header lowering, solver evaluation,
normalization, or body checking reexecutes.

## Inspecting execution

The test database records Salsa `WillExecute` events, so a test can distinguish
query execution from merely requesting a memoized result:

```{anchor}
architecture_query_execution_log
```

External test providers separately record raw `TcxDb` calls. A useful
incremental test clears setup events, performs one semantic request, records a
cold trace, repeats it for a warm trace, then applies a relevant or unrelated
edit and records the next trace. Assertions use semantic query names and keys,
not incidental wall-clock timing.

Query traces are evidence, not part of the compiler's logical output. Internal
scheduling may reorder independent events. Normalize event sets or multisets
unless ordering/count is itself the contract.

## Code map

| Path | Responsibility |
|---|---|
| `db.rs` | Salsa database, source-file registry, query-execution log |
| `source.rs` | mutable source inputs |
| `local_syms/` | stable tracked identities and symbol-keyed queries |
| `sage-stash` | deterministic content-addressed storage and `Stashed` equality/update |
| `span.rs` | relative and generated provenance that avoids offset-only semantic churn |
| `sage-test-harness` | cold/warm and edit/query trace fixtures |

## Current status

### Current frontier

Module, signature, body, external metadata, and solver operations are memoized
at semantic keys. Focused tests inspect warm reuse, selected external facts,
generated identity across movement, and narrow associated-value access.

### Implemented capabilities and evidence

- `clone_method_body_has_a_narrow_reusable_semantic_query_trace` proves warm
  body reuse and rejects external callee-body reads.
- `external_adt_default_and_predicate_have_one_narrow_reusable_dependency`
  proves one keyed external ADT dependency and warm reuse.
- `local_normalization_reads_one_keyed_value_without_impl_item_enumeration`
  proves normalization reads one associated value and not the item list.
- `moving_source_item_preserves_derive_expansion_identity` distinguishes
  stable generated identity from its changing source coordinate.
- `unrelated_body_edit_exposes_current_body_invalidation` is negative evidence:
  it pins a coarse same-file invalidation that the destination should remove.

### Current limitations

- The local trait impl query is trait-keyed but still scans the complete local
  expanded-module vector; the private stable index/backdating firewall is not
  implemented.
- Same-file unrelated body edits currently reexecute a requested body and
  module/derive discovery, even though selected callee-interface metadata is
  reused.
- Query logs are debug strings and tests rather than a structured persistent
  event schema.
- Edit matrices cover selected semantic paths, not every symbol kind and
  query boundary.

### Related roadmap slices

- [Semantic inspector and persistent edit
  testing](../../implementation/roadmap.md#planned-slice-semantic-inspector-and-persistent-edit-testing)
  will provide a persistent database, structured traces, and edit commands.
- [Trait-partitioned impl
  discovery](../../implementation/roadmap.md#planned-slice-trait-partitioned-impl-discovery)
  will implement the local index firewall and its invalidation matrix.
