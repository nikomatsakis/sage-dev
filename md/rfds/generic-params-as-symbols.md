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

Each generic parameter (type, lifetime, const) declared on an item becomes a salsa tracked struct:

```rust
#[salsa::tracked]
pub struct GenericParamSymbol<'db> {
    pub name: Name<'db>,
    pub kind: GenericParamKind,
    /// The item this parameter belongs to.
    pub parent: Symbol<'db>,
    /// Position in the parent's parameter list.
    pub index: u32,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub enum GenericParamKind {
    Type,
    Lifetime,
    Const,
}
```

These are created during item lowering (the same phase that currently builds `BoundVarInfo` lists). They are stable salsa identities — two references to the same `T` always refer to the same tracked struct, regardless of where the reference appears.

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
    /// A reference to a generic parameter (universal variable).
    /// Replaces BoundVar for signatures and resolved types.
    Param(GenericParamSymbol<'db>),

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
    Param(GenericParamSymbol<'db>),
    Static,
    Erased,
}
```

### Signatures without binders

Signatures are plain structs — no `Binder<T>` wrapper:

```rust
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, AllocStashData)]
pub struct FnSig<'db> {
    /// The generic parameters declared on this function.
    pub generics: Slice<GenericParamSymbol<'db>>,
    pub params: Slice<Ty<'db>>,
    pub ret: Ptr<Ty<'db>>,
}
```

Example: `fn identity<T>(x: T) -> T` becomes:

```
FnSig {
    generics: [GenericParamSymbol("T", Type, parent=identity)],
    params: [Ty { data: TyData::Param(GenericParamSymbol("T", ...)) }],
    ret: Ptr<Ty> → TyData::Param(GenericParamSymbol("T", ...)),
}
```

### Substitution on instantiation

When calling a generic function, you substitute its `Param` symbols with concrete types (or fresh inference variables). This happens when importing a foreign signature into the local stash:

```rust
struct Substitute<'db> {
    source: &Stash,
    target: &mut Stash,
    /// Maps generic param symbols to their substituted types.
    subst: FxHashMap<GenericParamSymbol<'db>, Ptr<Ty<'db>>>,
}

impl TyFolder for Substitute<'_> {
    fn fold_ty(&mut self, ty: Ty<'db>) -> Ty<'db> {
        match ty.data {
            TyData::Param(sym) => {
                match self.subst.get(&sym) {
                    Some(&replacement) => self.target[replacement],
                    None => ty,  // not in scope of this substitution (e.g., outer impl param)
                }
            }
            _ => default_fold_ty(self, ty),
        }
    }
}
```

For the type-checking setup, the function's *own* generics are left as `Param` nodes (they're universals in scope). Only when *calling* another function do you substitute its params with your locals.

### No binder shifting, ever

Since types reference symbols directly:
- Entering a closure doesn't shift anything — the outer `T` is still `Param(same_symbol)`
- The stash `Ptr` for `Param(T)` is stable across all nesting depths
- The egraph can use stash `Ptr`s as keys without worrying about de Bruijn drift

### Scope and visibility

The type checker maintains an environment that tracks which `GenericParamSymbol`s are in scope:

```rust
struct TypeEnv<'db> {
    /// All generic params visible at this point, from outermost to innermost.
    /// Used for name resolution during signature lowering.
    in_scope: Vec<GenericParamSymbol<'db>>,
}
```

This replaces the de Bruijn binder stack. Shadowing is impossible — if two items both have a `T`, they have *different* `GenericParamSymbol` tracked structs.

### Higher-ranked types (`for<'a>`)

Higher-ranked types (e.g., `for<'a> fn(&'a u32) -> &'a u32`) introduce anonymous parameters that aren't attached to any item. These get their own tracked structs:

```rust
#[salsa::tracked]
pub struct HigherRankedParam<'db> {
    pub kind: GenericParamKind,
    /// The syntactic location where this was introduced.
    pub span: AbsoluteSpan,
}
```

Or they could be a variant of `GenericParamSymbol` with a different `parent`. The key property is that each `for<'a>` in the source creates a distinct tracked struct, so two `for<'a>` at different locations never collide.

This is deferred — higher-ranked types are not needed for the initial type elaboration milestone.

## Migration from current representation

### What changes

| Current | New |
|---------|-----|
| `BoundVar { binder_index, param_index }` | `Param(GenericParamSymbol)` |
| `Binder<T> { bound_vars, value }` | Plain `T` with `generics: Slice<GenericParamSymbol>` |
| `BoundVarInfo { kind }` | `GenericParamSymbol` (tracked struct with name, kind, parent, index) |
| `Instantiate` folder (substitutes de Bruijn) | `Substitute` folder (substitutes symbols) |
| `Lifetime::BoundVar(BoundVar)` | `Lifetime::Param(GenericParamSymbol)` |

### What is deleted

- `BoundVar` struct
- `Binder<T>` struct and its `unsafe impl StashData`
- `BoundVarInfo`, `BoundVarKind`
- The de Bruijn shifting logic in `TyFolder`
- `Instantiate` folder (replaced by `Substitute`)

### What stays the same

- `FnSig`, `StructSig`, `EnumSig`, `FieldSig`, `VariantSig` — same fields minus the `Binder` wrapper, plus a `generics` field
- `TyFolder` trait — still used, just doesn't need shift/binder-depth tracking
- `SigLowerCtx` — instead of tracking a de Bruijn depth, it holds the current item's `GenericParamSymbol`s and resolves names to them
- Stash hash-consing — unchanged; `Param(symbol)` hashes by the symbol's salsa id

## Implementation plan

### Step 1: Create `GenericParamSymbol`

Add the tracked struct. Create instances during item lowering (where `BoundVarInfo` is currently created). Wire them into the existing `GenericParam` AST processing.

### Step 2: Add `TyData::Param` variant

Add the variant. Initially unused — existing code still produces `BoundVar`.

### Step 3: Migrate `SigLowerCtx` to produce `Param` instead of `BoundVar`

When lowering `TypeRefAst` → `Ty`, resolve generic parameter names to their `GenericParamSymbol` and emit `TyData::Param(sym)` instead of `TyData::BoundVar(...)`.

### Step 4: Remove `Binder<T>` from signature queries

Change `fn_signature` etc. from returning `Stashed<Binder<FnSig>>` to returning `Stashed<FnSig>` (with a `generics` field on `FnSig`). Update all callers.

### Step 5: Replace `Instantiate` with `Substitute`

The new folder maps `GenericParamSymbol → Ty` instead of `BoundVar → Ty`. Update call sites that instantiate signatures.

### Step 6: Delete dead code

Remove `BoundVar`, `Binder<T>`, `BoundVarInfo`, `BoundVarKind`, the old `Instantiate` folder, and any de Bruijn shifting logic.

## Open questions

1. **`GenericParamSymbol` vs. reusing `Symbol`.** Should generic params be a variant of the existing `Symbol`/`SymbolData` enum, or a separate tracked struct? Using `Symbol` is more uniform but adds a new kind to an already-complex enum. A separate struct is cleaner but means two kinds of "named thing" in the system.

2. **External crate generics.** For types imported from rustc metadata (via `TcxDb`), how do we create `GenericParamSymbol`s? Presumably on-demand when we first encounter the foreign item's signature, cached via salsa.

3. **Const generics.** `Const::Other(Symbol<'db>)` in the current design uses a Symbol for named consts. Should const generic parameters also use `GenericParamSymbol`, or does `Const` need its own parallel treatment?

4. **`where` clauses.** Where-clauses reference generic params. With symbols this is straightforward (`T: Foo` references `GenericParamSymbol("T")`), but the representation of where-clauses themselves isn't yet designed.
