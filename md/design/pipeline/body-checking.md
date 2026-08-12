# Body Checking and Typed-IR Elaboration

Body checking resolves one function body, infers its expression and pattern
types, proves required obligations, and returns completed tree-structured
[Typed IR](../typed-ir.md). Source conveniences are consumed: a successful
result records resolved definitions and explicit semantic operations rather
than adjustment side tables.

## Contract

### Granularity

The unit of demand and memoization is one local function or associated-function
body. Temporary inference versions, method candidates, and partially checked
expressions are internal to that query.

### Input

The direct key is `LocalFnSym`. The phase reads the function's body CST and
checked signature, referenced item interfaces, name-resolution results,
external metadata, and canonical trait-solver results required by this body.
It must not read callee bodies.

### Output

`CheckedBody` contains a stash-owned `TyBody` plus diagnostics. On successful
completion every expression and pattern has a final type, paths and fields
refer to semantic identities, method calls are elaborated to resolved call
targets, required dereferences/borrows/coercions are explicit, and no live
inference or obligation work remains.

The output remains tree-structured, between rustc THIR and MIR in intent. It
does not contain control-flow lowering or borrow-checking results.

### Guarantees

Downstream consumers may traverse one self-contained typed tree without
consulting adjustment side tables. Generic substitutions and static trait
dispatch are attached to call targets. Diagnostics accompany recovery nodes;
a body with errors is not evidence of successful type checking.

## Entry points

`LocalFnSym::body(db)` is the tracked phase boundary:

```{anchor}
example_fn_body
```

The method imports the signature, installs generic/value bindings and the
solver environment, asynchronously checks the body expression, relates it to
the declared return type, finalizes inference and obligations, resolves all
types, and freezes the result.

## Construction

An `InferCtx` owns the body output stash, egraph, obligation registry, wake
queues, and diagnostic sink. A `Scope` owns lexical ribs and module
resolution. Expression and pattern checking proceed asynchronously so method
and trait candidates can perform solver work without exposing partial public
results.

Inference operations use child versions transactionally: speculative
equalities collapse into their parent only after the whole operation succeeds.
Trait operations cross a canonical Salsa boundary and import responses back
into the caller. Finalization applies fallback, retries woken obligations,
runs a mandatory terminal obligation pass, and asserts quiescence before
constructing `CheckedBody`.

Elaboration happens while checking. For example, a selected trait method call
becomes a `ResolvedCall` whose argument contains explicit reference and
dereference nodes:

```{anchor}
example_elaborate_trait_method_call
```

## Failure and terminal incompleteness

User type errors produce diagnostics and error-typed recovery nodes. An
unsupported expression, incomplete method-provider set, ambiguous trait
answer, unavailable external fact, or inference/solver resource limit likewise
prevents the successful-output guarantee. These are terminal outcomes for the
body query, not pending background work.

Non-ground trait goals may soundly terminate as ambiguous even when a proof
exists. Ground goals must be sound and complete modulo documented overflow and
term-size limits. Body checking turns an unresolved required obligation into a
diagnostic before it returns.

## Incremental dependencies

The key is the function symbol. The query may read its body and signature,
interfaces of referenced definitions, relevant name-resolution and candidate
queries, and canonical solver results. It must not read uncalled/unreferenced
interfaces merely because they share a crate, and it must never read a callee
body.

An edit to a referenced signature or relevant impl set should invalidate the
body. An unrelated body edit should not. Repeating the same request in an
unchanged database must reuse the result without rereading external metadata.

## Worked example

For:

```rust
fn first<T>(pair: Pair<T>) -> T {
    pair.first
}
```

the body query imports `Pair<T_first>`, binds `pair`, resolves the field owner
to the `Pair` symbol, substitutes the struct's parameter with `T_first`, and
returns a field expression typed `T_first`. The [function-body
walkthrough](../examples/function-body.md) follows each boundary.

The more demanding [oracle-checked method
body](../examples/oracle-checked-method.md) shows `self.db.clone()` elaborated
to static `Clone::clone` dispatch and compared exactly with rustc output.

## Code map

| Path | Responsibility |
|---|---|
| `local_syms/fns.rs` | body query orchestration |
| `check/infer_ctx.rs` and `check/infer/` | inference state, transactions, obligations, finalization |
| `check/expr.rs` | expression/pattern checking and elaboration |
| `check/method.rs` | method candidate discovery, selection, and instantiation |
| `check/solve/` | canonical trait proof and alias normalization |
| `tytree/` | completed typed body representation |

## Current status

### Current frontier

Body checking and elaboration pass exact oracle comparison for the pinned
mini-redis `DbDropGuard::db` and `Parse::next` bodies. Those slices cover local
field access, external trait dispatch, explicit adjustments, generic
obligations, and associated-type normalization.

### Implemented capabilities and evidence

- **Simple field inference.** The [function-body
  walkthrough](../examples/function-body.md) maps the input to a resolved field
  owner and final type.
- **Elaborated external trait call.**
  `clone_method_call_is_elaborated_to_a_resolved_trait_call` directly inspects
  the completed Sage tree.
- **Exact conformance.** The [oracle-checked method
  walkthrough](../examples/oracle-checked-method.md) links the structural Sage
  assertion, independent emitters, exact shared snapshot, and byte identity.
- **Narrow reuse.**
  `clone_method_body_has_a_narrow_reusable_semantic_query_trace` checks the
  exact external interface reads, rejects all callee-body reads, and verifies
  that an unchanged second request executes neither the body query nor the
  metadata reads.
- **Current edit boundary.**
  `unrelated_body_edit_exposes_current_body_invalidation` edits a different
  function body and records that this body currently reexecutes, while its
  cached callee-interface metadata queries do not. Module/derive discovery
  metadata currently reexecutes with the containing module.

Run the review evidence with:

```bash
cargo test -p sage-test-harness clone_method_call_is_elaborated_to_a_resolved_trait_call
cargo test -p sage-test-harness clone_method_body_has_a_narrow_reusable_semantic_query_trace
cargo test -p sage-test-harness unrelated_body_edit_exposes_current_body_invalidation
cargo test -p sage-oracle-harness --test oracle_compare -- mini_redis/db_drop_guard.rs
```

### Current limitations

- Rust expression, pattern, coercion, and method-resolution coverage is
  incomplete; diagnostics or recovery nodes mark unsupported paths.
- Lifetimes are always `Lifetime::Dummy`, and borrow checking is deliberately
  omitted. Reference/dereference nodes are structural only.
- Trait solving is recursive with current cycle/resource semantics; planned
  fairness and progress publication are not part of this phase result.
- General local/inherent method resolution, dynamic dispatch, GATs,
  specialization, and codegen method/vtable selection remain outside the
  completed slices.
- Query-trace evidence is currently test-asserted text rather than a
  user-facing Semantic Inspector report.
- An unrelated body edit in the same source file currently reexecutes the
  requested body query. The focused edit test above pins this gap from the
  destination, while also verifying that cached callee interfaces remain
  reused; module/derive discovery metadata still reruns.

### Related roadmap slices

- [Solver recursion, scheduling, and monotone
  progress](../../implementation/roadmap.md#planned-slice-solver-recursion-scheduling-and-monotone-progress)
  changes how obligations are searched without changing the successful Typed
  IR contract.
- [Shutdown::recv](../../implementation/roadmap.md#next-application-slice-shutdownrecv)
  is the next body-coverage target.
- [Semantic inspector and persistent edit
  testing](../../implementation/roadmap.md#planned-slice-semantic-inspector-and-persistent-edit-testing)
  will expose readable bodies and structured query traces.
