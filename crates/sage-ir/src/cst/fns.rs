use sage_stash::{AllocStashData, Ptr, Slice, Stashed};

use crate::cst::expr::ExprCst;
use crate::cst::generics::GenericParamCst;
use crate::cst::ty::TypeCst;
use crate::cst::where_clause::WhereClauseCst;
use crate::name::Name;
use crate::span::RelativeSpan;

pub type FnCst<'db> = Stashed<Ptr<FnCstData<'db>>>;

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, AllocStashData)]
pub struct FnCstData<'db> {
    pub name: Name<'db>,
    pub generics: Slice<GenericParamCst<'db>>,
    pub params: Slice<ParamCst<'db>>,
    pub ret: Option<Ptr<TypeCst<'db>>>,
    pub body: Option<Ptr<ExprCst<'db>>>,
    pub where_clauses: Slice<WhereClauseCst<'db>>,
    pub span: RelativeSpan,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, AllocStashData)]
pub struct ParamCst<'db> {
    pub name: Option<Name<'db>>,
    pub ty: Ptr<TypeCst<'db>>,
    pub span: RelativeSpan,
}
