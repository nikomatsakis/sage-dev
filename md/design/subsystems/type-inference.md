# Type Inference

Type inference is the body-local subsystem that relates partially known Sage
types, coordinates deferred obligations, and turns a checked expression tree
into a completed [Typed IR](../typed-ir.md). It is invoked by body checking;
its mutable intermediate state is not an incremental query boundary.

## Contract

One `InferCtx` belongs to one body query. It owns fresh inference variables,
their versioned equality/bound state, the target stash, diagnostic recovery,
the obligation registry, and the cooperative runtime. The public semantic
operations require equality, subtyping/coercion, instantiate signatures,
register proof or normalization obligations, and create isolated speculative
versions.

A successful completion has:

- no unresolved inference variables in the typed tree;
- no pending or live obligations;
- no live speculative branch or runtime task; and
- explicit elaboration for adjustments selected during checking.

Inference may defer a non-ground obligation and retry it after another
constraint narrows its inputs. It may not treat ambiguity as proof or let a
failed speculative candidate leak equalities, wakes, or obligations.

<a id="inf-a1"></a>
> **INF-A1 — One body owns inference through quiescence.** An `InferCtx` and all
> of its variables, versions, tasks, and obligations belong to one body query.
> A successful `CheckedBody` is published only after every type in its tree is
> resolved and no task, obligation, or speculative branch remains live.
>
> **Required verification:** Completion tests inspect a representative typed
> body for inference-free types and exercise `finish` with live variables,
> obligations, tasks, and branches to prove that each prevents successful
> publication; diagnostic recovery has separate expected output.

## Entry points and callers

The body query constructs one context. Its fields show the state kept inside
that body boundary:

```{anchor}
architecture_infer_context
```

Expression and pattern `check_with` methods allocate types and typed nodes
through the context. `require_eq`, `require_sub`, and `require_coerce` relate
types. `finalize` performs fallback and mandatory terminal obligation
discharge; `finish` asserts quiescence and resolves every type edge while
copying the working tree into a fresh append-only result stash. The working
stash remains unchanged, as required by [STASH-A5](../stash.md#stash-a5).

## Transactional construction

The equality engine is a versioned egraph. A child version sees its ancestors
but owns a sparse diff. Speculative work either discards the child or, after
the complete operation succeeds, atomically collapses an exclusive leaf child
into its direct parent. Published commit effects wake only variables whose
semantic bounds changed.

Trait queries do not borrow caller-local variables directly. The boundary
canonicalizes a goal and local environment, executes solver work, then checks
and imports the canonical response transactionally:

```{anchor}
example_apply_response_transaction
```

An obligation remains registered until its residual goals are discharged or
terminal checking reports a diagnostic. Output-producing normalization binds
the solver's returned `Type`; the caller's expected type is related only after
that result is imported.

<a id="inf-a2"></a>
> **INF-A2 — Speculation is an all-or-nothing inference transaction.** Every
> operation which may partially constrain inference executes in an isolated
> child version. Complete success may atomically collapse an exclusive leaf;
> failure or cancellation discards all of its equalities, bounds, universe
> changes, wakes, tasks, and staged obligations. This is the inference
> consequence of
> [D6](../decisions.md#d6-versioned-egraph-children-are-inference-transactions)
> and [D7](../decisions.md#d7-inference-variable-identities-are-unique-across-egraph-versions).
>
> **Required verification:** Failed occurs checks, universe checks,
> multi-entry response imports, candidate matches, and cancellations leave the
> parent semantically identical and publish no wake; concurrent siblings never
> observe one another's branch state.

<a id="inf-a3"></a>
> **INF-A3 — Solver knowledge crosses one canonical, transactional boundary.**
> Goals and their local environment are canonicalized together; rigid inputs,
> flexible inputs, universes, substitutions, residuals, and goal-specific
> outputs all round-trip without capture or lost sharing. A normalization
> output is imported before it is related to the caller's expected type. This
> is the inference consequence of
> [D14](../decisions.md#d14-solver-operations-return-goal-specific-semantic-outputs).
>
> **Required verification:** Canonical round-trip tests cover rigid and
> flexible inputs, nested binders, universe lowering, shared response-local
> variables, residual obligations, and type outputs. An incompatible expected
> type does not select among otherwise ambiguous normalization candidates.

## Incremental boundary

Inference state is deliberately recreated when `LocalFnSym::body` executes.
Making each unification step a Salsa query would expose unstable partial state
and create dependencies below the semantic body boundary. Incremental reuse
comes from reusing the completed `CheckedBody` and the signature, metadata,
resolution, and canonical solver queries it observed.

Speculative branch order must not affect the final semantic result. Stable
stash allocation and final type resolution make equal completed bodies
backdatable even if internal scheduling differs.

<a id="inf-a4"></a>
> **INF-A4 — The completed body is the incremental and scheduling boundary.**
> Mutable inference steps are not separately memoized or externally visible.
> Reuse is determined by the completed `CheckedBody` and its semantic query
> dependencies, and internal branch or polling order cannot change that body.
> This is the inference consequence of
> [D15](../decisions.md#d15-cross-item-dependencies-stop-at-semantic-interfaces).
>
> **Required verification:** Query traces show that unchanged and
> interface-equivalent requests reuse the body without replaying inference,
> while relevant interface edits reexecute it; scheduler-order perturbation
> produces an identical completed body and diagnostics.

## Code map

| Path | Responsibility |
|---|---|
| `check/infer_ctx.rs` | body-local context, type relations, obligations, runtime, finalization |
| `check/finalize.rs` | resolve settled type edges and rebuild completed Typed IR in a fresh stash |
| `check/infer/egraph.rs` | versioned union-find, bounds, wake effects, transactional collapse |
| `check/infer/version.rs` | branch ownership, inference-variable identity, universes |
| `check/infer/runtime.rs` | cooperative task polling and wakeups |
| `check/infer/obligation.rs` | retained proof/normalization work and provenance |
| `check/expr.rs` | syntax-directed constraints and elaboration |

## Current status

### Current frontier

Type variables, structural equality, a limited subtyping/coercion relation,
transactional candidate work, wake-driven obligation retries, error-sentinel
recovery, and final quiescence support the two completed mini-redis bodies.

### Implemented capabilities and evidence

| Anchor | State | Implemented claim and evidence |
|---|---|---|
| [INF-A1](#inf-a1) | Partial | `body_finalization_resolves_types_into_a_fresh_stash` proves nested inference edges resolve into a separately owned output while the working stash is unchanged; `equivalent_obligations_deduplicate_after_inference` covers obligation quiescence after inputs narrow; `clone_method_call_is_elaborated_to_a_resolved_trait_call` inspects an inference-free completed body for the supported method slice. Direct negative completion tests for every live-state class are missing. |
| [INF-A2](#inf-a2) | Partial | `canonical_equality_round_trips_transactionally` checks transactional response import, while `implication_assumptions_do_not_leak_to_siblings` verifies proof-branch isolation. Cancellation-specific isolation evidence is missing. |
| [INF-A3](#inf-a3) | Partial | `canonical_equality_round_trips_transactionally`, `local_associated_type_normalization_produces_a_type_output`, and `response_local_type_output_round_trips_with_sharing` cover the implemented equality and type-output boundary; `conjunction_retries_after_a_sibling_pins_its_input` verifies retained work observes later hard information. No caller-side expected-type test yet proves that an expectation cannot select one of two otherwise ambiguous normalization candidates. |
| [INF-A4](#inf-a4) | Partial | `clone_method_body_has_a_narrow_reusable_semantic_query_trace` verifies warm reuse for one completed body slice; scheduler-order perturbation is not covered. |

### Current limitations

- Subtyping and coercions currently cover only a small structural subset.
- Numeric inference/fallback, closures, casts, patterns, and control-flow joins
  are incomplete relative to Rust.
- All lifetime syntax becomes `Lifetime::Dummy`; no lifetime variables or
  borrow checks are performed.
- The executor is single-threaded and cooperative; planned solver fairness and
  concurrent conjunction work are not implemented.
- Failed bodies recover with error types and diagnostics, not a structured
  phase-incompleteness taxonomy.
- INF-A3's source boundary imports normalization output before relating the
  caller expectation, but the required caller-side candidate-isolation
  regression test is missing.

### Related roadmap slices

[Solver recursion, scheduling, and monotone
progress](../../implementation/roadmap.md#planned-slice-solver-recursion-scheduling-and-monotone-progress)
will change task progress and wake behavior. [Shutdown::recv](../../implementation/roadmap.md#next-application-slice-shutdownrecv)
will extend body-level inference coverage.
