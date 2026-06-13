use sage_stash::{AllocStashData, Ptr, Slice, Stashed};

use crate::cst::generics::GenericParamCst;
use crate::cst::ty::TypeCst;
use crate::cst::where_clause::WhereClauseCst;
use crate::name::Name;
use crate::span::RelativeSpan;

pub type StructCst<'db> = Stashed<Ptr<StructCstData<'db>>>;

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, AllocStashData)]
pub struct StructCstData<'db> {
    pub name: Name<'db>,
    pub generics: Slice<GenericParamCst<'db>>,
    pub fields: Slice<FieldCst<'db>>,
    pub where_clauses: Slice<WhereClauseCst<'db>>,
    pub span: RelativeSpan,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, AllocStashData)]
pub struct FieldCst<'db> {
    pub name: Name<'db>,
    pub ty: Ptr<TypeCst<'db>>,
    pub span: RelativeSpan,
}
