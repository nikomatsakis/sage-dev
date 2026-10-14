# Type Inference

Type inference is the body-local subsystem that relates partially known Sage
types, coordinates deferred obligations, and turns a checked expression tree
into a completed [Typed IR](typed-ir-guide.md). It is invoked by body checking;
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

{spec}`crates.sage_ir.inference.body_local_quiescence`

Verification intent: Completion tests inspect a representative typed
body for inference-free types and exercise `finish` with live variables,
obligations, tasks, and branches to prove that each prevents successful
publication; diagnostic recovery has separate expected output.

## Entry points and callers

The body query constructs one context. Its fields show the state kept inside
that body boundary:

```{anchor}
architecture_infer_context
```

Expression and pattern `check_with` methods synthesize types and allocate typed
nodes through the context. `check_with_expected` is the initial checking-mode
counterpart: a caller supplies a type before checking the expression at that
boundary. It currently synthesizes the expression first and applies the type
as a coercion expectation afterward; expression-specific inward propagation is
part of the destination described by [crates.sage_ir.checking.body.expected_types](../../anchor/crates/sage_ir/checking/body/expected_types.html).
`require_eq`, `require_sub`, and `require_coerce` relate types. `finalize`
performs fallback and mandatory terminal obligation discharge; `finish`
asserts quiescence and resolves every type edge while copying the working tree
into a fresh append-only result stash. The working stash remains unchanged, as
required by [crates.sage_stash.storage.immutable_entries](../../anchor/crates/sage_stash/storage/immutable_entries.html).

## Domain-constrained inference

An inference variable may carry a class-wide domain restriction in addition
to its structural equality and bound state. Ordinary variables initially
admit any type. Unsuffixed integer and float literals instead begin in
`Integer` and `Float` domains. The restriction belongs to the variable's
current egraph equality class: union intersects the two domains, binding to a
concrete type checks membership, and an empty intersection is a type error.

Domains are versioned semantic state. Speculative refinement is visible only
inside its inference child; collapse publishes it and wakes affected
consumers, while discard removes it without an observable effect. A caller
inference variable unified with a numeric literal inherits the numeric domain,
so the restriction survives generic instantiation and representative changes.
Using that variable as an operand of an overloadable operator does not itself
narrow the domain; only literal construction or a proof-derived constraint
does so in this slice.

Expected types, equality, selected impls, where-clauses, and other ordinary
proof-derived constraints may bind a numeric class to a concrete member of its
domain. Numeric domain metadata itself does not choose a concrete default. An
integer- or float-domain class which remains unresolved at terminal
finalization receives explicit unresolved-inference diagnostics and recovery,
just like other unresolved inference. A later holistic commitment policy may
introduce Rust's numeric defaults after defining their relationship to trait
ambiguity, provider completeness, normalization, and never fallback. The
[Numeric Inference Domains and Binary Operator Typing RFD][numeric-rfd]
specifies the accepted first slice.

The implemented lattice and its concrete membership predicate are centralized
in the inference version layer:

```{anchor}
inference_domain_lattice
```

{spec}`crates.sage_ir.inference.numeric_domains`

Verification intent: Egraph tests cover both union orders,
transitivity, incompatible concrete bindings, branch collapse/discard,
revisions, and wakes. Source-driven completion tests cover contextual
numeric inference, generic propagation, selected-impl and where-clause
constraints, preservation of an ordinary `Any` operand before proof
selection, unrelated-variable isolation, and explicit recovery for otherwise
unresolved integer and float classes. Tests prove this slice neither narrows
from operator spelling nor silently chooses `i32` or `f64`.

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

{spec}`crates.sage_ir.inference.transaction_isolation`

Verification intent: Failed occurs checks, universe checks,
multi-entry response imports, candidate matches, and cancellations leave the
parent semantically identical and publish no wake; concurrent siblings never
observe one another's branch state.

{spec}`crates.sage_ir.inference.canonical_boundary`

Verification intent: Canonical round-trip tests cover rigid and
flexible inputs, nested binders, universe lowering, shared response-local
variables, residual obligations, and type outputs. An incompatible expected
type does not select among otherwise ambiguous normalization candidates.

## Incremental boundary

Inference state is deliberately recreated when `LocalFnSym::body` executes.
Making each unification step a Salsa query would expose unstable partial state
and create dependencies below the semantic body boundary. Incremental reuse
comes from reusing the completed `CheckedBody` and the signature, metadata,
resolution, and canonical solver queries it observed.

Speculative branch order must not affect the final semantic result. Stable
stash allocation and final type resolution make equal completed bodies
backdatable even if internal scheduling differs.

{spec}`crates.sage_ir.inference.completed_body_boundary`

Verification intent: Query traces show that unchanged and
interface-equivalent requests reuse the body without replaying inference,
while relevant interface edits reexecute it; scheduler-order perturbation
produces an identical completed body and diagnostics.

## Subtyping and coercion

Subtyping and coercion are separate relations. Subtyping states that a value
can be used at the target type without changing its runtime representation:
the source and target therefore have the same memory layout. Equal layout is
necessary but not sufficient for subtyping. A coercion is a selected,
type-directed conversion and may produce a value with a different layout. If
checking selects such a conversion, the completed body records it explicitly,
as required by
[Completed bodies are elaborated typed trees](../../../rfds/architectural-foundations/README.md#completed-bodies-are-elaborated-typed-trees).

In particular, never-to-any is a coercion, not the subtype rule `! <: T`. It
may be selected when an expression of type `!` occurs at a coercion site, but
it cannot be applied recursively while establishing a structural subtype
relation such as `&mut ! <: &mut u32`.

When the coercion target is an unresolved inference type, a diverging
expression leaves a provisional conversion rather than immediately binding
the target to `!`. Other constraints have priority, independent of the order
in which coercion sites are checked. If no other constraint determines the
target, finalization defaults it to `!` and removes the now-redundant
conversion. Thus `pair(loop {}, true)` and `pair(true, loop {})` both infer the
shared parameter as `bool`, while `single(loop {})` infers its parameter as
`!`. If body checking is suspended waiting for such a target, quiescent
recovery defaults only the deferred target blocking that task; unrelated
targets remain available to constraints in the rest of the body.

{spec}`crates.sage_ir.inference.subtyping_and_coercion`

Verification intent: Source-driven integration tests reject
never-to-any beneath invariant type constructors, accept it at a top-level
coercion site, and inspect the completed Typed IR to show that the accepted
coercion is explicit. Focused relation tests cover every implemented subtype
and coercion rule and demonstrate that subtype success preserves layout.

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

## Implementation and test evidence

The generated [anchor pages](../../anchor/crates/sage_ir/inference.html) report implementation entries,
annotated tests, explicit TODO/N/A gaps, and saved claim/test reviews. These
associations do not themselves establish that a test ran.

Source-driven inference scenarios use `compile_test!` in
`crates/sage-test-harness/tests/inference.rs`. Inline `ty` observations expose
completed numeric types and explicit coercion wrappers; ERROR markers declare
expected failures before diagnostic snapshots are updated. These source tests
complement direct egraph, transaction, and typed-tree tests, which retain access
to internal invariants that a type snapshot cannot establish.

The [earlier coverage snapshot](../../../rfds/implementation-sequencing/coverage-history.md#type-inference)
preserves the former capability descriptions, edit experiments, and limitations
as historical evidence. Current coverage comes from the generated entries;
implementation plans and decision rationale belong in the relevant RFDs.
