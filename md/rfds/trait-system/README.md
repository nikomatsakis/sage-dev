# RFD: Trait System

**Status:** Proposed

**Depends on:**

- [Symbol-Level Signature Queries](../symbol-signatures/README.md) — symbol-keyed signature queries
- [Per-kind symbol data](../per-kind-symbol-data/README.md) — `TraitSymbol` and `ImplSymbol`
- [Type Signatures](../type-signatures/README.md) — `Ty`, `Binder`, and type folding

## Problem

The codebase has symbols and CST nodes for traits and impls, but no checked
representation of trait references, trait bounds, trait items, or impl signatures. This
blocks lowering generic bounds, assembling clauses for trait solving, and finding methods
defined by traits and impls.

## Scope

The MVP provides the checked data consumed by the trait solver and method resolver:

- positive local trait and impl declarations;
- type-only trait arguments;
- checked type-predicate environments on trait, impl, function, and ADT
  signatures, including bounds lowered from generic-parameter syntax;
- one binder for an entire impl signature, so its self type, trait reference, and
  where-clauses share the same generic parameters;
- an explicit eligibility marker which prevents incomplete or unsupported
  signatures from becoming solver or method candidates;
- deterministic enumeration of every local impl in a `LocalCrateSymbol`.

The MVP does not perform proof search, choose a method, or check coherence. It also defers
lifetime/outlives predicates, const trait arguments, higher-ranked bounds, associated type
bindings and normalization, supertrait elaboration, external and builtin impl discovery,
auto and negative impls, specialization, and overlap/orphan checking. Unsupported syntax
must be diagnosed or represented as unsupported; it must not be silently dropped from an
impl's applicability conditions.

The type-only consumers cannot instantiate lifetime or const candidate
variables. A trait or impl whose header contains lifetime/const generics, or
whose applicability depends on any other deferred predicate form, is retained
for diagnostics and item ownership but marked `Unsupported` for solver and
method-candidate exposure. Supporting lifetime/const leaves structurally in
`Ty` does not make those binders inferable.

This gate includes implicit lifetime binders. An impl header such as
`impl<T> Trait for &T` is effectively generic over the reference lifetime and
is `Unsupported` for candidate matching; an erased lifetime must not be used as
a wildcard. A fully concrete lifetime such as `'static` may still be compared
structurally. The special elided borrow in supported `&self`/`&mut self`
receivers is represented by `MethodReceiver` and does not become an impl-head
lifetime variable.

Eligibility requires source fidelity before checked lowering. The CST/parser
must preserve every header feature which can change applicability or candidate
kind: trait supertraits and `auto`/`unsafe`, trait generic defaults, and impl
negative/default/unsafe/const markers. The MVP may keep `unsafe` positive
traits/impls eligible because safety does not change proposition truth, but a
supertrait, auto trait, generic default, negative impl, const impl, or
default/specializing impl is `Unsupported` until its semantics exist. Missing
syntax must never look like an ordinary eligible declaration.

Lowering recognizes polarity and specialization syntax before constructing the
checked signature. Negative impls, auto traits, and default/specializing impls
are marked `Unsupported` until their proof and priority rules exist; none may
be exposed through the positive MVP clause path. The MVP does not otherwise
need a checked polarity enum because ineligible signatures are never opened by
consumers.

## Design

### Trait references and predicates

`TraitRef` contains only the trait's explicit type arguments. The type implementing the
trait is stored separately in `WherePredicate` or `ImplSignatureData::self_ty`. Keeping
`Self` explicit avoids two competing argument-order conventions at the boundary with the
solver.

```rust
/// `Trait<A, B>` in a bound or impl header. MVP arguments are types only.
pub struct TraitRef<'db> {
    pub trait_sym: TraitSymbol<'db>,
    pub args: Slice<Ptr<Ty<'db>>>,
}

/// `self_ty: trait_ref`, for example `T: Iterator`.
pub struct WherePredicate<'db> {
    pub self_ty: Ptr<Ty<'db>>,
    pub trait_ref: TraitRef<'db>,
}

/// Whether the complete applicability contract can be consumed by the
/// type-only trait solver and method resolver.
pub enum SolverEligibility {
    /// Every generic and defining predicate needed by consumers is represented.
    Eligible,
    /// Some header generic or applicability predicate is outside the MVP.
    Unsupported,
}
```

Several source forms lower to the same predicate:

```text
fn f<T: Clone>()              -> T: Clone
fn f<T>() where T: Clone      -> T: Clone
impl<T: Clone> Trait for T    -> impl where-clause T: Clone
```

Associated type syntax such as `Iterator<Item = u32>` is not an ordinary positional type
argument and is deferred with projection normalization.
Because trait generic defaults are also deferred, an eligible `TraitRef` has
exactly the declared number of explicit type arguments (excluding `Self`);
lowering diagnoses missing or extra arguments rather than zipping/truncating or
inventing a fresh/default value.

### Trait signatures and items

The signature query mints a trait's generic parameters once. The items query reuses those
same `GenericParam` identities when checking item signatures. The binder starts with a
synthesized type parameter for `Self`, followed by the trait's explicit source parameters;
`TraitRef::args` corresponds only to the explicit parameters. Recording `self_param`
separately makes that split explicit when a method signature is instantiated.

```rust
pub type TraitSignature<'db> = Binder<'db, TraitSignatureData<'db>>;

pub struct TraitSignatureData<'db> {
    pub self_param: GenericParam<'db>,
    pub where_clauses: Slice<WherePredicate<'db>>,
    /// `Eligible` only when the complete defining-predicate set was lowered and
    /// every source generic is a type parameter supported by the MVP.
    pub solver_eligibility: SolverEligibility,
}

pub type TraitItems<'db> = Binder<'db, Slice<TraitItemDef<'db>>>;

pub enum TraitItemDef<'db> {
    Function(FnSymbol<'db>),
    Type(TypeAliasSymbol<'db>),
    Const(ConstSymbol<'db>),
}
```

Function signatures also preserve whether a function is a dot-call method and
which receiver form it declared:

```rust
pub enum MethodReceiver {
    Value { mutable_binding: bool }, // `self` / `mut self`
    Ref { mutability: Mutability },  // `&self` / `&mut self`
}

pub struct CheckedReceiver<'db> {
    pub owner_self_ty: Ptr<Ty<'db>>,
    pub form: MethodReceiver,
}
```

The owner binder supplies `owner_self_ty`: the trait `Self` parameter for a
trait item or the opened impl self type for an impl item. A function signature
stores `Option<CheckedReceiver>` separately from ordinary parameters, so an
associated function with no receiver cannot become a dot-call candidate.
Explicit-lifetime and typed receivers such as `&'a self` or
`self: Box<Self>` are preserved but marked unsupported by the method MVP; only
`self`, `mut self`, `&self`, and `&mut self` are lowered to the forms above.
Region matching for the reference forms is deferred with lifetime inference;
the receiver form, not a fabricated lifetime variable, drives autoref lookup.

An owner-item function signature also records
`method_candidate_eligibility: SolverEligibility`. It is `Eligible` only when
the receiver/associated-function form is supported, every method-level generic
can be instantiated by the type-only method resolver, the complete function
`CheckedParameterEnv` is eligible, and parameter/return types contain no
deferred projection or other unsupported form. This is a method-consumer gate,
not a claim that an ordinary free function with lifetime syntax cannot be
represented for other checking purposes.

Type and const items are recorded so item identity is complete, but associated type and
associated const semantics remain deferred. A function item symbol retains its owning
trait or impl. Its signature query opens the owner's binder before minting and opening the
function's own generics; module scope alone is not enough to resolve `Self` or an impl
generic used by a method signature.

### Impl signatures and items

An impl has exactly one binder. Opening it once creates one substitution used throughout
the impl header and where-clauses. In particular, both occurrences of `T` below retain the
same identity:

```rust
impl<T: Clone> Trait<T> for Pair<T, T> { /* ... */ }
```

```rust
pub type ImplSignature<'db> = Binder<'db, ImplSignatureData<'db>>;

pub struct ImplSignatureData<'db> {
    /// `None` for an inherent impl.
    pub trait_ref: Option<TraitRef<'db>>,
    pub self_ty: Ptr<Ty<'db>>,
    pub where_clauses: Slice<WherePredicate<'db>>,
    /// Applies to both trait-clause exposure and type-only method-candidate
    /// instantiation. Inherent impls can therefore also be unsupported.
    pub solver_eligibility: SolverEligibility,
}

pub type ImplItems<'db> = Binder<'db, Slice<TraitItemDef<'db>>>;
```

`ImplItems` uses the same checked impl generics as `ImplSignature`. Method-level generics
remain owned by each function signature.

### Function and ADT parameter environments

The same `WherePredicate` lowering is used by non-trait signatures. Function
and ADT signature data store their type predicates inside the same binder as
the generics and types those predicates mention. This replaces the current
always-empty/planned where-clause slots in the Symbol Signatures design.

```rust
pub struct CheckedParameterEnv<'db> {
    pub where_clauses: Slice<WherePredicate<'db>>,
    pub solver_eligibility: SolverEligibility,
}
```

`FnSignature` and `AdtSignature` embed `CheckedParameterEnv` under their owner
binder; trait and impl signatures expose the equivalent two fields directly.

When checking a function body, the body checker opens the function signature
and adds its eligible type predicates to the solver environment. A method body
also inherits the opened predicates of its owning trait or impl. A trait method
body additionally receives the owner fact
`Self: ThisTrait<opened trait args>`; a trait-impl method body receives the
opened impl-head fact. Those facts are valid under the corresponding owner
predicates and let one method call another without recursively rediscovering
its own abstract owner impl. An inherent impl contributes predicates but no
trait-head fact.

At use sites, an ordinary function call instantiates the callee function's
parameter environment, and ADT construction/use instantiates the ADT
environment as well-formedness obligations. Silently discarding either would
make a generic call or `Container<T>` appear usable without its declared
bounds.

An owner with an unsupported lifetime, const, higher-ranked, or projection
predicate is marked unsupported and diagnosed. The MVP may preserve any
independently represented positive type predicates for diagnostics, but it may
not finalize that owner as successfully checked while the rest of its
parameter environment is missing.

Opening an eligible body parameter environment also elaborates the defining
where-predicates of each referenced local trait. For example, if
`Trait<U>` declares `where U: Bound`, an assumption `T: Trait<U>` adds the
instantiated `U: Bound` fact. Elaboration uses a deduplicating worklist to a
fixed point so recursive defining predicates terminate. This is distinct from
supertrait elaboration, which remains deferred. A trait whose complete defining
predicates are unavailable makes that environment unsupported rather than
contributing an incomplete set of facts.

### Symbol queries

Queries are keyed by the symbol that owns the checked data. Local queries are the MVP;
external symbols will use the `TcxDb` metadata boundary later.

```rust
impl<'db> LocalTraitSym<'db> {
    #[salsa::tracked]
    pub fn sig(self, db: &'db dyn Db) -> Stashed<TraitSignature<'db>>;

    #[salsa::tracked]
    pub fn items(self, db: &'db dyn Db) -> Stashed<TraitItems<'db>>;
}

impl<'db> LocalImplSym<'db> {
    #[salsa::tracked]
    pub fn sig(self, db: &'db dyn Db) -> Stashed<ImplSignature<'db>>;

    #[salsa::tracked]
    pub fn items(self, db: &'db dyn Db) -> Stashed<ImplItems<'db>>;
}
```

The signature query is the canonical minting point for generics. A trait-items
query first loads the trait signature, opens its binder into the checking
scope, and binds `Self` to `Ty::Param(self_param)`. An impl-items query opens the
impl signature's binder and binds `Self` to that opening's substituted
`self_ty`. Both then check their items without reminting owner generics.

### Per-crate impl enumeration

The solver and method resolver need a complete, deterministic set of local impl symbols.
The crate, not a `SourceRoot`, is the semantic key:

```rust
#[salsa::tracked(returns(ref))]
pub fn local_impls<'db>(
    db: &'db dyn Db,
    krate: LocalCrateSymbol<'db>,
) -> Vec<LocalImplSym<'db>>;
```

`local_impls` walks the expanded module tree rooted at `krate.root_mod(db)`, including
inline modules and macro-produced impls, and returns each impl once in deterministic module
and item order. The MVP consumers linearly scan this list:

- the trait solver keeps positive trait impls whose `trait_ref.trait_sym` matches the fixed
  trait goal, whose impl and referenced local-trait signatures are both
  `SolverEligibility::Eligible`, then opens each impl binder freshly;
- inherent method lookup keeps impls with `trait_ref: None`, a matching self-type
  head, and an `Eligible` type-only opening;
- trait method discovery enumerates traits separately and asks the solver a
  fixed post-deref `LookupSelfTy: Trait<Args>` question. The solver does not
  return an impl or discover a trait by method name.

An index by trait and self-type head may replace the scan later without changing the
checked signature model.

An `Unsupported` signature is not equivalent to an empty predicate set. A
consumer which encounters a potentially relevant unsupported trait or impl
marks its candidate source incomplete (and ultimately reports the earlier
unsupported-feature diagnostic); it must not expose an unconditional clause or
conclude `NotFound`/`No` from the remaining subset. Local impls of external
traits are likewise ineligible until metadata supplies the external trait's
complete defining predicates.

## Deferred work

- External trait and impl signatures and per-crate enumeration through `TcxDb`.
- Exposing local impls of external traits to the solver; this requires the
  external trait's checked defining predicates rather than assuming none.
- Lifetime/outlives and const predicates.
- Solver and method-candidate exposure for lifetime/const-generic trait and impl
  headers.
- Associated type bindings, projections, normalization, and associated item values.
- Higher-ranked bounds and universe-bearing binders.
- Supertrait elaboration and implied predicates.
- Builtin, auto, and negative impls.
- Coherence, orphan rules, overlap, and specialization.
