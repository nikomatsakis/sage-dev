# RFD: Numeric Inference Domains and Binary Operator Typing

**Status:** Accepted

**Depends on:**

- [Type Inference](../type-inference/README.md) — body-local inference
  variables, versioned transactions, and finalization
- [Typed IR Elaboration](../typed-ir-elaboration/README.md) — resolved calls,
  explicit semantic operations, and recovery nodes
- [External Metadata](../../design/subsystems/external-metadata.md) — relevant
  impl identities and headers from `core` and other dependency crates

## TL;DR

- Give every inference variable an effective domain: `Any`, `Integer`, or
  `Float`. Unsuffixed integer and float literals start in the corresponding
  restricted domain; ordinary inference variables start as `Any`.
- Store domain narrowing in the versioned egraph. Unification intersects
  domains, so a restriction follows the whole equality class and speculative
  narrowing rolls back with its branch.
- Do not select a concrete default for an unresolved numeric class in this
  RFD. Expected types, equality, selected impls, and where-clauses may narrow
  it normally; otherwise it reaches finalization as unresolved inference.
- Represent every non-short-circuit binary operator as an operator-trait
  obligation with the trait method's result type. Operator spelling alone does
  not narrow either operand's domain or equate their types.
- Discover primitive operator implementations as ordinary `core` impls through
  external metadata and the trait solver. Do not add a compiler-builtin
  primitive candidate or hardcoded operator matrix. A later general design may
  derive common constraints from the actual complete set of viable answers.
- After obligations settle, elaborate every proven non-short-circuit operator
  to a resolved static trait-method call, including operations whose current
  operands are concrete primitive types. Primitive recognition belongs to
  future codegen, after generic substitutions are concrete. A failed or
  ambiguous obligation produces a diagnostic and recovery node.

## Motivation

Sage currently assigns the same unconstrained `Ty::InferVar` to every
unsuffixed numeric literal. The expression `1 + true` therefore equates the
literal variable with `bool` and completes without a type diagnostic. This is
[KD-7](../../implementation/known-deviations.md#kd-7-arithmetic-operators-do-not-require-numeric-operands).

The same missing information prevents the checker from preserving what an
unsuffixed literal is allowed to become while other constraints are pending:

```rust,ignore
let integer = 1;   // some integer type, not bool or a user-defined type
let float = 1.0;   // some floating-point type
```

These are two aspects of one problem. The checker must remember which concrete
types a partially known value may still become, and operator checking must
express more than equality between its operands.

Here, a type's **domain** means the set of concrete types still permitted by a
constraint. For example:

- an unsuffixed integer literal permits the signed and unsigned integer types;
- an unsuffixed float literal permits `f32` and `f64`;
- an ordinary inference variable initially permits any type; and
- unifying an ordinary variable with an integer literal narrows their complete
  equality class to `Integer`.

A domain records a restriction justified by the expression which created a
variable or by a later proof-derived constraint. It does not describe every
type accepted by an operator token: `<<`, for example, is overloadable and may
accept user-defined operands. Merely using an operator never narrows an
ordinary inference variable to a numeric domain.

## Change in a nutshell

For this source:

```rust,ignore
fn example() {
    let a = 1;
    let b = 1.0;
    let c = 1u8 << 2i16;
    let d = 1 + true;
}
```

checking should establish:

```text
a: ?I where domain(?I) = Integer  ------------> unresolved without context
b: ?F where domain(?F) = Float    ------------> unresolved without context
c: u8: Shl<i16, Output = u8>      -------------> resolved Shl::shl call
d: ?I: Add<bool>, domain(?I) = Integer --------> diagnostic + recovery
```

The shift is the important counterexample to the current equality shortcut:
its operands are both integers, but Rust permits different integer types and
the result has the left operand's type. This RFD intentionally does not decide
whether or when the unresolved `a` and `b` should become `i32` and `f64`.

## Detailed design

### 1. Domains constrain equality classes

Keep the single `Ty::InferVar(InferVarIndex)` representation. Add an initial
domain to immutable variable metadata and versioned domain refinements to the
egraph state:

```rust,ignore
enum InferDomain {
    Any,
    Integer,
    Float,
}

struct VarInfo {
    owner: Version,
    creation_universe: Universe,
    initial_domain: InferDomain,
}

struct VersionState<'db> {
    // existing parents, bounds, ceilings, dependents, worklist, and wakes
    domains: FxHashMap<Ptr<Ty<'db>>, InferDomain>,
}
```

`Any` is the top of this small lattice. Narrowing computes an intersection:

| left | right | result |
|---|---|---|
| `Any` | `D` | `D` |
| `Integer` | `Integer` | `Integer` |
| `Float` | `Float` | `Float` |
| `Integer` | `Float` | type error |

The effective domain belongs to the current egraph class, not whichever
variable happens to be its representative. Before unioning two classes, the
egraph intersects their effective domains and records the result on the new
representative in the current version. Binding a restricted class to a
concrete type first checks membership: signed and unsigned integer types are
in `Integer`, `f32` and `f64` are in `Float`, and all concrete types are in
`Any`. `Ty::Error` remains admissible to every domain for diagnostic recovery;
it is never evidence of a valid operator.

Domain compatibility is enforced by the transactional type-relation layer,
not by individual expression checkers. Before unifying two variable classes,
the unifier intersects their effective domains and rejects an empty
intersection. Before binding a restricted class to a known concrete type, it
checks membership before installing an exact bound or unioning the classes.
Every other operation which can install an exact type must use the same check;
the low-level egraph union and bound mutations must not provide an unchecked
public bypass.

A failed check returns a structured domain-conflict result from the same
fallible unification operation which reports structural mismatch, occurs-check
failure, and universe leakage. Because it occurs within the owning inference
child, failure discards all partial equality, domain, universe, bound, and wake
state. Candidate probing treats that result as ordinary candidate rejection;
it does not emit a diagnostic.

Domain refinement is a semantic change just like a bound or universe-ceiling
change. A committed refinement advances the semantic revision and wakes
consumers of the affected variables. A discarded child version publishes
neither the refinement nor its wakes.

<a id="niv-a1"></a>
> **NIV-A1 — A domain is a transactional equality-class constraint.** The
> effective domain is preserved regardless of union representative, narrows
> only by intersection, and participates in the same commit, rollback,
> revision, and wake rules as the equality facts it constrains.
>
> **Required verification:** Focused egraph tests cover both union orders,
> transitive variable unions, concrete binding, incompatible intersections,
> exact-bound installation, accepted-child collapse, discarded-child
> isolation, semantic revisions, and wakes. Tests exercise every class-changing
> entry point and prove that none bypasses domain compatibility. A generic
> caller variable unified with an integer or float literal retains that literal
> domain.

This deliberately differs from rustc's distinct `IntVar` and `FloatVar` type
forms. Sage still preserves the same semantic distinction, but attaches it to
the transactional equality class so it survives unions with ordinary caller
variables and branch rollback. The representation tradeoff is discussed under
[Alternatives considered](#encode-intvar-and-floatvar-in-ty).

### 2. Literal construction and constraint propagation

Literal checking distinguishes suffixes from unsuffixed literals:

- a suffixed integer literal immediately has its named integer type;
- a suffixed float literal immediately has `f32` or `f64`;
- an unsuffixed integer literal creates an `Integer` inference variable; and
- an unsuffixed float literal creates a `Float` inference variable.

The token's value and range remain attached to the literal expression. Range
validation and `overflowing_literals` diagnostics are separate from choosing
the type and are not added by this RFD.

Ordinary equality, expected types, signature instantiation, selected impls,
where-clauses, and retained obligations may constrain a numeric class to a
concrete member of its domain. These are proof-derived constraints and use the
same egraph refinement and wake machinery as other inference.

This RFD adds no numeric fallback. An integer- or float-domain class which
remains unresolved at terminal body finalization receives the existing
unresolved-inference diagnostic and recovery treatment. It does not silently
become `i32` or `f64`. Choosing a default can resolve otherwise ambiguous trait
goals and therefore belongs to a later, holistic inference-commitment design
which accounts for impl and where-clause constraints, candidate completeness,
normalization, never fallback, and the required post-commitment solver pass.

<a id="niv-a2"></a>
> **NIV-A2 — Numeric domains constrain inference without selecting a
> default.** A numeric variable becomes concrete only through ordinary
> proof-derived constraints in this RFD. An unresolved numeric class remains
> unresolved at terminal finalization and follows explicit diagnostic
> recovery; scheduler quiescence is not authority to choose a concrete type.
>
> **Required verification:** Source-driven tests cover explicit annotations,
> call and generic propagation, selected-impl and where-clause constraints,
> consumer wakeup, and independent unrelated variables. Unconstrained integer
> and float literals must not silently become `i32` or `f64`; completion tests
> inspect their diagnostic recovery and prove that successful Typed IR contains
> only types selected by actual constraints.

### 3. Trait-first operator checking

Except for `&&` and `||`, binary syntax first creates the corresponding
operator-trait operation. The checker retains a goal of the form
`Lhs: Operator<Rhs>`, instantiates the trait method, and relates the expression
type to that method's result. Associated `Output` normalization is therefore
part of the same operation for traits such as `Add` and `Shl`. The selected
trait proof and instantiated method signature are the semantic authority for
both primitive and user-defined operands.

The operator token selects the language-item trait and method, not a special
primitive implementation family. Candidate discovery then follows the normal
trait path across local source and reachable external crates. Implementations
such as `u8: Add<u8>` and `u8: Shl<i16>` are ordinary impls defined in `core`;
Sage obtains their identities and headers through external metadata and proves
them exactly as it does other external impls. They are not solver
`BuiltinImpl` candidates of the kind needed for structural compiler traits
such as `Sized` or auto traits.

Operator spelling adds no independent domain restriction or equality between
its operands. A goal such as `Lhs: Shl<Rhs>` may select a user-defined impl
whose types are not numeric. If one impl or where-clause is selected, importing
that proof's response may constrain either operand and the result through
ordinary inference. If several viable candidates remain, this RFD imports no
operator-specific “common” constraint from their candidate set.

Extracting shared information from ambiguous answers can still be useful. For
example, the complete set of viable `core` impl responses for `u8: Shl<?Rhs>`
may all constrain the result to `u8`. Any such constraint must be derived from
the actual assembled candidate answers and their completeness, through a
general inference-level hints or answer-merging design. Sage must not recreate
those answers from a hardcoded table of primitive operator signatures.

Lazy boolean operators are the exception to trait-first checking: they check
both operands as `bool`, preserve lazy evaluation, and create no operator-trait
goal.

<a id="niv-a3"></a>
> **NIV-A3 — Non-short-circuit operator syntax contributes only its trait
> operation.** Every such expression retains its operator-trait goal and method
> result. The operator token alone neither narrows operand domains nor equates
> operand types; constraints enter inference only through ordinary expression
> checking and imported proof responses in this slice.
>
> **Required verification:** Goal-level tests establish the operator trait and
> method result for primitive and user-defined operands. Query traces show
> primitive arithmetic and shift proofs loading ordinary `core` impl identities
> and headers through external metadata, with no synthetic primitive candidate
> or hardcoded signature table. Tests prove an
> unresolved ordinary operand remains `Any` merely from appearing in `+` or
> `<<`, while a selected impl or where-clause may constrain it. Source-driven
> cases include `1 + true`, `1u8 << 2i16`, a nonnumeric user-defined shift, and
> a user-defined `Add` whose `Rhs` and `Output` differ from `Self`.

### 4. Typed IR preserves operator trait calls

Every successfully proven non-short-circuit operator becomes a resolved
static trait-method call in completed Typed IR. The call records the operator
trait method, trait and method substitutions, explicit operand adjustments,
and final result type. This rule applies equally to concrete primitive types,
user-defined types, and generic parameters:

```rust,ignore
fn add<T: Add<Output = T>>(lhs: T, rhs: T) -> T {
    lhs + rhs
}
```

The generic body's semantic operation is `<T as Add<T>>::add`; body checking
cannot know whether a future monomorphization uses a primitive impl or a
user-defined one. Keeping the same call shape for `u8 + u8` avoids making an
earlier representation choice which cannot be applied uniformly to generic
bodies.

Future codegen owns primitive recognition. Once substitutions are concrete,
it may recognize a call to an operator language-item method with a supported
primitive signature and emit the corresponding primitive instruction or
lowered IR node. Otherwise it follows normal static trait-call lowering. That
later design must preserve Rust's overflow, shift, division, remainder, and
comparison semantics; this RFD does not specify that lowering matrix.

Lazy `&&` and `||` remain direct, short-circuiting boolean operations rather
than eager trait calls. They are not overloadable and their conditional RHS
evaluation is part of their semantics.

An operator with unresolved ordinary `Any` operands remains pending rather
than becoming numeric merely because the source used `+`. If terminal
checking cannot prove one applicable operator-trait operation, it emits a
diagnostic and a recovery node. There is no separate primitive candidate that
can bypass or compete with trait selection.

Expected result types constrain the trait method result but do not bypass the
trait goal or decide otherwise ambiguous semantic dispatch. This follows
[BODY-A4](../../design/pipeline/body-checking.md#body-a4): caller expectations
must not select a semantic candidate.

<a id="niv-a4"></a>
> **NIV-A4 — Typed IR represents proven operators uniformly as resolved
> calls.** After trait selection and type finalization, every proven
> non-short-circuit operator is a resolved static trait-method call, including
> calls over concrete primitive types and generic parameters. Primitive
> recognition is deferred to codegen. Failure or ambiguity produces an
> explicit terminal diagnostic and recovery node.
>
> **Required verification:** Typed-IR tests inspect a concrete primitive case,
> a mixed-integer shift, a generic operator body, and user-defined operators
> with both `Self` and non-`Self` `Rhs`/`Output`. Every successfully proven case
> is a resolved call with the expected method, substitutions, adjustments, and
> result type; no overloadable operator completes as `Binary`. Lazy boolean
> operators preserve short-circuit form, while failure and ambiguity produce
> diagnosed recovery.

### 5. Completion and diagnostics

Domains are body-local inference metadata and do not become `Ty` variants,
signature types, or completed Typed IR. When a variable crosses the canonical
solver boundary, its canonical variable metadata includes the effective
domain whenever that distinction can affect the answer; importing a response
must intersect, not overwrite, the caller's domain. The exact encoding can
follow the existing universe metadata shape, but distinct domains must not
share a cache key.

An incompatible literal annotation or failed operator obligation reports a
diagnostic at the constraining source span and installs ordinary error
recovery. A later use of the recovery type must not produce a second domain
error solely because `Ty::Error` is not numeric.

The unifier itself does not decide whether a domain conflict is user-facing.
A required source relation such as `require_eq(actual, expected, span)` turns
the structured conflict into a type error, preserving the caller's
expected/actual orientation and diagnostic context. The message describes
source-level categories—for example, “expected `f32`, found an integer”—and
never exposes an inference-variable index. After reporting, the affected class
is related to `Ty::Error(ErrorReported)` so finalization and downstream checks
do not emit derivative unresolved-inference or operator errors.

Canonical solver inputs carry each variable's effective domain. Candidate-side
relations therefore enforce it during proof, and transactional response import
uses the same unifier to recheck the response against the caller's current
domain before commit. A conflicting candidate or response is rejected without
publishing partial state or a candidate-local diagnostic.

<a id="niv-a5"></a>
> **NIV-A5 — Domains do not survive successful completion.** Domain metadata
> influences inference, canonical answers, and diagnostics, but a
> successful `CheckedBody` contains only concrete types and selected semantic
> operations.
>
> **Required verification:** Completion tests reject live domain-constrained
> variables, canonical round trips distinguish and preserve domains, and
> response-import tests reject incompatible substitutions transactionally.
> Diagnostic tests distinguish silent speculative rejection from a required
> source mismatch and establish one primary source-level error plus recovery
> for incompatible bindings and operators, without printing internal variable
> indices.

## Interactions and scope

### Generic arguments

A caller-side inference variable introduced for `T` begins as `Any`. In
`id(1)`, unifying that variable with the literal's `Integer` variable narrows
the whole class to `Integer`; if nothing else constrains it, both remain
unresolved and receive terminal diagnostic recovery. A rigid `Ty::Param(T)`
does not become numeric: requiring an arbitrary generic parameter to equal an
integer literal is an error unless a separate supported operator/trait
contract justifies the operation.

### Casts

An `as` cast does not equate its source and target types. For `1 as u8`, the
source retains its integer domain unless the eventual cast rules constrain it,
the cast validates that the source/target pair is permitted, and the cast
expression has type `u8`. General cast checking and cast Typed IR remain out of
scope; this RFD only rules out using equality as their future model.

### Unary operators

Unary `-` and `!` should use the same trait-call-in-Typed-IR and
primitive-recognition-in-codegen boundary, but their complete design is outside
the KD-7 closure slice. Their implementation must reuse class domains and
cannot reintroduce numeric-specific `Ty` variants.

### Numeric coercions

This RFD adds no implicit integer-to-integer, float-to-float, or integer-to-
float coercions. Equality constraints still require one concrete type unless
an explicit, separately designed coercion site says otherwise.

## Alternatives considered

### Encode `IntVar` and `FloatVar` in `Ty`

Rustc uses distinct integer and float inference variants, separate unification
tables, and distinct canonical variable kinds. Sage does not copy that
representation because its restriction is mutable equality-class metadata: it
must survive representative changes, transactional narrowing, and unions with
ordinary caller variables. Separate `Ty` variants would still need a
class-wide compatibility mechanism and would duplicate structural handling.
The chosen metadata representation preserves the same observable distinction
at canonicalization and constraint-import boundaries while reusing Sage's
versioned egraph machinery.

### Default unresolved numeric variables in this RFD

Deferred. Rust normally defaults unresolved integer and float variables to
`i32` and `f64`, but a default can resolve or reject a pending trait ambiguity.
Sage needs one explicit commitment policy which orders all proof-derived
constraints before heuristics and accounts for provider completeness,
normalization, where-clauses, never fallback, and re-solving. Neither eager
literal typing nor a scheduler-local quiescence rule establishes that boundary.

### Hardcode primitive operator relationships

Rejected. Rustc applies operator-specific inference shortcuts in addition to
proving the ordinary primitive impls defined in `core`, but Sage does not need
to copy those shortcuts. A hardcoded matrix would duplicate library facts and
could diverge from the impls in the compilation environment.

A future general inference-level hints or answer-merging design may derive
facts shared by multiple viable impl responses. That mechanism must operate on
the actual candidate answers, define when their provider set is complete, and
account for where-clauses and normalization. It must not synthesize candidates
or constraints merely from binary operator spelling and primitive type shape.

### Treat every binary operator as same-typed operands

Rejected because it both admits invalid domains such as `bool + bool` and
rejects valid pairs such as `u8 << i16`. It also conflates primitive
operations with trait-based overloads whose `Rhs` and `Output` may differ.

### Classify primitive operators in Typed IR

Rejected. A concrete expression such as `u8 + u8` could be classified during
body checking, but the body of `T: Add<Output = T>` cannot be classified until
its generic substitutions are concrete. Uniform resolved calls preserve the
source operation's semantics in both cases and leave representation-specific
primitive recognition at the codegen boundary where it applies uniformly.

## Reference behavior

The design follows the Rust Reference rules for [integer and floating-point
literal inference](https://doc.rust-lang.org/reference/expressions/literal-expr.html),
the primitive and overloadable [arithmetic and logical operator
table](https://doc.rust-lang.org/reference/expressions/operator-expr.html#arithmetic-and-logical-binary-operators),
the [comparison traits](https://doc.rust-lang.org/reference/expressions/operator-expr.html#comparison-operators),
and the built-in [lazy boolean
operators](https://doc.rust-lang.org/reference/expressions/operator-expr.html#lazy-boolean-operators).
Rustc also defaults otherwise unresolved numeric variables; Sage intentionally
defers that behavior in this RFD, so corresponding oracle cases remain an
explicit known scope difference rather than false conformance evidence. The
[rustc Binary Operator Typing and Lowering research
chapter](../../design/research/rustc-binary-operator-typing.md) records the
source-level compiler investigation behind the trait-first design, including
rustc's earlier primitive/call split and fallback ordering. Sage deliberately
keeps calls uniform through Typed IR and defers that split to codegen.

## High-level implementation plan

1. **Add transactional inference domains.** Extend inference-variable
   allocation and egraph equality classes with `Any`, `Integer`, and `Float`
   domains. Make intersection, concrete binding, rollback, commit, revision,
   wakeup, and canonical query/response handling preserve those restrictions.
   Route every exact class mutation through the same fallible compatibility
   check.
2. **Construct numeric literals with the right domain.** Lower suffixed
   literals directly to their declared primitive types and create restricted
   variables for unsuffixed literals. Propagate constraints normally, but leave
   an otherwise unresolved numeric variable for terminal diagnostic recovery
   rather than defaulting it.
3. **Replace binary equality with trait-first checking.** Map each
   non-short-circuit operator token to its language-item trait method, create
   the trait and result-normalization obligations, and discover both local and
   ordinary `core` impls through the existing candidate machinery. Import only
   proof-derived constraints; do not introduce a primitive candidate or
   hardcoded operator matrix.
4. **Elaborate the proven operation.** Emit a resolved static trait-method call
   for every successful overloadable operator, including primitive and generic
   operands. Preserve `&&` and `||` as direct lazy operations, and turn failed
   or ambiguous obligations into diagnostics and recovery nodes. Convert only
   required source-level domain conflicts into diagnostics; speculative
   conflicts remain silent candidate rejection.
5. **Close the slice with source-level evidence.** Cover constrained and
   unconstrained literals, primitive and user-defined arithmetic, comparisons,
   mixed-integer shifts, generic operator bodies, invalid operands, and lazy
   booleans. Inspect completed Typed IR, external-impl query traces, and the
   independent rustc oracle projection before closing KD-7 and promoting the
   RFD anchors.

## Implementation

See the [detailed implementation checklist and status](./implementation.md).
