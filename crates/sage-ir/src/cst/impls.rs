use sage_stash::{AllocStashData, Ptr, Slice, Stashed};

use crate::cst::attrs::AttrCst;
use crate::cst::generics::GenericParamCst;
use crate::cst::paths::Path;
use crate::cst::traits::TraitItemCst;
use crate::cst::ty::TypeCst;
use crate::cst::where_clause::WhereClauseCst;
use crate::span::RelativeSpan;

pub type ImplCst<'db> = Stashed<Ptr<ImplCstData<'db>>>;

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, AllocStashData)]
pub struct ImplCstData<'db> {
    pub attrs: Slice<AttrCst<'db>>,
    pub generics: Slice<GenericParamCst<'db>>,
    pub self_ty: Ptr<TypeCst<'db>>,
    pub trait_path: Option<Ptr<Path<'db>>>,
    pub where_clauses: Slice<WhereClauseCst<'db>>,
    pub items: Slice<TraitItemCst<'db>>,
    pub span: RelativeSpan,
}
