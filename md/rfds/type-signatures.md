# RFD: Type Signatures

**Status:** Draft

**Depends on:**
- [Module sym tree](./module-sym-tree.md) — `Symbol`, `ModSymbol`, `ItemAst`, stash architecture
- [Relative span model](./relative-span-model.md) — `RelativeSpan` on signature types
- [Tuple struct constructors](./tuple-struct-ctors.md) — `TupleStructCtor` memmap entry and `SymbolData` variant

**Depended on by:**
- [Per-kind symbol data](./per-kind-symbol-data.md) — `SymbolData` enum migration using the per-kind wrappers defined here

## Goal

Introduce the `Ty` type representation, per-kind symbol wrappers (`FnSymbol`, `StructSymbol`, ...), and `signature` queries that produce resolved type information for items. This is the foundation for type checking — everything downstream (inference, field/method resolution, trait solving) consumes signatures.

## Current state

Today sage resolves *names* but not *types*. `TypeRef` is syntactic — `HashMap<K, V>` is just `Path { segments: ["HashMap"] }` with the `<K, V>` arguments discarded. Generic parameter lists (`<T, U>`) are not parsed at all. The AST items (`FnAst`, `StructAst`, etc.) carry `TypeRef`s for their signatures but nothing resolved.

`Symbol<'db>` is a flat wrapper over `SymbolData { Ast(ItemAst), TupleStructCtor(StructAst), Ext(SymExt) }`. The `TupleStructCtor` variant was added by the [tuple struct constructors RFD](./tuple-struct-ctors.md). There are no per-kind wrappers like `FnSymbol` or `StructSymbol` — callers branch on `ItemAst::Function(_)` directly. The RFD for module-sym-tree deferred these to a later milestone. `StructAst` has a `kind: StructKind` tracked field distinguishing `Tuple`, `Unit`, and `Braced` structs.

## Design overview

The work has four layers:

1. **AST: parse generic params and type args** — teach the lowering to capture `<T, U>` on items and type arguments on paths.
2. **`Ty` representation** — stash-allocated resolved types with `Binder` and de Bruijn indexing.
3. **Per-kind symbol wrappers** — `FnSymbol`, `StructSymbol`, etc., following the `ModSymbol` pattern.
4. **Signature queries** — `fn_signature`, `struct_signature`, etc., each lowering from `TypeRef` → `Ty`.

Plus a cross-cutting piece: the **`TyFolder`** trait for cross-stash type mapping, with binder instantiation as the first concrete use.

## Layer 0: AST changes — unified signature stash

### One stash per item for all signature data

Today, item signatures are spread across individual salsa-tracked fields and salsa-tracked structs: `FnAst` has tracked fields `params: Vec<Param<'db>>`, `ret_type: Option<TypeRef<'db>>`, etc., where `Param`, `TypeRef`, `Path`, `FieldDef`, and `VariantDef` are each their own `#[salsa::tracked]` struct. This gives salsa fine-grained change tracking per field, but that granularity is overkill — signature edits are rare, and re-lowering a signature is cheap.

The new model: each item kind gets a **single `Stashed<...>` for its entire syntactic signature**. Generic params, parameter names and types, return types, field definitions — everything lives in one stash. The item's `*Ast` struct holds this as a single tracked field.

```rust
// Function signature — all syntactic data in one stash
pub type FnSigAst<'db> = Stashed<Ptr<FnSigAstData<'db>>>;

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, AllocStashData)]
pub struct FnSigAstData<'db> {
    pub generics: Slice<GenericParam<'db>>,
    pub params: Slice<ParamAst<'db>>,
    pub ret_type: Option<Ptr<TypeRefAst<'db>>>,
}

// Struct signature
pub type StructSigAst<'db> = Stashed<Ptr<StructSigAstData<'db>>>;

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, AllocStashData)]
pub struct StructSigAstData<'db> {
    pub generics: Slice<GenericParam<'db>>,
    pub fields: Slice<FieldDefAst<'db>>,
}

// Enum signature
pub type EnumSigAst<'db> = Stashed<Ptr<EnumSigAstData<'db>>>;

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, AllocStashData)]
pub struct EnumSigAstData<'db> {
    pub generics: Slice<GenericParam<'db>>,
    pub variants: Slice<VariantDefAst<'db>>,
}
```

Items hold these as tracked fields:

```rust
#[salsa::tracked(debug)]
pub struct FnAst<'db> {
    pub name: Name<'db>,
    #[tracked] #[returns(ref)] pub signature: FnSigAst<'db>,
    #[tracked] #[returns(ref)] pub body: FunctionBody<'db>,
    #[tracked] #[returns(ref)] pub attrs: Vec<Attr<'db>>,
    #[tracked] pub is_async: bool,
    #[tracked] pub is_unsafe: bool,
    #[tracked] pub span: AbsoluteSpan,
}
```

This replaces the current `params`, `ret_type` tracked fields with a single `signature` field. The body remains its own stash (it's a separate compilation unit). `is_async` and `is_unsafe` stay as tracked fields since they're scalars, not stash material.

**Other items.** `ImplAst` (`self_ty: TypeRef`, `trait_path: Option<Path>`), `TypeAliasAst` (`ty: Option<TypeRef>`), `ConstAst` (`ty: Option<TypeRef>`), and `StaticAst` (`ty: Option<TypeRef>`) also carry salsa-tracked `TypeRef`/`Path` fields. These get their own signature stashes following the same pattern:

```rust
pub type ImplSigAst<'db> = Stashed<Ptr<ImplSigAstData<'db>>>;

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, AllocStashData)]
pub struct ImplSigAstData<'db> {
    pub generics: Slice<GenericParam<'db>>,
    pub self_ty: Ptr<TypeRefAst<'db>>,
    pub trait_path: Option<Ptr<PathAst<'db>>>,
}
```

`TypeAliasAst`, `ConstAst`, and `StaticAst` each get a minimal signature stash containing their generics (if any) and type reference.

### Stash-allocated signature types

The following types live in the signature stash. They are stash-allocated (`Copy`, derive `AllocStashData`) equivalents of the current salsa-tracked types in `types.rs`:

```rust
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, AllocStashData)]
pub enum GenericParam<'db> {
    Type { name: Name<'db>, span: RelativeSpan },
    Lifetime { name: Name<'db>, span: RelativeSpan },
    Const { name: Name<'db>, ty: Ptr<TypeRefAst<'db>>, span: RelativeSpan },
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, AllocStashData)]
pub struct ParamAst<'db> {
    pub name: Option<Name<'db>>,
    pub ty: Ptr<TypeRefAst<'db>>,
    pub span: RelativeSpan,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, AllocStashData)]
pub struct FieldDefAst<'db> {
    pub name: Name<'db>,
    pub ty: Ptr<TypeRefAst<'db>>,
    pub span: RelativeSpan,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, AllocStashData)]
pub struct VariantDefAst<'db> {
    pub name: Name<'db>,
    pub fields: Slice<FieldDefAst<'db>>,
    pub span: RelativeSpan,
}
```

### Stash-allocated type references and paths

`TypeRef` and `Path` move from salsa-tracked structs to stash-allocated types within the signature stash:

```rust
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, AllocStashData)]
pub struct TypeRefAst<'db> {
    pub kind: TypeRefAstKind<'db>,
    pub span: RelativeSpan,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, AllocStashData)]
pub enum TypeRefAstKind<'db> {
    Path(Ptr<PathAst<'db>>),
    Reference(Ptr<TypeRefAst<'db>>, Mutability),
    Slice(Ptr<TypeRefAst<'db>>),
    Array(Ptr<TypeRefAst<'db>>),
    Tuple(Slice<TypeRefAst<'db>>),
    Never,
    Infer,
    Error,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, AllocStashData)]
pub struct PathAst<'db> {
    pub segments: Slice<PathSegmentAst<'db>>,
    pub span: RelativeSpan,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, AllocStashData)]
pub struct PathSegmentAst<'db> {
    pub name: Name<'db>,
    pub type_args: Slice<TypeRefAst<'db>>,
    pub span: RelativeSpan,
}
```

This also addresses the type-arguments-on-paths gap: `PathSegmentAst` carries a `type_args` slice, so `HashMap<K, V>` becomes a path with one segment whose `type_args` has two entries. Most segments have an empty `type_args` slice.

### Body stashes use the same types

Body stashes currently reference the salsa-tracked `TypeRef` and `Path` for type annotations and casts (e.g., `ExprKind::Cast(_, TypeRef<'db>)`, `StmtKind::Let(_, Option<TypeRef<'db>>, _)`, `ClosureParam { ty: Option<TypeRef<'db>> }`). These migrate to the new stash-allocated `TypeRefAst` and `PathAst`.

Since body nodes are already stash-allocated, the type reference nodes just go into the same body stash alongside expressions, statements, and patterns. A `let x: HashMap<K, V> = ...` allocates its `TypeRefAst` → `PathAst` → `PathSegmentAst` chain in the body stash, right next to the expression tree. Paths in expressions (e.g., `ExprKind::Path`) that currently use the salsa-tracked `Path` also migrate to `PathAst`.

The *resolved* body uses `Ty` for type annotations, not `TypeRefAst`. The body resolver already turns value paths into `Res<'db>` (i.e., `Symbol` or `LocalId`), so it's natural for it to also resolve type annotations into `Ty`. The `SigLowerCtx` machinery (which does `TypeRefAst` → `Ty` with name resolution) is reused by the body resolver for this purpose.

Concretely: `RExprKind::Cast(_, TypeRef<'db>)` becomes `RExprKind::Cast(_, Ptr<Ty<'db>>)`, `RStmtKind::Let(_, Option<TypeRef<'db>>, _)` becomes `RStmtKind::Let(_, Option<Ptr<Ty<'db>>>, _)`, and `RClosureParam { ty: Option<TypeRef<'db>> }` becomes `RClosureParam { ty: Option<Ptr<Ty<'db>>> }`. The resolved body stash contains both `RExpr`/`RPat` nodes and `Ty` nodes — all in the same stash.

After this migration, the old salsa-tracked `Param`, `FieldDef`, `VariantDef`, and `TupleTypeRef` in `types.rs` are dead code and can be deleted. **`TypeRef` and `Path` must survive** — they are used outside of signatures and bodies in memmap data (`MemmapEntry::Redirect`, `Glob`, `MacroUse`), `MacroInvocationAst`, `UseImport`, and `Attr`. These contexts are salsa-tracked, not stash-allocated, and don't migrate. The two representations coexist: `PathAst`/`TypeRefAst` in stashes, `Path`/`TypeRef` in salsa-tracked memmap/attr data.

### Lowering changes

The tree-sitter grammar provides:
- `type_parameters` on function/struct/enum/trait/impl/type-alias items — the declaration `<T, U: Bound>`
- `type_arguments` on `generic_type` nodes — the use `HashMap<K, V>`

The item lowering builds a signature stash per item:

```rust
fn lower_fn_signature(&self, fn_node: Node) -> FnSigAst<'db> {
    let mut stash = Stash::new();
    let generics = self.lower_generic_params(&mut stash, fn_node);
    let params = self.lower_params(&mut stash, fn_node);
    let ret_type = self.lower_ret_type(&mut stash, fn_node);
    let root = stash.alloc(FnSigAstData { generics, params, ret_type });
    Stashed::new(stash, root)
}
```

Each helper allocates into the shared stash. `lower_type_ref_ast` allocates `TypeRefAst` nodes; `lower_path_ast` allocates `PathAst` with `PathSegmentAst`s carrying type arguments.

Where clause support (`where T: Foo`) is deferred — bounds are not yet resolved. The MVP captures generic param *names* and type argument *positions*; resolve bounds later.

## Layer 1: `Ty` representation

All types below are stash-allocated (`Copy`, derive `AllocStashData`). They live in the same stash as the signature or body they belong to. No global interning.

### `Ty`

```rust
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, AllocStashData)]
pub struct Ty<'db> {
    pub data: TyData<'db>,
}
```

### `TyData`

```rust
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, AllocStashData)]
pub enum TyData<'db> {
    // --- primitives ---
    Bool,
    Char,
    Int(IntTy),
    Uint(UintTy),
    Float(FloatTy),
    Str,

    // --- compound ---
    /// An ADT (struct, enum, union) or other named type with substituted args.
    /// `Symbol` identifies the type; `Slice<Ty>` holds substituted type args.
    Adt(Symbol<'db>, Slice<Ty<'db>>),

    /// `&'a T` or `&'a mut T`.
    Ref(Ptr<Ty<'db>>, Mutability, Lifetime),

    /// `(A, B, C)`.
    Tuple(Slice<Ty<'db>>),

    /// `[T]`.
    Slice(Ptr<Ty<'db>>),

    /// `[T; N]`.
    Array(Ptr<Ty<'db>>, Const<'db>),

    /// `fn(A, B) -> C`.
    FnPtr(Slice<Ty<'db>>, Ptr<Ty<'db>>),

    /// A reference to a bound variable introduced by an enclosing `Binder`.
    BoundVar(BoundVar),

    /// `!`
    Never,

    /// Placeholder for types we couldn't resolve or haven't implemented.
    Error,
}
```

Primitive type details:

```rust
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, AllocStashData)]
pub enum IntTy { I8, I16, I32, I64, I128, Isize }

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, AllocStashData)]
pub enum UintTy { U8, U16, U32, U64, U128, Usize }

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, AllocStashData)]
pub enum FloatTy { F32, F64 }
```

`impl StashDirect` for all the leaf types (`IntTy`, `UintTy`, `FloatTy`, `BoundVar`, `Lifetime`, `Const`, `Mutability`) so they get blanket `StashEq`/`StashHash` impls.

### `Lifetime`

```rust
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, AllocStashData)]
pub enum Lifetime {
    /// A reference to a bound lifetime variable introduced by an enclosing `Binder`.
    BoundVar(BoundVar),
    /// `'static`.
    Static,
    /// Elided or unresolved.
    Erased,
}
```

### `Const`

```rust
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, AllocStashData)]
pub enum Const<'db> {
    /// A known literal value (e.g., `[u8; 4]`).
    Literal(u64),
    /// Anything else — named const, expression, etc.
    Other(Symbol<'db>),
}
```

### `BoundVar`

```rust
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, AllocStashData)]
pub struct BoundVar {
    /// De Bruijn index: 0 = innermost enclosing `Binder`.
    pub binder_index: u32,
    /// Position within that binder's parameter list.
    pub param_index: u32,
}
```

### `Binder<T>`

**Note:** The `AllocStashData` derive macro rejects type parameters. `Binder<'db, T>` uses a manual `unsafe impl StashData` with `#[repr(C)]` and `PhantomData` for the lifetime.

```rust
pub struct Binder<'db, T> {
    pub value: T,
    pub bound_vars: Slice<BoundVarInfo>,
}
```

`BoundVarInfo` carries just the kind — enough for a canonical representation. User-facing names (for diagnostics, printing) come from the syntactic signature AST when needed, not from the resolved type:

```rust
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, AllocStashData)]
pub struct BoundVarInfo {
    pub kind: BoundVarKind,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, AllocStashData)]
pub enum BoundVarKind {
    Type,
    Lifetime,
    Const,
}
```

**Example.** `struct HashMap<K, V> { ... }` produces:

```
Binder {
    bound_vars: [
        BoundVarInfo { kind: Type },
        BoundVarInfo { kind: Type },
    ],
    value: StructSig {
        fields: [
            // field types that reference K use BoundVar { binder_index: 0, param_index: 0 }
            // field types that reference V use BoundVar { binder_index: 0, param_index: 1 }
        ],
    },
}
```

`Binder` is generic over `T` so it wraps `FnSig`, `StructSig`, etc. The same struct is used for nested binders (e.g., `for<'a>` on trait bounds in the future) — the de Bruijn indices handle the nesting.

## Layer 2: Per-kind symbol wrappers

Following the `ModSymbol` pattern, introduce kind wrappers for the symbol types that carry signatures:

```rust
pub struct FnSymbol<'db> { data: FnSymbolData<'db> }

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
enum FnSymbolData<'db> {
    Ast(FnAst<'db>),
    Ext(SymExt),
}

pub struct StructSymbol<'db> { data: StructSymbolData<'db> }
pub struct EnumSymbol<'db> { data: EnumSymbolData<'db> }
pub struct TraitSymbol<'db> { data: TraitSymbolData<'db> }
```

Each follows the same template: `Copy` wrapper-of-enum, `From` impls from both arms, `data()` method. A `define_kind_symbol!` macro generates the struct, enum, `From` impls, `data()`, and `Copy`/`Clone`/`Debug`/`PartialEq`/`Eq`/`Hash` derives for each kind.

The `SymbolData` enum migration (rewriting the flat `Ast`/`TupleStructCtor`/`Ext` enum to use per-kind variants, adding `SymExtKind`, and migrating all callers) is covered by the [per-kind symbol data RFD](./per-kind-symbol-data.md).

## Layer 3: Signature queries

### Signature types

Each item kind has a sig struct that lives inside a stash:

```rust
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, AllocStashData)]
pub struct FnSig<'db> {
    pub params: Slice<Ty<'db>>,
    pub ret: Ptr<Ty<'db>>,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, AllocStashData)]
pub struct StructSig<'db> {
    pub fields: Slice<FieldSig<'db>>,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, AllocStashData)]
pub struct FieldSig<'db> {
    pub name: Name<'db>,
    pub ty: Ptr<Ty<'db>>,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, AllocStashData)]
pub struct EnumSig<'db> {
    pub variants: Slice<VariantSig<'db>>,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, AllocStashData)]
pub struct VariantSig<'db> {
    pub name: Name<'db>,
    pub fields: Slice<FieldSig<'db>>,
}
```

### Queries

Each returns `Stashed<Binder<'db, XSig<'db>>>`:

```rust
#[salsa::tracked(returns(ref))]
fn fn_signature<'db>(db: &'db dyn Db, fn_sym: FnAst<'db>, module: ModSymbol<'db>, source_root: SourceRoot) -> Stashed<Binder<'db, FnSig<'db>>>;

#[salsa::tracked(returns(ref))]
fn struct_signature<'db>(db: &'db dyn Db, struct_sym: StructAst<'db>, module: ModSymbol<'db>, source_root: SourceRoot) -> Stashed<Binder<'db, StructSig<'db>>>;

#[salsa::tracked(returns(ref))]
fn enum_signature<'db>(db: &'db dyn Db, enum_sym: EnumAst<'db>, module: ModSymbol<'db>, source_root: SourceRoot) -> Stashed<Binder<'db, EnumSig<'db>>>;
```

Note these are keyed on `FnAst`, not `FnSymbol`, because salsa tracked functions need salsa-tracked struct keys. The `module` and `source_root` parameters provide the resolution context (same pattern as `resolve_body`). For external symbols, the signature is constructed from `TcxDb` queries, not from these tracked functions.

A `FnSymbol::signature` convenience method dispatches: ast arm → call the tracked function; ext arm → query `TcxDb` and build the stashed result.

### Tuple struct constructor signature

The [tuple struct constructors RFD](./tuple-struct-ctors.md) deferred its Step 4 (signature) to this RFD. The constructor's signature is a `FnSig` derived from the struct's `StructSig` — no separate lowering from `TypeRefAst`, just a transformation of already-resolved types:

```rust
fn tuple_struct_ctor_signature(struct_sig: &Stashed<Binder<StructSig>>) -> Stashed<Binder<FnSig>>
```

The params are the struct's field types (in order), the return type is the struct type itself (with its generic args as `BoundVar`s). The `Binder` is inherited from the struct's own signature. This is a pure restructuring — no name resolution involved.

### Lowering `TypeRefAst` → `Ty`

Signature lowering reads from the syntactic signature stash and writes into a resolved signature stash. A `SigLowerCtx` holds:

```rust
struct SigLowerCtx<'a, 'db> {
    db: &'db dyn Db,
    /// The syntactic signature stash (source).
    src: &'a Stash,
    /// The resolved signature stash (target).
    dst: &'a mut Stash,
    module: ModSymbol<'db>,
    source_root: SourceRoot,
    /// Maps generic param names to their BoundVar (current binder only).
    generics: Vec<(Name<'db>, BoundVar)>,
    /// The impl block's self type, if lowering a method. `Self` resolves to this.
    self_type: Option<Ty<'db>>,
}
```

The core method walks `TypeRefAst` nodes from the source stash and allocates `Ty` nodes in the destination stash:

```rust
impl<'a, 'db> SigLowerCtx<'a, 'db> {
    fn lower_type_ref(&mut self, ty: TypeRefAst<'db>) -> Ptr<Ty<'db>> {
        match ty.kind {
            TypeRefAstKind::Path(path) => self.lower_path_type(self.src[path]),
            TypeRefAstKind::Reference(inner, m) => {
                let inner = self.lower_type_ref(self.src[inner]);
                self.dst.alloc(Ty { data: TyData::Ref(inner, m, Lifetime::Erased) })
            }
            TypeRefAstKind::Tuple(elems) => {
                let elems: Vec<_> = self.src[elems].iter()
                    .map(|e| self.lower_type_ref(*e))
                    .collect();
                let tys: Vec<_> = elems.iter().map(|ptr| self.dst[*ptr]).collect();
                let elems = self.dst.alloc_slice(&tys);
                self.dst.alloc(Ty { data: TyData::Tuple(elems) })
            }
            TypeRefAstKind::Slice(inner) => {
                let inner = self.lower_type_ref(self.src[inner]);
                self.dst.alloc(Ty { data: TyData::Slice(inner) })
            }
            TypeRefAstKind::Array(inner) => {
                let inner = self.lower_type_ref(self.src[inner]);
                self.dst.alloc(Ty { data: TyData::Array(inner, /* copy const */) })
            }
            TypeRefAstKind::Never => self.dst.alloc(Ty { data: TyData::Never }),
            TypeRefAstKind::Infer | TypeRefAstKind::Error =>
                self.dst.alloc(Ty { data: TyData::Error }),
        }
    }
}
```

`lower_path_type` is where it gets interesting:

1. **Single segment, matches a generic param** → `TyData::BoundVar(...)`.
2. **Single segment, primitive name** (`bool`, `i32`, `str`, ...) → `TyData::Bool`, `TyData::Int(I32)`, etc.
3. **Otherwise** → resolve the path via `resolve_name` / `resolve_path` in the type namespace → `Symbol`. Then lower any type arguments on the final path segment. Produce `TyData::Adt(symbol, args)`.
4. **Resolution failure** → `TyData::Error`.

Step 1 checks generics first, matching how Rust scoping works: a generic param `T` shadows any item named `T` in scope.

### External signatures via `TcxDb`

To produce signatures for external symbols, `TcxDb` needs new queries. The exact shape depends on how we marshal type information from rustc — this is a significant piece of work in its own right and is the main constraint on when external-symbol signatures become available.

The minimal addition:

```rust
trait TcxDb {
    // ... existing methods ...

    /// Returns the signature of a function as a serialized type descriptor.
    fn fn_sig(&self, crate_num: CrateNum, def_index: DefIndex) -> Option<RawFnSig>;

    /// Returns the fields of a struct/enum as serialized type descriptors.
    fn adt_fields(&self, crate_num: CrateNum, def_index: DefIndex) -> Option<RawAdtFields>;
}
```

The `Raw*` types are `'static` (no salsa lifetimes) and use a simple serialization format that the sage side can lower into stash-allocated `Ty` nodes. The exact format is deferred — what matters architecturally is that the boundary is a simple, owned data transfer, not a shared pointer into rustc's arenas.

## TyFolder: cross-stash type mapping

When consuming a signature (e.g., during type checking of a call expression), the caller instantiates the signature's `Binder` with concrete type arguments, producing `Ty` nodes in the caller's stash. This requires copying and transforming types across stashes.

```rust
trait TyFolder<'db> {
    fn target(&mut self) -> &mut Stash;
    fn source(&self) -> &Stash;

    fn fold_ty(&mut self, ty: Ty<'db>) -> Ty<'db> {
        default_fold_ty(self, ty)
    }
}
```

`fold_ty` takes and returns `Ty` by value — `Ty` is `Copy`, so there's no need to go through `Ptr` during the fold. The caller allocates the final result into a stash only when it needs a handle. This makes the fold composable and avoids the double-indirection of fold-then-dereference.

The default implementation does a structural copy — walk each `TyData` variant, recursively fold children from `self.source()`, alloc intermediates into `self.target()` only where indirection is needed (e.g., `Ptr<Ty>` fields inside `TyData`):

```rust
fn default_fold_ty<'db>(folder: &mut impl TyFolder<'db>, ty: Ty<'db>) -> Ty<'db> {
    let data = match ty.data {
        TyData::Adt(sym, args) => {
            let args = fold_slice(folder, args);
            TyData::Adt(sym, args)
        }
        TyData::Ref(inner, m, lt) => {
            let inner = folder.fold_ty(folder.source()[inner]);
            let inner = folder.target().alloc(inner);
            TyData::Ref(inner, m, lt)
        }
        TyData::Tuple(elems) => {
            let elems = fold_slice(folder, elems);
            TyData::Tuple(elems)
        }
        TyData::Slice(inner) => {
            let inner = folder.fold_ty(folder.source()[inner]);
            let inner = folder.target().alloc(inner);
            TyData::Slice(inner)
        }
        TyData::Array(inner, c) => {
            let inner = folder.fold_ty(folder.source()[inner]);
            let inner = folder.target().alloc(inner);
            TyData::Array(inner, c)
        }
        TyData::FnPtr(params, ret) => {
            let params = fold_slice(folder, params);
            let ret = folder.fold_ty(folder.source()[ret]);
            let ret = folder.target().alloc(ret);
            TyData::FnPtr(params, ret)
        }
        // Leaves — no indirection to follow
        leaf => leaf,
    };
    Ty { data }
}

fn fold_slice<'db>(
    folder: &mut impl TyFolder<'db>,
    slice: Slice<Ty<'db>>,
) -> Slice<Ty<'db>> {
    let source = folder.source();
    let tys: Vec<_> = source[slice].iter().map(|ty| folder.fold_ty(*ty)).collect();
    folder.target().alloc_slice(&tys)
}
```

### Instantiation

The first concrete folder — substitutes `BoundVar`s at the outermost binder with supplied arguments:

```rust
struct Instantiate<'a, 'db> {
    source: &'a Stash,
    target: &'a mut Stash,
    /// Concrete types for each bound var in the outermost binder.
    args: Vec<Ty<'db>>,
}

impl<'db> TyFolder<'db> for Instantiate<'_, 'db> {
    fn target(&mut self) -> &mut Stash { self.target }
    fn source(&self) -> &Stash { self.source }

    fn fold_ty(&mut self, ty: Ty<'db>) -> Ty<'db> {
        match ty.data {
            TyData::BoundVar(bv) if bv.binder_index == 0 => {
                self.args[bv.param_index as usize]
            }
            TyData::BoundVar(bv) => {
                Ty {
                    data: TyData::BoundVar(BoundVar {
                        binder_index: bv.binder_index - 1,
                        ..bv
                    }),
                }
            }
            _ => default_fold_ty(self, ty),
        }
    }
}
```

A convenience method on `Binder`:

```rust
impl<'db, T> Binder<'db, T> {
    fn instantiate(
        &self,
        source: &Stash,
        target: &mut Stash,
        args: &[Ptr<Ty<'db>>],
    ) -> T
    where T: /* has a fold method or the sig types know how to fold themselves */
    { ... }
}
```

The exact mechanics of folding `T` (which is `FnSig`, `StructSig`, etc.) depend on whether we make `TyFolder` aware of sig types or just fold each `Ty` field individually. The simplest approach: each sig type has a `fold_with` method that folds its `Ty`-containing fields.

### `fold_binder` hook

When folding enters a nested `Binder` (e.g., `for<'a>` in a trait bound), the default implementation shifts the depth for substitutions. The trait has a `fold_binder` method that handles this:

```rust
trait TyFolder<'db> {
    // ...

    fn fold_binder<T>(&mut self, binder: Binder<'db, T>) -> Binder<'db, T>
    where T: Foldable<'db>
    {
        // Default: shift depth tracking, fold inner value, rebuild Binder
        self.enter_binder();
        let value = binder.value.fold_with(self);
        let bound_vars = /* copy bound_vars to target stash */;
        self.exit_binder();
        Binder { value, bound_vars }
    }
}
```

For the MVP, nested binders don't arise (no `for<'a>`, no `impl Trait` in signatures), so this hook exists for forward compatibility but the default implementation suffices.

## Incremental behavior

Signature queries are naturally incremental via salsa:

- `fn_signature(db, fn_ast, module, source_root)` depends on `fn_ast.signature(db)` (a single tracked field holding the `FnSigAst` stash), plus `resolve_name` calls for type paths.
- Editing a function's *body* does not change its signature stash, so `fn_signature` is not re-executed.
- Editing a *different* function in the same file does not change this function's signature stash (salsa tracks field-level reads via tracked structs).
- The syntactic signature stash (`FnSigAst`) is compared by byte content — if the signature text is unchanged after re-parsing, salsa sees no change and skips re-lowering entirely.
- The returned `Stashed<Binder<FnSig>>` is also compared by content — if the resolved signature is structurally unchanged after re-lowering (e.g., an unrelated import was added but the resolved types are the same), downstream consumers see no change.

The per-item signature stash is coarser than the previous per-field tracking (changing a return type invalidates the whole signature, not just consumers of the return type). This is acceptable: signature edits are uncommon, and re-lowering a signature is cheap (a handful of name resolutions, no body walking).

## Scope and non-goals

**In scope:**
- Generic parameter lists on items (type params; lifetime and const params parsed but not fully resolved)
- Type arguments on paths
- Stash-allocated syntactic types (`TypeRefAst`, `PathAst`) replacing salsa-tracked `TypeRef`/`Path` in signatures and bodies
- `Ty` / `Binder` / `BoundVar` representation
- Per-kind symbol wrappers
- `fn_signature`, `struct_signature`, `enum_signature` for workspace items
- `SigLowerCtx` lowering `TypeRefAst` → `Ty` with name resolution
- Resolved body uses `Ty` for type annotations (casts, let bindings, closure params)
- `TyFolder` and `Instantiate`
- Primitive type recognition (`bool`, `i32`, `str`, etc.)

**Out of scope (future work):**
- Where clauses and trait bounds (parsed syntactically, not resolved)
- `impl Trait` in argument or return position
- Lifetime resolution (lifetimes in `Binder` are modeled but not resolved)
- Const generics beyond `Const::Literal` and `Const::Other`
- External symbol signatures via `TcxDb` (deferred until the format is designed)
- Type inference and the `type_checked_body` query
- Trait solving
- **Stash-ify the memmap.** The salsa-tracked `Path` and `TypeRef` survive in this RFD for memmap/attr uses (`MemmapEntry::Redirect`, `Glob`, `MacroUse`, `MacroInvocationAst`, `UseImport`, `Attr`). A future RFD should convert the memmap itself to stash-allocated data, at which point these salsa-tracked types can be deleted entirely. That refactor is non-trivial because the iterative expansion loop mutates memmap entries across rounds (unresolved → resolved → expanded), which requires rethinking the build-once-seal pattern — e.g., rebuilding the stash each iteration or using a mutable stash sealed at convergence.

## Implementation plan

Every step follows **test-first**: write the tests, verify they fail (compile error or assertion failure), then write the implementation to make them pass. Tests use `expect_test` inline snapshots (for structural output) and the mini-redis fixture (for integration coverage), matching the patterns in the existing test suite.

### Step A: Stash-allocated signature AST types ✓

**Types.** Add the new stash-allocated signature types: `TypeRefAst`, `TypeRefAstKind`, `PathAst`, `PathSegmentAst`, `GenericParam`, `ParamAst`, `FieldDefAst`, `VariantDefAst`. Add the per-item signature stash types: `FnSigAst`/`FnSigAstData`, `StructSigAst`/`StructSigAstData`, `EnumSigAst`/`EnumSigAstData`, `ImplSigAst`/`ImplSigAstData`, and minimal stashes for `TypeAliasAst`, `ConstAst`, `StaticAst`. Pure type definitions — no lowering yet.

**Tests.** Write unit tests that manually construct signature stashes and verify round-tripping through `Stashed`:
1. Build a `FnSigAstData` by hand with generics, params, and a return type in a stash. Verify the root and fields are accessible via `Ptr`/`Slice` indexing.
2. Build a `PathSegmentAst` with type arguments. Verify the type args slice is non-empty and contains the expected `TypeRefAst` nodes.
3. Build two `Stashed<...>` with identical content and assert they are `==`. Build a third with different content and assert `!=`.

Write these tests first. They should compile (the types exist) and pass (pure construction). This validates the stash layout before any lowering touches it.

### Step B: Body stash migration (deferred)

**Tests first.** Before changing any body types, write new tests (or extend existing `body_resolve_tests`) that assert on the *current* resolved body output for a snippet containing a cast, a let-with-type-annotation, and a closure with typed params. Capture the current output as a baseline snapshot. These tests pass against the old `TypeRef`-based body.

**Implementation.** Migrate body stashes (`body.rs`, `resolved.rs`) from salsa-tracked `TypeRef`/`Path` to the new stash-allocated `TypeRefAst`/`PathAst`. Body lowering allocates type reference nodes into the body stash alongside expressions and patterns.

Types to update in `body.rs`: `ExprKind::Cast`, `ExprKind::Path`, `ExprKind::StructLit`, `ExprKind::MacroCall`, `StmtKind::Let`, `ClosureParam`, `PatKind::Path`, `PatKind::Struct`, `PatKind::TupleStruct` — all reference `Path<'db>` or `TypeRef<'db>`. The resolved equivalents in `resolved.rs` follow suit (most expression/pattern paths are already `Res<'db>`, but `RExprKind::Cast` and `RStmtKind::Let` still carry `TypeRef`, and `RExprKind::MacroCall` carries `TokenTree`).

Delete the old salsa-tracked `Param`, `FieldDef`, `VariantDef`, `TupleTypeRef` from `types.rs`. **`TypeRef` and `Path` survive** for memmap/attr uses. Update `display.rs` to format `TypeRefAst`/`PathAst` from stashes (the current `Display for TypeRef` reads salsa fields). Update `derive/builtins.rs` to build stash-allocated types when constructing synthetic items (it currently calls `Param::new(db, ...)`, `TypeRef::new(db, ...)`, `Path::new(db, ...)`).

Note: `body_resolve.rs:390` reads `function.params(db)` to push parameter bindings into scope — post-migration it accesses the signature stash instead.

**Verify.** Update the baseline snapshots to reflect the new types and confirm all existing tests (including mini-redis snapshot tests) still pass.

### Step C: Signature lowering from tree-sitter ✓

**Tests first.** Write tests for signature lowering — these should fail initially (the lowering doesn't exist yet):
1. Parse `fn foo<T, U>(x: T, y: &U) -> bool` from a source string. Access `fn_ast.signature(db)`. Walk the stash to verify: generics has 2 entries (`T`, `U`), params has 2 entries with correct `TypeRefAst` types, return type is a path to `bool`.
2. Parse `struct Pair<A, B> { first: A, second: B }`. Access `struct_ast.signature(db)`. Verify generics and fields.
3. Parse a type with type arguments: `fn bar(m: HashMap<String, Vec<u8>>)`. Verify the `PathSegmentAst` for `HashMap` has 2 type args, and the second type arg is itself a `PathAst` with one segment (`Vec`) carrying one type arg.
4. Mini-redis integration: parse all files, iterate items, access signatures, and produce a snapshot dump of signature stash contents. This snapshot starts empty / fails and gets filled in as the lowering is built.

**Implementation.** Build the lowering that produces the new signature stashes. Each item gets a `lower_*_signature` method that allocates generic params, params/fields/variants, and type references (including type arguments on paths) into a single stash. Migrate `FnAst`, `StructAst`, `EnumAst` to hold a single `signature` tracked field instead of separate `params`/`ret_type`/`fields`/`variants`/`generics` fields. Update all callers of the old fields.

**Verify.** All new tests pass. All existing tests still pass (snapshot updates as needed for the field changes).

### Step D: `Ty` types ✓

**Types.** Add `ty.rs` with `Ty`, `TyData`, `BoundVar`, `Binder`, `BoundVarInfo`, `BoundVarKind`, `Lifetime`, `Const`, `IntTy`, `UintTy`, `FloatTy`, `FnSig`, `StructSig`, `EnumSig`, `FieldSig`, `VariantSig`. All derive `AllocStashData`, `Copy`, etc.

**Tests.** Write unit tests for stash round-tripping, mirroring step A:
1. Build a `Ty` with `TyData::Adt(symbol, args)` in a stash. Verify the args slice contains the expected types.
2. Build a `Binder<FnSig>` with two `BoundVarInfo { kind: Type }` entries and a `FnSig` whose param types reference `BoundVar { binder_index: 0, param_index: 0 }`. Verify all fields accessible.
3. Verify `Stashed` equality: two identical `Binder<FnSig>` stashes are `==`; one with a different return type is `!=`.

These are pure construction tests — they compile and pass immediately, validating the type layout.

### Step E: Per-kind symbol wrappers ✓

**Tests first.** Write tests that construct `Symbol` values and round-trip through `data()`:
1. Construct a `Symbol` from a `FnAst`, call `data()`, match on `SymbolData::Fn(fn_sym)`, verify `fn_sym.data()` yields `FnSymbolData::Ast(fn_ast)`.
2. Construct a `Symbol` from a `StructAst`, verify it matches `SymbolData::Struct(...)`.
3. Construct a `Symbol::tuple_struct_ctor(struct_ast)`, verify it matches `SymbolData::TupleStructCtor(struct_sym)`.
4. Construct a `Symbol` from a `SymExt`, verify it yields the correct kind variant (this exercises the ext-arm dispatch — the test should clarify what kind information is available for ext symbols).

These tests fail initially (the new `SymbolData` variants don't exist). Write the wrappers and macro to make them pass.

**Implementation.** Introduce `FnSymbol`, `StructSymbol`, `EnumSymbol`, `TraitSymbol`, `TypeAliasSymbol`, `ConstSymbol`, `StaticSymbol`. Rework `SymbolData` to use kind variants. The existing `TupleStructCtor(StructAst)` variant migrates to `TupleStructCtor(StructSymbol)`. Add a macro for the boilerplate. Update existing `Symbol::data()` callers — mostly mechanical, including the tuple-struct-ctor resolution code in `resolve.rs` and the display/validation code. `ModSymbol` is unchanged.

**Verify.** New tests pass. All existing tests still pass (including the tuple-struct-ctor tests from the previous RFD).

### Step F: `SigLowerCtx` and signature queries ✓

**Tests first.** Write tests that call the signature queries and assert on the resolved `Ty` output — these fail initially (the queries don't exist):
1. `fn identity<T>(x: T) -> T` → `fn_signature` yields `Binder { bound_vars: [Type], value: FnSig { params: [BoundVar(0,0)], ret: BoundVar(0,0) } }`.
2. `fn add(a: i32, b: i32) -> i32` → no binder (or empty binder), params are `[Int(I32), Int(I32)]`, ret is `Int(I32)`.
3. `struct Pair<A, B> { first: A, second: B }` → `struct_signature` yields `Binder { bound_vars: [Type, Type], value: StructSig { fields: [("first", BoundVar(0,0)), ("second", BoundVar(0,1))] } }`.
4. `fn takes_ref(x: &str) -> &[u8]` → params include `Ref(Str, Shared, Erased)`, ret is `Ref(Slice(Uint(U8)), Shared, Erased)`.
5. **Tuple struct constructor signature** (deferred Step 4 from tuple-struct-ctors RFD):
   - `struct Foo(i32, String);` → constructor signature is `FnSig { params: [Int(I32), Adt(String, [])], ret: Adt(Foo, []) }`.
   - `struct Pair<A, B>(A, B);` → constructor signature is `Binder { bound_vars: [Type, Type], value: FnSig { params: [BoundVar(0,0), BoundVar(0,1)], ret: Adt(Pair, [BoundVar(0,0), BoundVar(0,1)]) } }`.
   - `struct Unit;` → constructor signature is `FnSig { params: [], ret: Adt(Unit, []) }`.
6. Mini-redis integration: for each function and struct in mini-redis, call the signature query and produce a snapshot dump. Dual snapshot: result + query log (same pattern as `body_resolve_tests`).
7. Resolved body with type annotations: `let x: Vec<i32> = ...` → the `RStmtKind::Let` contains a `Ty` with `TyData::Adt(vec_symbol, [Int(I32)])`.

**Implementation.** Implement `SigLowerCtx` — reads `TypeRefAst` from the syntactic signature stash, writes `Ty` into a resolved signature stash. Generic param tracking and name resolution. Implement `fn_signature`, `struct_signature`, `enum_signature` as salsa tracked functions. Implement `tuple_struct_ctor_signature` that derives a `FnSig` from a `StructSig`. Wire body resolver to use `SigLowerCtx` for type annotations.

**Verify.** All new tests pass. All existing tests still pass (including tuple-struct-ctor resolution tests).

### Step G: `TyFolder` and instantiation ✓

**Tests first.** Write tests for the folder — these fail initially (the trait doesn't exist):
1. **Plain copy.** Build a `Ty` tree in stash A (e.g., `Ref(Adt(sym, [Int(I32)]), Shared, Erased)`). Fold with an identity folder into stash B. Verify the result in stash B is structurally equal to the original.
2. **Instantiate generic function.** Build a `Binder<FnSig>` for `fn identity<T>(x: T) -> T` in stash A. Instantiate with `T = Int(I32)` into stash B. Verify: `FnSig { params: [Int(I32)], ret: Int(I32) }` — no `BoundVar`s remain.
3. **Instantiate generic struct.** Build a `Binder<StructSig>` for `struct Pair<A, B> { first: A, second: B }` in stash A. Instantiate with `A = Bool, B = Str` into stash B. Verify fields are `[("first", Bool), ("second", Str)]`.
4. **Nested type arguments.** Build a signature containing `Adt(HashMap, [BoundVar(0,0), Adt(Vec, [BoundVar(0,1)])])`. Instantiate with `[Str, Int(I32)]`. Verify result is `Adt(HashMap, [Str, Adt(Vec, [Int(I32)])])`.
5. **De Bruijn shift.** Build a type with `BoundVar { binder_index: 1, param_index: 0 }` (references an outer binder). Instantiate the inner binder. Verify the outer reference is shifted to `binder_index: 0`.

**Implementation.** Implement the `TyFolder` trait, `default_fold_ty`, `fold_slice`. Implement `Instantiate`.

**Verify.** All new tests pass.

### Step H: Remove legacy `params`/`ret_type` fields from `FnAst` ✓

`FnAst` currently carries both the old per-field tracked fields (`params: Vec<Param<'db>>`, `ret_type: Option<TypeRef<'db>>`) and the new `signature: FnSigAst<'db>` field side by side. All signature consumers should now read from the `signature` stash.

**Implementation.** Remove `params` and `ret_type` from `FnAst`. Update all callers to read from the signature stash instead. Similarly audit `StructAst`, `EnumAst`, and other items for any legacy per-field tracked fields that are superseded by their signature stash.

**Verify.** All existing tests still pass. The old salsa-tracked `Param` type in `types.rs` may become dead code — delete it if so.

### Step I: Add `self_type` to `SigLowerCtx` ✓

`SigLowerCtx` currently has no way to resolve `Self` in method signatures inside impl blocks. Resolved decision #2 calls for an `Option<Ptr<Ty<'db>>>` self type, but it is not yet implemented.

**Implementation.** Add `self_type: Option<Ptr<Ty<'db>>>` to `SigLowerCtx`. In `lower_path_type`, check for the single-segment name `Self` (after generic params, before primitives) and return the self type if present. Callers that lower impl-block methods pass the impl's self type; free-function callers pass `None`. Update the `SigLowerCtx` definition in the Layer 3 design section to include this field.

**Tests.** Write a test with `impl Foo { fn bar(&self) -> Self { ... } }` and verify the signature resolves `Self` to `Adt(Foo, [])`. Write a test for a generic impl: `impl<T> Wrapper<T> { fn get(&self) -> T }` and verify `Self` resolves to `Adt(Wrapper, [BoundVar(0,0)])`.

### Step J: Extract `SymbolData` migration to per-kind-symbol-data RFD ✓

Layer 2 of this RFD defines both the per-kind wrapper types (`FnSymbol`, `StructSymbol`, ...) and the `SymbolData` enum migration. The [per-kind-symbol-data RFD](./per-kind-symbol-data.md) now covers the `SymbolData` migration as its own scope.

**Implementation.** Remove the `SymbolData` enum definition, `TupleStructCtor` migration details, and caller-migration discussion from this RFD's Layer 2 section. Retain only the wrapper type definitions and the `define_kind_symbol!` macro. Update Step E in this plan to match — it should only cover adding the wrapper types, not rewriting `SymbolData` or migrating callers. Add a forward reference: "The `SymbolData` enum migration is covered by [per-kind-symbol-data](./per-kind-symbol-data.md)."

### Step K: Update `Binder` note and mark completed steps ✓

The note at the `Binder<T>` definition (Layer 1) describes the `AllocStashData` derive macro limitation as an open implementation choice. The manual `unsafe impl` approach was chosen (`#[repr(C)]` + `PhantomData`). Update the note to reflect the decision taken.

Mark steps A, D, E (wrappers only), F, and G as complete. Annotate steps B and C with their current progress.

## Resolved design decisions

1. **Sig queries keyed on `*Ast`, dispatched by `*Symbol`.** The salsa tracked functions are keyed on `FnAst`, `StructAst`, etc. The per-kind symbol wrappers (`FnSymbol`, `StructSymbol`, ...) provide a convenience method that matches on the ast/ext arms and dispatches: ast → call the tracked function, ext → query `TcxDb`. This is the natural pattern.

2. **`self` parameter.** `SigLowerCtx` holds `self_type: Option<Ty<'db>>`. The public `lower_fn_sig` function (non-salsa) accepts this; the `fn_signature` salsa query delegates with `None`. When lowering methods inside an impl block, the caller passes the impl's self type; `Self` in the signature resolves to it, and `&self` becomes `&Self`. The self type is copied from the caller's stash into the destination stash via the Identity folder.

3. **Tuple struct constructors.** Handled by the [tuple struct constructors RFD](./tuple-struct-ctors.md). The expansion phase emits a `TupleStructCtor` memmap entry; the signature layer returns a `FnSig` for it derived from the struct's fields.

4. **Naming.** The `*Ast` suffix is kept on `TypeRefAst`, `PathAst`, `PathSegmentAst` for clarity against the resolved `Ty`.

## Open questions

None remaining — all design questions resolved or split into separate RFDs.
