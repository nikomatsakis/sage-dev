# RFD: Trait System

**Status:** Proposed

**Depends on:**
- [Symbol-Level Signature Queries](./symbol-signatures.md) — `ScopeSymbol`, `AdtSignature`, symbol-keyed queries
- [Per-kind symbol data](./per-kind-symbol-data.md) — `TraitSymbol`, `ImplSymbol`

## Problem

The codebase currently has no representation of traits, trait bounds, or where-clauses in the type system. `TraitSymbol` and `ImplSymbol` exist as symbol wrappers but have no associated signature or content queries. This blocks:

1. **Where-clause lowering** — `AdtSignature` and `FnSignature` define slots for `Slice<WherePredicate<'db>>` but there is no `WherePredicate` type or lowering logic.
2. **Trait bounds on generics** — `fn foo<T: Clone>(x: T)` cannot be represented or checked.
3. **Trait method resolution** — method calls on bounded type parameters cannot be resolved.
4. **Impl dispatch** — determining which impl satisfies a trait bound.

## Scope

This RFD covers the data model for traits, impls, and where-clauses. It does NOT cover the trait *solver* (obligation discharge, coherence checking) — that is a separate concern built on top of these representations.

## Design

### Types

```rust
/// A where-predicate: `T: Trait<Args>` or `T: 'a`.
pub struct WherePredicate<'db> {
    pub ty: Ptr<Ty<'db>>,
    pub bound: Bound<'db>,
}

pub enum Bound<'db> {
    Trait(TraitRef<'db>),
    Lifetime(Lifetime),
}

/// A reference to a trait with type arguments: `Clone`, `Iterator<Item = u32>`.
pub struct TraitRef<'db> {
    pub trait_sym: TraitSymbol<'db>,
    pub args: Slice<Ptr<Ty<'db>>>,
}
```

### Trait signature and items

```rust
pub struct TraitSignature<'db> {
    pub where_clauses: Binder<'db, Slice<WherePredicate<'db>>>,
}

pub struct TraitItems<'db> {
    pub items: Binder<'db, Slice<TraitItemDef<'db>>>,
}

pub enum TraitItemDef<'db> {
    Function(FnSymbol<'db>),
    Type(TypeAliasSymbol<'db>),
    Const(ConstSymbol<'db>),
}
```

### Impl signature

```rust
pub struct ImplSignature<'db> {
    /// The trait being implemented (None for inherent impls).
    pub trait_ref: Binder<'db, Option<TraitRef<'db>>>,
    /// The self type.
    pub self_ty: Binder<'db, Ptr<Ty<'db>>>,
    pub where_clauses: Binder<'db, Slice<WherePredicate<'db>>>,
}
```

### Queries

```rust
fn trait_signature(db, sym: TraitSymbol, source_root: SourceRoot) -> Stashed<TraitSignature>;
fn trait_items(db, sym: TraitSymbol, source_root: SourceRoot) -> Stashed<TraitItems>;
fn impl_signature(db, sym: ImplSymbol, source_root: SourceRoot) -> Stashed<ImplSignature>;
```

## Open questions

- How do associated types interact with `WherePredicate`? (e.g., `where I: Iterator<Item = u32>`)
- Should supertraits (`trait Foo: Bar`) be stored on `TraitSignature` or as implicit where-clauses?
- What is the indexing strategy for finding impls of a given trait? (needed for dispatch but may belong in a separate solver RFD)
