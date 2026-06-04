# RFD: Generic Parameters as Symbols

**Status:** Draft

**Depends on:**
- [Type Signatures](./type-signatures.md) — current `Ty`, `Binder`, `BoundVar` representation
- [Per-kind symbol data](./per-kind-symbol-data.md) — `Symbol`, `SymbolData`, per-kind wrappers

**Depended on by:**
- [Type Inference](./type-inference.md) — assumes this representation for the elaboration engine

## Goal

Replace de Bruijn-indexed bound variables (`BoundVar`, `Binder<T>`) with direct `Symbol<'db>` references in type representations. Each generic parameter becomes a salsa tracked struct, and types reference it directly. This eliminates binder-related machinery and simplifies the path into type inference.

## Motivation

The current representation uses de Bruijn indices:

```rust
// Current: fn identity<T>(x: T) -> T
Binder {
    bound_vars: [BoundVarInfo { kind: Type }],
    value: FnSig {
        params: [Ty { data: TyData::BoundVar(BoundVar { binder_index: 0, param_index: 0 }) }],
        ret: Ptr<Ty> → TyData::BoundVar(BoundVar { binder_index: 0, param_index: 0 }),
    },
}
```

This has several problems for the type inference layer:

1. **Shifting.** When you enter a new scope (e.g., a closure), all existing de Bruijn indices must be shifted. This means the same logical type parameter has different `TyData` representations at different nesting depths — making it impossible to use the stash `Ptr` as a stable identity for egraph keys.

2. **Opening binders.** Before type-checking, you must substitute bound variables with free variables. This is a whole-tree traversal just to get started.

3. **Shadowing ambiguity.** Two `T`s at different scopes are distinguished only by de Bruijn depth, which is fragile and error-prone.

4. **No reuse across queries.** A `Binder<FnSig>` can't be directly used by a caller — it must always be instantiated first.

## Design

### Generic parameters as tracked structs

Each generic parameter becomes a salsa identity. The `GenericParam` enum splits by **origin** (ast/ext/anon) — each variant is a distinct salsa struct that stores a `kind` field indicating whether it's a type, lifetime, or const parameter:

```rust
/// A generic parameter. Split by origin; each variant carries its own `kind`.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub enum GenericParam<'db> {
    /// From local source — created during item lowering.
    Ast(AstGenericParam<'db>),
    /// From external crate metadata — interned on first encounter.
    Ext(ExtGenericParam<'db>),
    /// Canonical placeholder — used for alpha-equivalence testing.
    AlphaEquiv(AlphaEquivParam<'db>),
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub enum GenericParamKind { Type, Lifetime, Const }

impl<'db> GenericParam<'db> {
    pub fn kind(&self, db: &'db dyn Db) -> GenericParamKind {
        match self {
            Self::Ast(p) => p.kind(db),
            Self::Ext(p) => p.kind(db),
            Self::AlphaEquiv(p) => p.kind(db),
        }
    }
}
```

All three variants are identity structs that store a `kind` plus some form of name. They differ only in how they're created and what naming info they carry:

**Ast params** are `#[salsa::tracked]` — created during item lowering from AST nodes. They carry a user-given name:

```rust
#[salsa::tracked]
pub struct AstGenericParam<'db> {
    pub kind: GenericParamKind,
    pub name: Option<Name<'db>>,
    pub parent: Symbol<'db>,
}
```

**Ext params** are `#[salsa::interned]` — created on-demand when importing foreign crate signatures via `TcxDb`. They also carry a name (from the metadata):

```rust
#[salsa::interned]
pub struct ExtGenericParam<'db> {
    pub kind: GenericParamKind,
    pub name: Option<Name<'db>>,
    pub parent: Symbol<'db>,
}
```

**AlphaEquiv params** are `#[salsa::interned]` — used for alpha-equivalence testing. Two `AlphaEquivParam`s with the same kind and index are intentionally the *same* identity (that's the point — they provide a canonical set of placeholders). They never appear in stored signatures; they're used transiently when substituting both signatures' real params with canonical ones and comparing the results for structural equality:

```rust
#[salsa::interned]
pub struct AlphaEquivParam<'db> {
    pub kind: GenericParamKind,
    pub index: u32,
}
```

**Display.** `GenericParam` impls `Display`:
- Params with a name print it (e.g., `T`, `'a`, `N`)
- Params without a name fall back to kind + index (e.g., `_0`, `'_1`, `_2`)

Generic params participate in the `Symbol` infrastructure. `AstGenericParam` is a `SymbolData` variant (it has a name and parent, so name resolution, `Symbol::name()`, etc. work naturally):

```rust
enum SymbolData<'db> {
    // ... existing variants ...
    GenericParam(AstGenericParam<'db>),
}
```

Ast params are created during item lowering (the same phase that currently builds `BoundVarInfo` lists). Ext params are interned on first encounter when importing foreign item generics from rustc metadata. AlphaEquiv params are interned on-demand for alpha-equivalence checks. All are stable salsa identities — two references to the same parameter always resolve to the same struct.

### Types reference symbols directly

```rust
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, AllocStashData)]
pub enum TyData<'db> {
    // --- primitives (unchanged) ---
    Bool, Char, Int(IntTy), Uint(UintTy), Float(FloatTy), Str,

    // --- compound (unchanged) ---
    Adt(Symbol<'db>, Slice<Ty<'db>>),
    Ref(Ptr<Ty<'db>>, Mutability, Lifetime<'db>),
    Tuple(Slice<Ty<'db>>),
    Slice(Ptr<Ty<'db>>),
    Array(Ptr<Ty<'db>>, Const<'db>),
    FnPtr(Slice<Ty<'db>>, Ptr<Ty<'db>>),

    // --- variables ---
    /// A reference to a generic type parameter (universal variable).
    /// Replaces BoundVar for signatures and resolved types.
    /// Invariant: param.kind() == Type.
    Param(GenericParam<'db>),

    /// An inference variable (existential). Only appears during type elaboration.
    FreeVar(FreeVarIndex),

    // --- other ---
    Never,
    Error,
}
```

`Lifetime` similarly changes:

```rust
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, AllocStashData)]
pub enum Lifetime<'db> {
    /// Invariant: param.kind() == Lifetime.
    Param(GenericParam<'db>),
    Static,
    Erased,
}
```

### `Binder<T>` carries symbols

We retain the `Binder<T>` wrapper, but instead of de Bruijn `BoundVarInfo` it carries a `Slice<GenericParam<'db>>`. The `Parametric` trait avoids a redundant `'db` parameter on `Binder` (since `T` already carries it):

```rust
/// Trait for types that can appear inside a `Binder`.
/// Associates the param type so `Binder`
/// doesn't need its own lifetime parameter.
trait Parametric {
    type Param;
}

impl<'db> Parametric for FnSig<'db> {
    type Param = GenericParam<'db>;
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, AllocStashData)]
pub struct Binder<T: Parametric> {
    /// The generic parameters bound by this binder.
    pub generics: Slice<T::Param>,
    pub value: T,
}
```

Signatures themselves stay clean — no `generics` field:

```rust
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, AllocStashData)]
pub struct FnSig<'db> {
    pub params: Slice<Ty<'db>>,
    pub ret: Ptr<Ty<'db>>,
}
```

Example: `fn identity<T>(x: T) -> T` becomes:

```
Binder {
    generics: [GenericParam::Ast(AstGenericParam { kind: Type, name: "T", parent: identity, index: 0 })],
    value: FnSig {
        params: [Ty { data: TyData::Param(GenericParam::Ast(...)) }],
        ret: Ptr<Ty> → TyData::Param(GenericParam::Ast(...)),
    },
}
```

The `Binder` serves the same structural role as before (marking "this signature is parameterized") but the bound variables are now stable symbol references rather than positional indices. Callers that need to instantiate the signature substitute the `generics` symbols; callers that just need the shape (e.g., arity checks) can look through the binder without any opening ceremony.

### Substitution on instantiation

When calling a generic function, you substitute its `Param` symbols with concrete types (or fresh inference variables). This happens when importing a foreign signature into the local stash:

```rust
struct Substitute<'db> {
    source: &Stash,
    target: &mut Stash,
    /// Maps generic params to their substituted types/lifetimes.
    /// For type params: maps to a Ptr<Ty>.
    /// For lifetime params: maps to a Lifetime.
    subst: FxHashMap<GenericParam<'db>, SubstTarget<'db>>,
}

enum SubstTarget<'db> {
    Ty(Ty<'db>),
    Lifetime(Lifetime<'db>),
    Const(Const<'db>),
}

impl TyFolder for Substitute<'_> {
    fn fold_ty(&mut self, ty: Ty<'db>) -> Ty<'db> {
        match ty.data {
            TyData::Param(param) => {
                match self.subst.get(&param) {
                    Some(SubstTarget::Ty(t)) => *t,
                    _ => ty,  // not in scope of this substitution (e.g., outer impl param)
                }
            }
            _ => default_fold_ty(self, ty),
        }
    }

    fn fold_lifetime(&mut self, lt: Lifetime<'db>) -> Lifetime<'db> {
        match lt {
            Lifetime::Param(param) => {
                match self.subst.get(&param) {
                    Some(SubstTarget::Lifetime(l)) => *l,
                    _ => lt,
                }
            }
            _ => lt,
        }
    }
}
```

For the type-checking setup, the function's *own* generics are left as `Param` nodes (they're universals in scope). Only when *calling* another function do you substitute its params with your locals.

Note that `TyData::Param`, `Lifetime::Param`, and `Const::Param` are all leaf nodes — the default `fold_ty`/`fold_lifetime`/`fold_const` passes them through unchanged. Only folders that specifically need to substitute (like `Substitute`) override for them.

### No binder shifting, ever

Since types reference symbols directly:
- Entering a closure doesn't shift anything — the outer `T` is still `Param(same_symbol)`
- The stash `Ptr` for `Param(T)` is stable across all nesting depths
- The egraph can use stash `Ptr`s as keys without worrying about de Bruijn drift

**Tradeoff: alpha-equivalence.** Structural equality of types no longer implies semantic equivalence of signatures — `fn identity<T>(T) -> T` from two different items will have different `GenericParam`s. To test alpha-equivalence, substitute both signatures' params with shared `AlphaEquivParam`s (by position) and compare the results structurally.

### Scope and visibility

The type checker maintains an environment that tracks which `GenericParam`s are in scope:

```rust
struct TypeEnv<'db> {
    /// All generic params visible at this point, from outermost to innermost.
    /// Used for name resolution during signature lowering.
    in_scope: Vec<GenericParam<'db>>,
}
```

This replaces the de Bruijn binder stack. Shadowing is impossible — if two items both have a `T`, they have *different* `AstGenericParam` tracked structs.

### Higher-ranked types and anonymous lifetimes

Lifetimes in function signatures are not always named. There are several cases:

- `fn(&u32)` — anonymous lifetime, no user-given name
- `fn(&'a u32)` — named lifetime introduced by an outer `for<'a>` or item-level generic
- `for<'a> fn(&'a u32) -> &'a u32` — higher-ranked with a named lifetime

All of these are handled by `AstGenericParam` with an optional name. Anonymous lifetimes (like the one in `fn(&u32)`) get `name: None` — they're still distinct tracked structs (each syntactic occurrence creates its own identity), they just don't have a user-visible name.

This is deferred — higher-ranked types are not needed for the initial type elaboration milestone.

## Migration from current representation

### What changes

| Current | New |
|---------|-----|
| `BoundVar { binder_index, param_index }` | `TyData::Param(GenericParam)` / `Lifetime::Param(GenericParam)` |
| `Binder<T> { bound_vars: Slice<BoundVarInfo>, value }` | `Binder<T> { generics: Slice<GenericParam>, value }` |
| `BoundVarInfo { kind }` | `GenericParam` enum (`Ast`/`Ext`/`AlphaEquiv` variants, each with a `kind` field) |
| `Instantiate` folder (substitutes de Bruijn) | `Substitute` folder (substitutes `GenericParam`s) |
| `Lifetime::BoundVar(BoundVar)` | `Lifetime::Param(GenericParam)` |

### What is deleted

- `BoundVar` struct
- `BoundVarInfo`, `BoundVarKind`
- The de Bruijn shifting logic in `TyFolder`
- `Instantiate` folder (replaced by `Substitute`)

### What stays the same

- `Binder<T>` — same role, now carries `Slice<GenericParam>` instead of `Slice<BoundVarInfo>`; uses a `Parametric` trait to avoid a `'db` lifetime parameter
- `FnSig`, `StructSig`, `EnumSig`, `FieldSig`, `VariantSig` — same fields, still wrapped in `Binder`
- `TyFolder` trait — still used, just doesn't need shift/binder-depth tracking
- `SigLowerCtx` — instead of tracking a de Bruijn depth, it holds the current item's `GenericParam`s and resolves names to them
- Stash hash-consing — unchanged; `Param(GenericParam)` hashes by the salsa id

## Implementation plan

### Step 1: Create `GenericParam` and `AstGenericParam`

Add `AstGenericParam` tracked struct, `ExtGenericParam` and `AlphaEquivParam` interned structs, and the `GenericParam` enum. Add a `GenericParam` variant to `SymbolData`. Create `AstGenericParam` instances during item lowering (where `BoundVarInfo` is currently created).

### Step 2: Add `TyData::Param` and `Lifetime::Param` variants

Add `TyData::Param(GenericParam)` and `Lifetime::Param(GenericParam)`. Initially unused — existing code still produces `BoundVar`.

### Step 3: Migrate `SigLowerCtx` to produce `Param` instead of `BoundVar`

When lowering `TypeRefAst` → `Ty`, resolve generic parameter names to their `AstGenericParam` and emit `TyData::Param(GenericParam::Ast(...))` instead of `TyData::BoundVar(...)`.

### Step 4: Migrate `Binder<T>` to carry `GenericParam`

Change `Binder` from storing `Slice<BoundVarInfo>` to `Slice<T::Param>` via the `Parametric` trait. Signature queries still return `Stashed<Binder<FnSig>>` etc. — the shape is the same, only the bound-variable representation changes. Update all callers that inspect the binder's variable list.

### Step 5: Replace `Instantiate` with `Substitute`

The new folder maps `GenericParam → SubstTarget` instead of `BoundVar → Ty`. Update call sites that instantiate signatures.

### Step 6: Delete dead code

Remove `BoundVar`, `BoundVarInfo`, `BoundVarKind`, the old `Instantiate` folder, and any de Bruijn shifting logic.

## Open questions

1. ~~**`GenericParamSymbol` vs. reusing `Symbol`.**~~ **Resolved:** `AstGenericParam` is a `SymbolData` variant. `GenericParam` is an enum over origin (`Ast`/`Ext`/`AlphaEquiv`), where each variant stores a `kind: GenericParamKind` field. No per-kind split at the enum level — kind is data, not structure.

2. ~~**External crate generics.**~~ **Resolved:** `ExtGenericParam` is `#[salsa::interned]` with `kind`, `name`, `parent`, `index`. Created on-demand when importing foreign signatures. `AlphaEquivParam` is also `#[salsa::interned]` with `kind` and `index` — used during signature normalization/canonicalization.

3. ~~**Const generics.**~~ **Resolved:** `Const` gets a `Const::Param(GenericParam<'db>)` variant, symmetric with `TyData::Param` and `Lifetime::Param`. Invariant: `param.kind() == Const`.

4. ~~**`where` clauses.**~~ **Not blocking:** Where-clauses will reference `GenericParam`s naturally, but their own representation is a separate design problem (future RFD). Nothing here needs to be resolved first.
