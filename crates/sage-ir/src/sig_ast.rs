//! Stash-allocated signature AST types.
//!
//! Each item kind gets a single stash for its entire syntactic signature:
//! generic params, parameter types, return types, field definitions, etc.
//! These replace the per-field salsa-tracked types (`Param`, `FieldDef`,
//! `VariantDef`) for signature data, while the old salsa-tracked `TypeRef`
//! and `Path` survive for memmap/attr uses.

use sage_stash::{AllocStashData, Ptr, Slice, Stashed};

use crate::name::Name;
use crate::span::RelativeSpan;
use crate::types::Mutability;

// ---------------------------------------------------------------------------
// Per-item signature stash type aliases
// ---------------------------------------------------------------------------

pub type FnSigAst<'db> = Stashed<Ptr<FnSigAstData<'db>>>;
pub type StructSigAst<'db> = Stashed<Ptr<StructSigAstData<'db>>>;
pub type EnumSigAst<'db> = Stashed<Ptr<EnumSigAstData<'db>>>;
pub type ImplSigAst<'db> = Stashed<Ptr<ImplSigAstData<'db>>>;
pub type TypeAliasSigAst<'db> = Stashed<Ptr<TypeAliasSigAstData<'db>>>;
pub type ConstSigAst<'db> = Stashed<Ptr<ConstSigAstData<'db>>>;
pub type StaticSigAst<'db> = Stashed<Ptr<StaticSigAstData<'db>>>;
pub type TraitSigAst<'db> = Stashed<Ptr<TraitSigAstData<'db>>>;

// ---------------------------------------------------------------------------
// Per-item signature data structs
// ---------------------------------------------------------------------------

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, AllocStashData)]
pub struct FnSigAstData<'db> {
    pub generics: Slice<GenericParam<'db>>,
    pub params: Slice<ParamAst<'db>>,
    pub ret_type: Option<Ptr<TypeRefAst<'db>>>,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, AllocStashData)]
pub struct StructSigAstData<'db> {
    pub generics: Slice<GenericParam<'db>>,
    pub fields: Slice<FieldDefAst<'db>>,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, AllocStashData)]
pub struct EnumSigAstData<'db> {
    pub generics: Slice<GenericParam<'db>>,
    pub variants: Slice<VariantDefAst<'db>>,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, AllocStashData)]
pub struct ImplSigAstData<'db> {
    pub generics: Slice<GenericParam<'db>>,
    pub self_ty: Ptr<TypeRefAst<'db>>,
    pub trait_path: Option<Ptr<PathAst<'db>>>,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, AllocStashData)]
pub struct TypeAliasSigAstData<'db> {
    pub generics: Slice<GenericParam<'db>>,
    pub ty: Option<Ptr<TypeRefAst<'db>>>,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, AllocStashData)]
pub struct ConstSigAstData<'db> {
    pub ty: Option<Ptr<TypeRefAst<'db>>>,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, AllocStashData)]
pub struct StaticSigAstData<'db> {
    pub ty: Option<Ptr<TypeRefAst<'db>>>,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, AllocStashData)]
pub struct TraitSigAstData<'db> {
    pub generics: Slice<GenericParam<'db>>,
}

// ---------------------------------------------------------------------------
// Generic parameters
// ---------------------------------------------------------------------------

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, AllocStashData)]
pub enum GenericParam<'db> {
    Type {
        name: Name<'db>,
        span: RelativeSpan,
    },
    Lifetime {
        name: Name<'db>,
        span: RelativeSpan,
    },
    Const {
        name: Name<'db>,
        ty: Ptr<TypeRefAst<'db>>,
        span: RelativeSpan,
    },
}

// ---------------------------------------------------------------------------
// Signature-level items
// ---------------------------------------------------------------------------

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

// ---------------------------------------------------------------------------
// Stash-allocated type references and paths
// ---------------------------------------------------------------------------

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
    pub generic_args: Slice<GenericArgAst<'db>>,
    pub span: RelativeSpan,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, AllocStashData)]
pub enum GenericArgAst<'db> {
    Type(TypeRefAst<'db>),
    Lifetime(Name<'db>),
}
