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

## Entry points and callers

The body query constructs one context. Its fields show the state kept inside
that body boundary:

```{anchor}
architecture_infer_context
```

Expression and pattern `check_with` methods allocate types and typed nodes
through the context. `require_eq`, `require_sub`, and `require_coerce` relate
types. `finalize` performs fallback and mandatory terminal obligation
discharge; `resolve_types` replaces solved inference-variable nodes before
`finish` asserts quiescence.

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

## Incremental boundary

Inference state is deliberately recreated when `LocalFnSym::body` executes.
Making each unification step a Salsa query would expose unstable partial state
and create dependencies below the semantic body boundary. Incremental reuse
comes from reusing the completed `CheckedBody` and the signature, metadata,
resolution, and canonical solver queries it observed.

Speculative branch order must not affect the final semantic result. Stable
stash allocation and final type resolution make equal completed bodies
backdatable even if internal scheduling differs.

## Code map

| Path | Responsibility |
|---|---|
| `check/infer_ctx.rs` | body-local context, type relations, obligations, runtime, finalization |
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

- `canonical_equality_round_trips_transactionally` checks canonical response
  import without leaking failed state.
- `implication_assumptions_do_not_leak_to_siblings` verifies isolation between
  proof branches.
- `conjunction_retries_after_a_sibling_pins_its_input` verifies wake-driven
  progress after a related constraint becomes known.
- `equivalent_obligations_deduplicate_after_inference` verifies canonical
  deduplication after variables are narrowed.
- `clone_method_call_is_elaborated_to_a_resolved_trait_call` inspects the
  completed body rather than an inference intermediate.

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

### Related roadmap slices

[Solver recursion, scheduling, and monotone
progress](../../implementation/roadmap.md#planned-slice-solver-recursion-scheduling-and-monotone-progress)
will change task progress and wake behavior. [Shutdown::recv](../../implementation/roadmap.md#next-application-slice-shutdownrecv)
will extend body-level inference coverage.
