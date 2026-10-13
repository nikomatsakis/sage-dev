# Signature Checking

Signature checking turns one definition's interface syntax into checked Sage
types and semantic parameter identities. It is the cross-item boundary:
another body may depend on a callee's signature, but never on the callee body.

## Contract

### Granularity

The unit of demand and memoization is one definition or one narrow piece of
its interface. A function, struct, enum, trait, impl, alias, const, or static
has a kind-specific signature query. Fields and associated-item enumeration
remain separate lazy queries when they form useful incremental boundaries.

### Input

The direct key is the definition symbol. Checking reads only the signature
portion of its CST (or one keyed external metadata fact), its owner and scope,
and signatures or metadata needed to resolve referenced interface types.

### Output

A successful local query returns a stash-owned, kind-specific checked
signature. Generic parameter identities and their value are packaged in a
`Binder`; referenced names have become symbol-backed `Ty` values; predicates
form a `CheckedParameterEnv`.

The output preserves alias types rather than eagerly erasing their identity.
Explicit and elided lifetimes currently occupy the type structure but the
destination lifetime policy is defined separately from this phase contract.

### Guarantees

Consumers may instantiate the binder, reuse the same generic identities in
detail/body queries, and type-check uses without reading the defining body.
Checked signature types never point into the source CST stash.

{spec}`crates.sage_ir.checking.signature.body_independence`

Verification intent: Query traces for local and external callers name
the selected interface facts and no bodies; a body-only edit preserves the
signature result and does not reexecute its consumers.

{spec}`crates.sage_ir.checking.signature.owner_binder`

Verification intent: Trait, impl, associated-function, field, and body
fixtures compare semantic parameter identities across queries and revisions,
including separate owner and method generic scopes.

## Entry points

Entry methods are kind-specific `sym.sig(db)` queries. A function query
demonstrates owner generics, method generics, parameter and return lowering,
and parameter-environment construction:

```{anchor}
example_fn_sig
```

Struct fields deliberately use a distinct query keyed by the same symbol:

```{anchor}
example_struct_fields
```

## Construction

A `Check` context opens the per-item CST stash read-only and allocates semantic
output in a fresh target stash. It installs owner parameters and `Self`, mints
this definition's generic parameters exactly once, resolves signature paths,
lowers types and predicates, constructs the binder, and freezes the target
stash as `Stashed<T>`.

{spec}`crates.sage_ir.checking.signature.storage`

Verification intent: Structural snapshots traverse representative
signatures after the parser and source stash borrows have ended, cover
retained aliases and predicates, and detect any cross-stash pointer.

Associated items first receive stable owner-linked symbols from the trait or
impl item query. Querying one item's signature then opens the owner binder and
adds method-level generics; it does not enumerate or check sibling bodies.

{spec}`crates.sage_ir.checking.signature.narrow_detail`

Verification intent: Query traces select one field or associated item
and reject reads of unselected sibling signatures and all bodies; edit tests
show that a detail-only change does not remint the owner binder or invalidate
unrelated detail queries.

## Failure and terminal incompleteness

An unresolved path, malformed type, unsupported signature form, unavailable
external metadata fact, or unsupported solver-relevant predicate prevents the
full successful guarantee. Checked output can retain `Ty::Error` and mark a
parameter environment solver-ineligible so downstream code recovers without
claiming a semantic proof.

This result is terminal for the current input. More polling does not make an
unsupported signature eligible. External metadata unavailability is likewise
distinguished from an ordinary type error where the relevant query exposes
that distinction.

This is the signature-phase application of
[Incompleteness is an explicit terminal outcome](../../../rfds/architectural-foundations/README.md#incompleteness-is-an-explicit-terminal-outcome).

## Incremental dependencies

The query key is the symbol. A local signature query may read its signature
CST, owner binder, referenced symbols and signatures, and keyed external
metadata. It must not read its own body or any callee body.

Editing only a function body must preserve the signature result. Editing one
field should invalidate the field query without necessarily reminting the
struct's generic binder. An unchanged checked stash can be backdated after
reexecution because allocation and hashing are deterministic.

## Worked example

For `fn first<T>(pair: Pair<T>) -> T`, signature checking resolves `Pair` to
its struct symbol and the path `T` to the function's own generic parameter:

```text
params = [Ty::Adt(Pair, [Ty::Param(T_first)])]
return = Ty::Param(T_first)
```

The parameter called `T` on `Pair` is a different semantic identity. It is
related to `T_first` only when `Pair<T_first>` is instantiated. The complete
walkthrough is [Function body and field access](../../examples/function-body.md).

## Code map

| Path | Responsibility |
|---|---|
| `local_syms/*.rs` | kind-specific signature and interface-detail queries |
| `check/sig.rs` | two-stash signature-lowering context |
| `check/trait_env.rs` | predicate lowering and solver eligibility |
| `cst/ty.rs`, `cst/generics.rs`, `cst/paths.rs` | syntax-directed checking methods |
| `ty.rs` | checked signatures, binders, types, and parameter environments |
| `external_syms.rs` | keyed lowering of external signature metadata |

## Implementation and test evidence

The generated [anchor pages](../../anchor/crates/sage_ir/checking/signature.html) report implementation entries,
annotated tests, explicit TODO/N/A gaps, and saved claim/test reviews. These
associations do not themselves establish that a test ran.

The [earlier coverage snapshot](../../../rfds/implementation-sequencing/coverage-history.md#signature-checking)
preserves the former capability descriptions, edit experiments, and limitations
as historical evidence. Current coverage comes from the generated entries;
implementation plans and decision rationale belong in the relevant RFDs.
