use sage_stash::{AllocStashData, Ptr, Slice, Stashed};

use crate::cst::generics::GenericParamCst;
use crate::cst::structs::FieldCst;
use crate::cst::ty::TypeCst;
use crate::cst::where_clause::WhereClauseCst;
use crate::name::Name;
use crate::span::RelativeSpan;

pub type EnumCst<'db> = Stashed<Ptr<EnumCstData<'db>>>;

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, AllocStashData)]
pub struct EnumCstData<'db> {
    pub name: Name<'db>,
    pub generics: Slice<GenericParamCst<'db>>,
    pub variants: Slice<VariantCst<'db>>,
    pub where_clauses: Slice<WhereClauseCst<'db>>,
    pub span: RelativeSpan,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, AllocStashData)]
pub struct VariantCst<'db> {
    pub name: Name<'db>,
    pub fields: Slice<FieldCst<'db>>,
    pub discriminant: Option<Ptr<TypeCst<'db>>>,
    pub span: RelativeSpan,
}
