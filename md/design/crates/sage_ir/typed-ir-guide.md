# Typed IR

This guide explains the representations specified in [Types](types.sage) and
[Symbols](symbols.sage). Generated anchor pages collect implementation and test
associations; the discussion here gives context for reading those contracts.

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
[Completed bodies are elaborated typed trees](../../../rfds/architectural-foundations/README.md#completed-bodies-are-elaborated-typed-trees).

The specification is {spec}`crates.sage_ir.types.body.typed_provenance`, {spec}`crates.sage_ir.types.body.resolved_references`, {spec}`crates.sage_ir.types.body.no_inference`, {spec}`crates.sage_ir.types.body.elaborated`, {spec}`crates.sage_ir.types.body.diagnosed_recovery`.

Review cases: Completion validators and representative typed-IR snapshots traverse the entire returned tree and reject live inference, unresolved/source-shaped operations, unsupported placeholders, and missing type, definition, or provenance fields.

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

The specification is {spec}`crates.sage_ir.types.calls.selected_semantics`, {spec}`crates.sage_ir.types.calls.dispatch_families`.

Review cases: Typed-tree fixtures cover direct, static-trait, dynamic, pointer, and callable dispatch as each becomes supported; they assert owner and method substitutions and the explicit adjustment nodes, and verify that no replayable adjustment list or selected-impl field remains.

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
| closure | {spec}`crates.sage_ir.types.body.closures` |
| async block or async function body | {spec}`crates.sage_ir.types.body.coroutines` |
| `x.method(a)` | Resolved call with dispatch, owner/method substitutions, and receiver operations materialized (built for the conservative external trait and inherent method slices; general lookup planned) |
| non-short-circuit operator syntax | Resolved trait-method call (built for ground primitive and local user-defined operands; generic bodies planned) |
| `x && y`, `x || y` | Typed lazy logical operation with conditional RHS evaluation |
| `x[i]` | {spec}`crates.sage_ir.types.body.indexing` |
| callable `f(a)` | Direct, pointer, dynamic, or `Fn*` call as resolved (planned) |
| implicit coercion | Explicit coercion, borrow, dereference, or unsizing node (shared borrow and built-in dereference represented; general coercions planned) |
| `if let`, `while let`, and `let` chains | {spec}`crates.sage_ir.types.body.conditional_patterns` |
| `for` | {spec}`crates.sage_ir.types.body.for_iteration` |
| `expr?` | {spec}`crates.sage_ir.types.body.try_residual` |
| `.await` | {spec}`crates.sage_ir.types.body.await` |
| macro invocation | {spec}`crates.sage_ir.types.body.macro_expansion` |

`await` deliberately remains above generator lowering. Its node records the
resolved future conversion and output type, but not polling, suspension
points, or state-machine layout.

Overloadable operators remain calls throughout Typed IR. For example, both a
concrete `u8 + u8` expression and an operation in a generic `T: Add` body name
`Add::add` with the applicable substitutions and explicit adjustments. The
generic body does not yet determine whether a future instantiation uses a
primitive or user-defined implementation. Future codegen may recognize a
supported primitive operator call once substitutions are concrete and replace
it with an appropriate primitive operation. Lazy `&&` and `||` remain direct
logical nodes because their non-overloadable, short-circuit evaluation cannot
be represented by an ordinary eager call.

This is the representation consequence of
[Overloadable operators remain trait calls through Typed IR](../../../rfds/architectural-foundations/README.md#overloadable-operators-remain-trait-calls-through-typed-ir).

The specification is {spec}`crates.sage_ir.types.operators.trait_calls`, {spec}`crates.sage_ir.types.operators.lazy_control_flow`.

Review cases: Completed-tree fixtures inspect concrete arithmetic, mixed-integer shift, generic operator, and user-defined operator cases and require resolved method identity, substitutions, adjustments, and result types. No overloadable case may complete as `Binary`; `&&` and `||` fixtures must retain conditional RHS evaluation.

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
[Named, associated, and opaque aliases share one semantic family](../../../rfds/architectural-foundations/README.md#named-associated-and-opaque-aliases-share-one-semantic-family).

The specification is {spec}`crates.sage_ir.types.aliases.identity`, {spec}`crates.sage_ir.types.aliases.revelation`.

Review cases: Copy/fold/display, inference, canonical query/response, oracle, and normalization tests preserve alias kind, definition, and arguments; boundary tests distinguish named revelation, associated projection, and opaque reveal permissions.

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
[Lifetimes collapse to `Dummy` and borrow checking is deferred](../../../rfds/architectural-foundations/README.md#lifetimes-collapse-to-dummy-and-borrow-checking-is-deferred).

The specification is {spec}`crates.sage_ir.types.lifetimes.dummy`, {spec}`crates.sage_ir.types.lifetimes.unchecked_borrows`.

Review cases: Lowering and import tests enumerate explicit, elided, bound, inferred, external, and synthesized lifetime origins; typed-IR snapshots retain `Dummy` on reference operations; negative fixtures pin the temporary acceptance of programs rejected only by lifetime or borrow rules.

## Query boundary

`LocalFnSym::body` remains the public query boundary. It reads the owning
signature and only the signatures, fields, associated items, impl headers, and
solver results demanded while checking that body. Callee bodies are not a
dependency of call checking.

Elaboration may use temporary source-shaped nodes, inference variables,
method-resolution candidates, and adjustment recipes internally. Those are
not separately queryable public IR and must be consumed before a successful
`CheckedBody` is returned. The inference graph and its working stash are not
the completed representation: completion resolves every type edge and copies
the tree into a fresh append-only stash.

This is the body-representation consequence of
[Cross-item dependencies stop at semantic interfaces](../../../rfds/architectural-foundations/README.md#cross-item-dependencies-stop-at-semantic-interfaces).

The specification is {spec}`crates.sage_ir.symbols.body_interfaces`.

Review cases: Cold and warm body traces enumerate allowed semantic inputs and assert the absence of callee/sibling body reads; edit tests show that a callee-body-only change does not change or reexecute the caller's completed body.

## Oracle comparison

The oracle compares the semantic typed tree, not Sage's temporary checking
state or rustc's adjustment encoding. The rustc side translates its selected
definitions, substitutions, and adjustments into the same explicit tree form.
For overloadable operators this includes projecting rustc's concrete scalar
operation back to the language-item trait-method call required by {spec}`crates.sage_ir.types.operators.trait_calls`.
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

## Implementation and test evidence

The generated [anchor pages](../../anchor/crates/sage_ir/types.html) report implementation entries,
annotated tests, explicit TODO/N/A gaps, and saved claim/test reviews. These
associations do not themselves establish that a test ran.

The [earlier coverage snapshot](../../../rfds/implementation-sequencing/coverage-history.md#typed-ir)
preserves the former capability descriptions, edit experiments, and limitations
as historical evidence. Current coverage comes from the generated entries;
implementation plans and decision rationale belong in the relevant RFDs.
