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

Associated items first receive stable owner-linked symbols from the trait or
impl item query. Querying one item's signature then opens the owner binder and
adds method-level generics; it does not enumerate or check sibling bodies.

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
walkthrough is [Function body and field access](../examples/function-body.md).

## Code map

| Path | Responsibility |
|---|---|
| `local_syms/*.rs` | kind-specific signature and interface-detail queries |
| `check/sig.rs` | two-stash signature-lowering context |
| `check/trait_env.rs` | predicate lowering and solver eligibility |
| `cst/ty.rs`, `cst/generics.rs`, `cst/paths.rs` | syntax-directed checking methods |
| `ty.rs` | checked signatures, binders, types, and parameter environments |
| `external_syms.rs` | keyed lowering of external signature metadata |

## Current status

### Current frontier

The represented local item kinds and external interfaces required by the
completed `DbDropGuard::db` and `Parse::next` slices have checked,
symbol-keyed signatures. Associated items reuse owner binders; local and
external predicates needed by those slices are retained.

### Implemented capabilities and evidence

- **Binder identity and field separation.** The [struct-signature
  walkthrough](../examples/struct-signature.md) follows `sig` and `fields`
  through separate stashes.
- **Owner and method generics.**
  `trait_and_impl_signatures_share_complete_generic_binders` and
  `associated_items_reuse_owner_generics_and_bodies_resolve_fields` verify
  owner-linked identities.
- **Parameter environments.**
  `function_and_adt_signatures_retain_parameter_environments` checks retained
  bounds on callable and ADT interfaces.
- **Narrow reusable external dependency.**
  `external_adt_default_and_predicate_have_one_narrow_reusable_dependency`
  inspects cold and warm query/metadata logs.

### Current limitations

- Rust signature coverage and external definition-kind coverage are partial.
- Local `Check` collects signature diagnostics internally, but the current
  `Stashed` signature results do not publish that diagnostic vector as part of
  a unified phase result.
- Every lifetime form lowers to `Lifetime::Dummy`; borrow checking is omitted.
- Some unsupported predicates are retained only as solver-ineligible
  environments, preventing definite trait conclusions.
- Const-call completeness and several advanced forms, including GATs and
  general opaque normalization, are not implemented.

### Related roadmap slices

- [Trait-partitioned impl
  discovery](../../implementation/roadmap.md#planned-slice-trait-partitioned-impl-discovery)
  will narrow a major signature-to-solver dependency.
- [Shutdown::recv](../../implementation/roadmap.md#next-application-slice-shutdownrecv)
  is the next application slice expected to extend interface coverage.
- [Semantic inspector and persistent edit
  testing](../../implementation/roadmap.md#planned-slice-semantic-inspector-and-persistent-edit-testing)
  will make signature output and edit behavior directly inspectable.
