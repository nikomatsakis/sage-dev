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

This is the incremental-boundary consequence of
[Salsa for incremental computation](../../../rfds/architectural-foundations/README.md#salsa-for-incremental-computation) and
[Cross-item dependencies stop at semantic interfaces](../../../rfds/architectural-foundations/README.md#cross-item-dependencies-stop-at-semantic-interfaces).

{spec}`crates.sage_ir.crate.semantic_query_boundaries`

Verification scenarios: Representative phase and subsystem queries state
their key, result, permitted dependencies, and forbidden dependencies;
focused traces prove that requesting one product does not execute unrelated
products or temporary substeps as separate public queries.

## Stable identity, equality, and backdating

Stable identity answers “is this the same definition?” independently of
whether a tracked field changed. [Symbols](symbols-guide.md) define that boundary.
Value equality answers whether a recomputed semantic result changed. A query
may reexecute after an observed input changes but produce an equal output;
Salsa backdating then prevents that equal value from propagating to its
dependents.

`Stashed<T>` supports this by comparing deterministic content rather than
arena addresses. Relative spans keep offset-only movement out of semantic CST
hashes. These mechanisms do not automatically make a query fine-grained: a
query which reads a whole map or module vector may still reexecute, but an
equal keyed result can stop the invalidation there.

{spec}`crates.sage_ir.crate.reexecution_and_invalidation`

Verification scenarios: Edit tests separately assert which queries may
reexecute, whether the keyed value changed, and which downstream consumers
must remain reused for unchanged, relevant, and unrelated edits.

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

{spec}`crates.sage_ir.crate.incremental_evidence`

Verification scenarios: Cold, warm, relevant-edit, and unrelated-edit
tests normalize stable query keys and metadata operations, assert execution
versus reuse, and remain insensitive to permitted ordering of independent
work.

## Code map

| Path | Responsibility |
|---|---|
| `db.rs` | Salsa database, source-file registry, query-execution log |
| `source.rs` | mutable source inputs |
| `local_syms/` | stable tracked identities and symbol-keyed queries |
| `sage-stash` | deterministic content-addressed storage and `Stashed` equality/update |
| `span.rs` | relative and generated provenance that avoids offset-only semantic churn |
| `sage-test-harness` | cold/warm and edit/query trace fixtures |

## Implementation and test evidence

The generated [anchor pages](../../anchor/crates/sage_ir/crate.html) report implementation entries,
annotated tests, explicit TODO/N/A gaps, and saved claim/test reviews. These
associations do not themselves establish that a test ran.

The [earlier coverage snapshot](../../../rfds/implementation-sequencing/coverage-history.md#incrementality-and-query-boundaries)
preserves the former capability descriptions, edit experiments, and limitations
as historical evidence. Current coverage comes from the generated entries;
implementation plans and decision rationale belong in the relevant RFDs.
