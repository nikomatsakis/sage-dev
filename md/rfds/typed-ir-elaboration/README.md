# RFD: Typed IR Elaboration

**Status:** Accepted

**Depends on:**

- [IR Reshape](../ir-reshape/README.md) — single-keyed body queries and the typed-tree boundary
- [Type Inference](../type-inference/README.md) — completed inference and coercion selection
- [Trait Solving](../trait-solving/README.md) — fixed-trait proofs and obligations
- [Method Resolution](../method-resolution/README.md) — method candidate selection and receiver recipes

## TL;DR

- Make completed bodies elaborated, fully resolved typed trees rather than
  typed copies of source syntax.
- Convert method calls, overloaded operations, and implicit coercions into
  explicit semantic nodes.
- Retain structured control flow and high-level closures/async bodies rather
  than lowering to MIR.
- Represent every lifetime as `Lifetime::Dummy` and defer borrow checking and
  meaningful lifetime inference.
- Compare the same semantic tree through the rustc oracle, with explicit
  coverage accounting.

## Motivation

The current `TyExprData` preserves source forms such as `MethodCall`, `For`,
`Try`, `Await`, and field names. This was useful while establishing the body
checker, but it does not define what downstream consumers may assume. A
consumer should not have to repeat method resolution, replay adjustment lists,
or infer which field a name selected.

Mini-redis makes this boundary immediate. Even the small body
`DbDropGuard::db` contains `self.db.clone()`: checking selects a field, a trait
method, a `Db: Clone` proof, and an implicit borrow. The result should encode
those choices directly.

## Change in a nutshell

Completed `CheckedBody` values satisfy the destination contract in [Typed
IR](../../design/typed-ir.md). Source-shaped nodes may exist during checking,
but successful finalization consumes them.

Conceptually:

```text
self.db.clone()
```

becomes:

```text
Call <Db as Clone>::clone(
    RefShared[Dummy](Field DbDropGuard::db(Deref(Local self)))
)
```

The method resolver may return an internal receiver-adjustment recipe. The
body checker applies it while constructing the call tree; the recipe is not
part of the completed IR.

## Detailed plans

### Separate checking state from completed IR

Checking may retain unresolved names, inference variables, candidate sets,
and source sugar. Successful body finalization validates that none remain and
produces a separate completed root. Error recovery produces explicit diagnosed
error nodes.

This remains one public `sym.body(db)` query. No intermediate resolved or
partially elaborated body becomes a cross-item query boundary.

### Resolve semantic identities

Completed nodes name definitions for fields, variants, constructors,
functions, and associated items. Calls retain substitutions and enough dispatch
information to distinguish direct, trait, dynamic, function-pointer, and
callable-trait calls.

Completed types distinguish rigid constructors from the common named,
associated, and opaque `AliasTy` family. Elaboration retains alias identity and
arguments; it does not eagerly erase every alias to a revealed type. The
separate normalization design owns how associated values are selected and how
alias relations are proved.

### Materialize implicit operations

Autoderef, autoref, unsizing, and other selected coercions become explicit
tree nodes. Primitive operations may remain intrinsics; overloaded operations
become resolved trait calls.

### Preserve structured control flow

Blocks, matches, conditional branches, and loops remain trees. Surface forms
whose typing depends on traits are elaborated as specified by the architecture.
Async bodies remain nested typed bodies, and `await` remains a high-level
operation over an explicitly resolved future. This RFD does not introduce
basic blocks, drop elaboration, or state-machine lowering.

### Defer lifetime and borrow semantics

Every lifetime-producing path immediately yields `Lifetime::Dummy`, including
explicit, elided, universal, existential, external, and synthesized lifetimes.
All lifetime relations are trivially true. Reference operations remain in the
tree, but Sage does not verify borrow validity.

This is a deliberate temporary soundness hole. The dedicated variant makes it
auditable and mechanically removable when a unified type-and-lifetime
inference design is accepted.

### Emit the shared oracle form

The rustc emitter minimally projects resolved definitions, substitutions, and
adjustments into the shared semantic tree rather than exposing rustc's
internal adjustment representation. Sage independently emits the same shared
form. Both sides serialize deterministically, and the harness decides
conformance using exact textual identity with no pairwise normalization. It
rejects successful outputs with unsupported placeholders and reports the set
of bodies covered by each side.

### Preserve narrow dependencies

Elaborating one body reads only semantic data it uses. In particular, a call
depends on the callee signature, owner predicates, relevant method/impl lookup,
and solver response, but not the callee body. Stable semantic query traces
make this contract executable.

## Frequently asked questions

### Is this Sage MIR?

No. The result is tree-structured and retains high-level control flow,
closures, and async bodies. A future MIR or other CFG can lower from this IR.

### Are receiver adjustments forbidden internally?

No. They are useful candidate-selection recipes. They are consumed before a
completed body is returned.

### Why not add lifetime inference variables now?

Doing so would prematurely choose a separate region-inference architecture.
Sage intends to consider a more unified type-and-lifetime inference design.
Until then, every lifetime is `Dummy` from creation.

### Does `Dummy` make lifetime errors ambiguous?

No. Lifetime relations succeed trivially. This intentionally accepts programs
which a future lifetime or borrow checker may reject.

## Implementation

See [Implementation plan and status](./implementation.md).
