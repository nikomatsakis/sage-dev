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

<a id="body-a1"></a>
> **BODY-A1 — A completed body is an elaborated semantic tree.** Successful
> output contains final types and semantic identities and materializes all
> type-directed operations used by the source. Method syntax, unresolved names,
> inference variables, pending obligations, and adjustment recipes do not
> survive in `CheckedBody`; [D11](../decisions.md#d11-completed-bodies-are-elaborated-typed-trees)
> owns the cross-cutting representation decision.
>
> **Required verification:** Structural Typed-IR snapshots cover paths, fields,
> calls, generic substitutions, borrows, dereferences, coercions, and recovery;
> completion checks reject every unresolved or live internal form; and selected
> slices match the independent rustc oracle by exact text.

<a id="body-a2"></a>
> **BODY-A2 — Body checking depends on interfaces, never other bodies.** The
> body query reads only its own CST and signature plus the name-resolution,
> interface, metadata, candidate, and solver facts demanded by that body. A
> call may read its callee's signature but cannot read the callee body. This is
> the body-phase consequence of [D15](../decisions.md#d15-cross-item-dependencies-stop-at-semantic-interfaces).
>
> **Required verification:** Cold traces whitelist the semantic facts used by a
> representative body and reject every callee-body or unrelated-interface
> read; an unchanged warm request performs no work; and persistent edits
> distinguish relevant interface changes from unrelated body changes.

## Entry points

`LocalFnSym::body(db)` is the tracked phase boundary:

```{anchor}
example_fn_body
```

The method imports the signature, installs generic/value bindings and the
solver environment, asynchronously checks the body expression, relates it to
the declared return type, and finalizes inference and obligations. Completion
then resolves and structurally copies the working tree into a fresh output
stash before freezing the result.

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
runs a mandatory terminal obligation pass, and asserts quiescence. A final
copy consults the settled egraph for every type edge and constructs
`CheckedBody` in a separate append-only stash; the transient inference stash
is never rewritten into an apparent result.

```{anchor}
finalize_typed_body_into_fresh_stash
```

<a id="body-a3"></a>
> **BODY-A3 — Speculation is isolated and completion is quiescent.** Candidate
> matching and other speculative relations occur in child inference versions;
> failure discards their equalities, wakeups, and staged obligations, while a
> successful operation commits them atomically. The public result is frozen
> only after fallback and terminal obligation processing leave no live work.
> [D6](../decisions.md#d6-versioned-egraph-children-are-inference-transactions)
> defines the shared transaction rules.
>
> **Required verification:** Rejected-candidate tests observe no leaked generic
> bindings, wakes, or obligations; accepted-candidate tests observe one atomic
> commit; unresolved residual obligations become diagnostics; and successful
> completion asserts an empty runtime, wake queue, obligation registry, and
> root transaction set.

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

This terminal treatment is the body-phase application of
[D16](../decisions.md#d16-incompleteness-is-an-explicit-terminal-outcome).

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

- **[BODY-A1](#body-a1) — Simple field inference.** The [function-body
  walkthrough](../examples/function-body.md) maps the input to a resolved field
  owner and final type.
- **[BODY-A1](#body-a1) — Elaborated external trait call.**
  `clone_method_call_is_elaborated_to_a_resolved_trait_call` directly inspects
  the completed Sage tree.
- **[BODY-A1](#body-a1) — Exact conformance.** The [oracle-checked method
  walkthrough](../examples/oracle-checked-method.md) links the structural Sage
  assertion, independent emitters, exact shared snapshot, and byte identity.
- **[BODY-A2](#body-a2) — Narrow reuse.**
  `clone_method_body_has_a_narrow_reusable_semantic_query_trace` checks the
  exact external interface reads, rejects all callee-body reads, and verifies
  that an unchanged second request executes neither the body query nor the
  metadata reads.
- **[BODY-A2](#body-a2) — Current edit boundary.**
  `unrelated_body_edit_exposes_current_body_invalidation` edits a different
  function body and records that this body currently reexecutes, while its
  cached callee-interface metadata queries do not. The test does not assert
  whether module or derive-discovery queries reexecute.
- **[BODY-A3](#body-a3) — Transaction rollback.**
  `external_method_mismatch_discards_partial_generic_bindings` rejects a call
  after its first generic match and verifies that neither the binding nor the
  root semantic revision escapes the child transaction.
- **[BODY-A3](#body-a3) — Terminal obligations.**
  `generic_function_use_checks_its_instantiated_bounds`,
  `equivalent_obligations_deduplicate_after_inference`, and
  `struct_bounds_converge_after_field_inference` cover diagnostic publication,
  canonical deduplication, and an obligation resolved after type inference.

Run the review evidence with:

```bash
cargo test -p sage-test-harness clone_method_call_is_elaborated_to_a_resolved_trait_call
cargo test -p sage-test-harness clone_method_body_has_a_narrow_reusable_semantic_query_trace
cargo test -p sage-test-harness unrelated_body_edit_exposes_current_body_invalidation
cargo test -p sage-ir external_method_mismatch_discards_partial_generic_bindings
cargo test -p sage-test-harness trait_obligation_tests
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
- BODY-A3 has focused rollback and obligation-lifecycle evidence, but its full
  accepted-commit, wake-publication, and quiescence verification is not yet
  assembled as one review packet.
- An unrelated body edit in the same source file currently reexecutes the
  requested body query. The focused edit test above pins this gap from the
  destination while also verifying that cached callee interfaces remain
  reused. Expansion/discovery execution after that edit is not yet pinned by
  this evidence.

### Related roadmap slices

- [Solver recursion, scheduling, and monotone
  progress](../../implementation/roadmap.md#planned-slice-solver-recursion-scheduling-and-monotone-progress)
  changes how obligations are searched without changing the successful Typed
  IR contract.
- [Shutdown::recv](../../implementation/roadmap.md#next-application-slice-shutdownrecv)
  is the next body-coverage target.
- [Semantic inspector and persistent edit
  testing](../../implementation/roadmap.md#implemented-slice-semantic-inspector-and-persistent-edit-testing)
  will expose readable bodies and structured query traces.
