# Typed IR

This page defines the destination representation produced by body checking.
Implementation coverage and evidence are recorded in **Current status** rather
than weakening the representation contract below.

## Role

Sage's typed IR is an elaborated, tree-structured program representation. It
sits after parsing, name resolution, type inference, method resolution, and
trait solving, but before any future control-flow, drop, lifetime, or borrow
analysis.

It is comparable to a point between rustc's THIR and MIR:

- it retains structured expressions such as blocks, `if`, `match`, and loops;
- it names the definitions and substitutions chosen during checking;
- it materializes implicit type-directed operations as expression nodes; and
- it does not contain basic blocks, a drop schedule, a generator state
  machine, or borrow-check results.

The source CST remains available separately. The typed IR does not need to
preserve source sugar merely to reconstruct the user's spelling.

## Completed-body invariants

A successfully checked body has these properties:

- every expression and pattern has a type;
- no inference variable remains;
- every path, field, variant, function, and associated item names a resolved
  definition;
- method syntax and implicit receiver adjustments have been consumed;
- implicit borrows, dereferences, and coercions are explicit tree nodes;
- generic arguments and the selected trait reference are retained where they
  affect meaning;
- structured control flow remains tree-structured; and
- every node retains source provenance, including nodes synthesized from a
  source expression.

An error-recovery body may contain explicit error nodes backed by diagnostics.
It must not represent an unresolved choice as an ordinary successful node.

This is the representation consequence of
[D11](./decisions.md#d11-completed-bodies-are-elaborated-typed-trees).

<a id="tir-a1"></a>
> **TIR-A1 — A successful completed body is semantically closed.** Every node
> is typed and provenance-bearing; every semantic reference is resolved; no
> inference variable, method syntax, implicit adjustment recipe, or unresolved
> choice survives. Recovery is represented by explicit diagnosed error nodes,
> never by silently successful source-shaped placeholders.
>
> **Required verification:** Completion validators and representative typed-IR
> snapshots traverse the entire returned tree and reject live inference,
> unresolved/source-shaped operations, unsupported placeholders, and missing
> type, definition, or provenance fields.

## Calls and receiver elaboration

A call records what is being invoked rather than how it was spelled. The call
target distinguishes at least:

- a free or associated function definition;
- a statically dispatched trait method;
- a dynamically dispatched trait-object method;
- a function pointer; and
- one of the callable traits when call syntax resolves through `Fn`, `FnMut`,
  or `FnOnce`.

For example, given a `Db: Clone` proof, this source:

```rust,ignore
self.db.clone()
```

has the following conceptual shape after elaboration:

```text
Call <Db as Clone>::clone(
    RefShared[Dummy](
        Field DbDropGuard::db(
            Deref(Local self)
        )
    )
)
```

`Deref` and `RefShared` are ordinary typed nodes. A method resolver may use an
adjustment recipe while comparing candidates, but completed typed IR never
contains an adjustment list which a consumer must replay.

The selected trait method does not always identify a concrete impl. A call on
a generic `T: Clone` still names `Clone::clone` with `Self = T`; the proof that
`T: Clone` justifies the call. Trait-object calls additionally record dynamic
dispatch.

<a id="tir-a2"></a>
> **TIR-A2 — Calls record selected semantics, not source dispatch syntax.** A
> completed call identifies the invoked definition, dispatch family,
> substitutions, and explicit receiver/coercion operations. A trait proof
> justifies a trait-method call without requiring the solver to return a
> concrete impl identity.
>
> **Required verification:** Typed-tree fixtures cover direct, static-trait,
> dynamic, pointer, and callable dispatch as each becomes supported; they
> assert owner and method substitutions and the explicit adjustment nodes, and
> verify that no replayable adjustment list or selected-impl field remains.

The built call schema currently makes direct versus static-trait dispatch
explicit:

```{anchor}
example_call_dispatch_schema
```

Every represented method call separately retains substitutions owned by the
associated owner and substitutions introduced by the method:

```{anchor}
example_call_substitution_schema
```

For the current slice, `Iterator::next` records static dispatch with
`Self = IntoIter<Frame, Global>`, while `Option::ok_or` records direct dispatch,
owner substitution `T = Frame`, and method substitution `E = ParseError`.
Receiver adjustment recipes are absent because their dereference and borrow
operations are already expression nodes.

## Preserved and elaborated constructs

The table is the destination contract. A row marked *planned* is not yet fully
represented by the implementation.

| Source construct | Typed IR form |
|---|---|
| block, `let`, `if`, `match` | Preserved structured node with typed children |
| `loop`, `break`, `continue`, `return` | Preserved structured control flow |
| tuple, array, struct, enum construction | Resolved constructor and typed operands |
| field access | Resolved field definition, not a field name |
| closure | Typed nested body, explicit parameters and captures (planned) |
| async block or async function body | Typed nested coroutine body; no state-machine lowering (planned) |
| `x.method(a)` | Resolved call with dispatch, owner/method substitutions, and receiver operations materialized (built for the conservative external trait and inherent method slices; general lookup planned) |
| overloaded operator | Resolved trait call (planned) |
| primitive operator | Typed intrinsic operation |
| `x[i]` | Resolved `Index` or `IndexMut` call (planned) |
| callable `f(a)` | Direct, pointer, dynamic, or `Fn*` call as resolved (planned) |
| implicit coercion | Explicit coercion, borrow, dereference, or unsizing node (shared borrow and built-in dereference represented; general coercions planned) |
| `if let`, `while let`, and `let` chains | Structured match/control-flow form (planned) |
| `for` | `IntoIterator` call plus structured loop and match (planned) |
| `expr?` | `Try::branch`, `ControlFlow` match, and `FromResidual` call (planned) |
| `.await` | High-level await over an explicitly resolved `IntoFuture` value and `Future::Output` (planned) |
| macro invocation | Expansion output; the invocation is not a checked expression node |

`await` deliberately remains above generator lowering. Its node records the
resolved future conversion and output type, but not polling, suspension
points, or state-machine layout.

## Rigid and alias types

Types in completed IR are semantic types, not source type paths. Nominal and
other non-normalizable constructors are **rigid**: a constructor such as
`Vec` is equal only to the same constructor applied to equal arguments. A
rigid type is not sent to normalization merely because more information about
it would be useful.

An **alias type** is a distinct semantic type term which may have a normalized
form. The common family has three variants: `AliasTy::Named`,
`AliasTy::Associated`, and `AliasTy::Opaque`. This family is represented
through inference, canonical query and response boundaries, display, and both
semantic emitters. Operational reveal and normalization remain staged as
described below.

| Alias kind | Example | Normalization rule |
|---|---|---|
| Named type alias | `type Id<T> = T; Id<u32>` | Infallibly reveal the declared right-hand side with its arguments substituted |
| Associated-type projection | `<T as Iterator>::Item` | Match the trait reference and select an associated value from the environment or an impl; this may create further trait and normalization goals |
| Opaque type | the hidden type of `impl Trait` | Reveal only within the opaque's definition boundary, or during a future code-generation phase |

All three retain their definition identity and generic arguments. Successfully
checking a body does not require erasing every alias from its types. In
particular, an opaque outside its reveal boundary remains an opaque alias, and
an associated type may remain a projection when its normalized value is not
needed or cannot yet be selected.

Normalization is an input-only semantic operation which returns a type, rather
than an eager syntax-expansion pass or a predicate containing a caller-supplied
output term. One-step expansion of a named alias is infallible, although the
resulting type may itself contain aliases. Projection normalization participates
in trait solving. Opaque normalization consults the current reveal boundary;
code-generation reveal is explicitly deferred. A caller which needs equality
relates the returned type only after normalization candidate answers have been
merged.

Aliases can also support proofs without normalization. Declared bounds on an
opaque apply while its hidden type remains unrevealed. Given the required
trait fact, bounds declared on an associated type apply directly to the
projection. Structurally identical alias applications can be related by their
identity and arguments. The solver must therefore not treat "could not reveal
the alias" as "nothing is known about the type."

A debug-formatted string is never a semantic rigid or alias type.

This is the typed-IR consequence of
[D13](./decisions.md#d13-named-associated-and-opaque-aliases-share-one-semantic-family).

<a id="tir-a3"></a>
> **TIR-A3 — Alias identity survives until semantics permit revelation.** Named,
> associated, and opaque aliases retain their definition and arguments as one
> semantic type family. Normalization is an explicit operation; completed IR
> neither eagerly erases every alias nor treats an unrevealed alias as having
> no provable facts.
>
> **Required verification:** Copy/fold/display, inference, canonical
> query/response, oracle, and normalization tests preserve alias kind,
> definition, and arguments; boundary tests distinguish named revelation,
> associated projection, and opaque reveal permissions.

## Lifetimes and borrow checking

Lifetime semantics and borrow checking are deliberately deferred. The CST
preserves written lifetime syntax, but every explicit, elided, universal,
existential, imported, or synthesized lifetime lowers directly to the
dedicated typed-IR value `Lifetime::Dummy`.

`Dummy` is not `'static`, an inference variable, or post-analysis region
erasure. It states that Sage intentionally did not model the lifetime. The
temporary lifetime relation is:

```text
Outlives(Dummy, Dummy) = true
```

Reference and dereference nodes are still materialized because they determine
ordinary types, call signatures, and coercions. Sage currently makes no claim
that a borrow is live, unique, non-overlapping, or otherwise valid. This is a
known soundness hole, isolated behind the `Dummy` variant, which is intended to
be removed when the unified lifetime and type inference design is introduced.

This is the typed-tree consequence of
[D12](./decisions.md#d12-lifetimes-collapse-to-dummy-and-borrow-checking-is-deferred).

<a id="tir-a4"></a>
> **TIR-A4 — Deferred lifetime semantics are explicit and total.** Every
> lifetime origin lowers to `Lifetime::Dummy`, `Outlives(Dummy, Dummy)` holds,
> and reference/dereference nodes remain in the tree while borrow validity is
> unchecked. No path substitutes `'static`, creates a lifetime inference
> variable, or converts this omission into ambiguity.
>
> **Required verification:** Lowering and import tests enumerate explicit,
> elided, bound, inferred, external, and synthesized lifetime origins; typed-IR
> snapshots retain `Dummy` on reference operations; negative fixtures pin the
> temporary acceptance of programs rejected only by lifetime or borrow rules.

## Query boundary

`LocalFnSym::body` remains the public query boundary. It reads the owning
signature and only the signatures, fields, associated items, impl headers, and
solver results demanded while checking that body. Callee bodies are not a
dependency of call checking.

Elaboration may use temporary source-shaped nodes, inference variables,
method-resolution candidates, and adjustment recipes internally. Those are
not separately queryable public IR and must be consumed before a successful
`CheckedBody` is returned.

This is the body-representation consequence of
[D15](./decisions.md#d15-cross-item-dependencies-stop-at-semantic-interfaces).

<a id="tir-a5"></a>
> **TIR-A5 — A body depends on interfaces, never other bodies.** Checking and
> elaborating one function may read its own source and signature plus demanded
> signatures, fields, associated items, impl headers, metadata facts, and
> solver results. It must not read a callee or sibling body.
>
> **Required verification:** Cold and warm body traces enumerate allowed
> semantic inputs and assert the absence of callee/sibling body reads; edit
> tests show that a callee-body-only change does not change or reexecute the
> caller's completed body.

## Oracle comparison

The oracle compares the semantic typed tree, not Sage's temporary checking
state or rustc's adjustment encoding. The rustc side translates its selected
definitions, substitutions, and adjustments into the same explicit tree form.
Alias identity is part of that common model: comparison does not globally
expand named aliases or reveal opaques merely to make the two trees agree.

Each emitter serializes that common form deterministically, and conformance is
exact textual identity. There is no pairwise normalization or semantic
comparison after emission. Any permitted adaptation is a fixed part of one
emitter's projection into the shared schema and cannot consult or compensate
for the other output.

Conformance also reports coverage. Equality is not meaningful if both sides
omit an associated body or replace a construct with the same unsupported
placeholder. A conformance run therefore accounts for every body in scope and
rejects unsupported nodes or non-semantic debug types in successful output.

## Current status

### Current frontier

The `DbDropGuard::db` and `Parse::next` mini-redis slices produce explicit
field identities, dereferences, `Dummy` borrows, selected functions, static or
direct dispatch, owner/method substitutions, and associated-type normalization
results. The common rigid/alias family survives inference, canonical solver
boundaries, display, and both semantic emitters.

### Implemented capabilities and evidence

- **[TIR-A1](#tir-a1):** [Function body and field access](./examples/function-body.md)
  inspects a
  resolved local field and substituted final type.
- **[TIR-A1](#tir-a1)/[TIR-A2](#tir-a2):** [An oracle-checked method
  body](./examples/oracle-checked-method.md) inspects
  the resolved `Clone::clone` tree and its exact checked-in snapshot.
- **[TIR-A1](#tir-a1)/[TIR-A2](#tir-a2):** The `Parse::next` evidence in the [Mini-redis
  roadmap](../implementation/mini-redis.md#slice-2-parsenext) checks direct and
  static-trait calls, substitutions, and normalized iterator item type.
- **[TIR-A3](#tir-a3):** `alias_variants_copy_fold_and_display_without_erasing_identity` and
  `aliases_round_trip_through_canonical_query_and_response_stashes` exercise
  the shared alias representation.
- **[TIR-A5](#tir-a5):** `clone_method_body_has_a_narrow_reusable_semantic_query_trace`
  rejects callee-body reads and proves warm reuse for the pinned method body.

### Current limitations

- `TyExprData` still contains source-shaped variants for constructs whose
  general elaboration is not implemented. Such variants are not permitted in
  a successful oracle slice that claims their semantics.
- Closures/captures, async/coroutine bodies, overloaded operators, indexing,
  callable dispatch, general coercions/unsizing, `for`, `?`, and fully
  elaborated `.await` remain planned.
- Method resolution is limited to the conservative external trait/inherent
  slices and selected local obligations, so TIR-A2's full dispatch-family
  matrix is not yet established.
- Named-alias expansion and opaque reveal are planned; associated projection
  normalization covers the pinned first slice rather than GATs in general, so
  TIR-A3's reveal-boundary matrix remains incomplete.
- `Lifetime::Dummy` and the absence of borrow checking remain the deliberate
  temporary soundness hole described above. The exhaustive lifetime-origin
  evidence required by TIR-A4 is not yet assembled.
- TIR-A5 has focused no-callee-body evidence but not yet the complete
  callee-body edit matrix.

### Related roadmap slices

- [Shutdown::recv](../implementation/roadmap.md#next-application-slice-shutdownrecv)
  is the next Typed-IR application target.
- [Mini-redis library
  coverage](../implementation/roadmap.md#future-slice-mini-redis-library-coverage)
  expands the construct table through vertical acceptance slices.
- [Semantic inspector and persistent edit
  testing](../implementation/roadmap.md#implemented-slice-semantic-inspector-and-persistent-edit-testing)
  will provide a readable typed-tree view independent of oracle JSON.
