//! Generic parameters as salsa-tracked symbols.
//!
//! Each generic parameter (type, lifetime, const) becomes a stable salsa identity.
//! Types reference these directly via `TyData::Param(GenericParam)` rather than
//! using de Bruijn indices.

use crate::name::Name;
use crate::span::RelativeSpan;
use crate::symbol::Symbol;

// ---------------------------------------------------------------------------
// GenericParamKind
// ---------------------------------------------------------------------------

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, salsa::Update)]
pub enum GenericParamKind {
    Type,
    Lifetime,
    Const,
}

impl sage_stash::StashDirect for GenericParamKind {}

// ---------------------------------------------------------------------------
// AstGenericParam — from local source, created during item lowering
// ---------------------------------------------------------------------------

#[salsa::tracked(debug)]
pub struct AstGenericParam<'db> {
    pub kind: GenericParamKind,
    pub name: Option<Name<'db>>,
    pub span: RelativeSpan,
    pub parent: Symbol<'db>,
    pub index: u32,
}

impl sage_stash::StashDirect for AstGenericParam<'_> {}

unsafe impl<'db> sage_stash::StashData<'db> for AstGenericParam<'db> {
    type StaticSelf = AstGenericParam<'static>;
}

impl<'db> sage_stash::AllocStashData<'db> for AstGenericParam<'db> {}

// ---------------------------------------------------------------------------
// ExtGenericParam — from external crate metadata, interned on first encounter
// ---------------------------------------------------------------------------

#[salsa::interned(debug)]
pub struct ExtGenericParam<'db> {
    pub kind: GenericParamKind,
    pub name: Option<Name<'db>>,
    pub parent: Symbol<'db>,
    pub index: u32,
}

impl sage_stash::StashDirect for ExtGenericParam<'_> {}

unsafe impl<'db> sage_stash::StashData<'db> for ExtGenericParam<'db> {
    type StaticSelf = ExtGenericParam<'static>;
}

impl<'db> sage_stash::AllocStashData<'db> for ExtGenericParam<'db> {}

// ---------------------------------------------------------------------------
// AlphaEquivParam — canonical placeholder for alpha-equivalence testing
// ---------------------------------------------------------------------------

#[salsa::interned(debug)]
pub struct AlphaEquivParam<'db> {
    pub kind: GenericParamKind,
    pub index: u32,
}

impl sage_stash::StashDirect for AlphaEquivParam<'_> {}

unsafe impl<'db> sage_stash::StashData<'db> for AlphaEquivParam<'db> {
    type StaticSelf = AlphaEquivParam<'static>;
}

impl<'db> sage_stash::AllocStashData<'db> for AlphaEquivParam<'db> {}

// ---------------------------------------------------------------------------
// GenericParam — the unified enum
// ---------------------------------------------------------------------------

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, salsa::Update)]
pub enum GenericParam<'db> {
    Ast(AstGenericParam<'db>),
    Ext(ExtGenericParam<'db>),
    AlphaEquiv(AlphaEquivParam<'db>),
}

impl<'db> GenericParam<'db> {
    pub fn kind(&self, db: &'db dyn crate::Db) -> GenericParamKind {
        match self {
            Self::Ast(p) => p.kind(db),
            Self::Ext(p) => p.kind(db),
            Self::AlphaEquiv(p) => p.kind(db),
        }
    }

    pub fn name(&self, db: &'db dyn crate::Db) -> Option<Name<'db>> {
        match self {
            Self::Ast(p) => p.name(db),
            Self::Ext(p) => p.name(db),
            Self::AlphaEquiv(_) => None,
        }
    }
}

impl sage_stash::StashDirect for GenericParam<'_> {}

unsafe impl<'db> sage_stash::StashData<'db> for GenericParam<'db> {
    type StaticSelf = GenericParam<'static>;
}

impl<'db> sage_stash::AllocStashData<'db> for GenericParam<'db> {}
