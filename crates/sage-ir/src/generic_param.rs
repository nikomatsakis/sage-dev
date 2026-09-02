//! Generic parameters as salsa-tracked symbols.
//!
//! Each generic parameter (type, lifetime, const) becomes a stable salsa identity.
//! Types reference these directly via `Ty::Param(GenericParam)` rather than
//! using de Bruijn indices.

use crate::name::Name;
use crate::span::RelativeSpan;
use crate::symbol::Symbol;

// ---------------------------------------------------------------------------
// GenericParamKind
// ---------------------------------------------------------------------------

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, salsa::Update, sage_reflect::Reflect)]
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
    #[tracked]
    pub span: RelativeSpan,
    pub parent: Symbol<'db>,
    pub index: u32,
}

impl sage_stash::StashDirect for AstGenericParam<'_> {}

// Safety: `AstGenericParam` is `Copy`, and `StaticSelf` changes only the Salsa
// database lifetime carried by its handles; it contains no borrowed data.
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

// Safety: `ExtGenericParam` is `Copy`, and `StaticSelf` changes only the Salsa
// database lifetime carried by its handles; it contains no borrowed data.
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

// Safety: `AlphaEquivParam` is `Copy`, and `StaticSelf` changes only the Salsa
// database lifetime carried by its handles; it contains no borrowed data.
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

// Safety: `GenericParam` is `Copy`, and `StaticSelf` changes only the Salsa
// database lifetime carried by its handles; it contains no borrowed data.
unsafe impl<'db> sage_stash::StashData<'db> for GenericParam<'db> {
    type StaticSelf = GenericParam<'static>;
}

impl<'db> sage_stash::AllocStashData<'db> for GenericParam<'db> {}

impl<'db> sage_reflect::Reflect<'db> for GenericParam<'db> {
    fn reflect(
        &self,
        context: &mut sage_reflect::ReflectionContext<'_>,
        _stash: Option<&sage_stash::Stash>,
    ) -> sage_reflect::ValueNode {
        use salsa::plumbing::AsId;

        let (family, id) = match self {
            GenericParam::Ast(param) => ("generic-param-ast", param.as_id()),
            GenericParam::Ext(param) => ("generic-param-external", param.as_id()),
            GenericParam::AlphaEquiv(param) => ("generic-param-alpha", param.as_id()),
        };
        let key = sage_reflect::ReferenceKey {
            family,
            id: id.as_bits(),
        };
        context.reflect_node("GenericParam", |context| {
            context.reflected_value(&key).unwrap_or_else(|| {
                sage_reflect::ValueNode::scalar("GenericParam", format!("{family}:{}", key.id))
            })
        })
    }
}
