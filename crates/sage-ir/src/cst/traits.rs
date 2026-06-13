use sage_stash::{AllocStashData, Ptr, Slice, Stashed};

use crate::cst::attrs::AttrCst;
use crate::cst::consts::ConstCstData;
use crate::cst::fns::FnCstData;
use crate::cst::generics::GenericParamCst;
use crate::cst::type_aliases::TypeAliasCstData;
use crate::cst::where_clause::WhereClauseCst;
use crate::name::Name;
use crate::span::RelativeSpan;

pub type TraitCst<'db> = Stashed<Ptr<TraitCstData<'db>>>;

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, AllocStashData)]
pub struct TraitCstData<'db> {
    pub attrs: Slice<AttrCst<'db>>,
    pub name: Name<'db>,
    pub generics: Slice<GenericParamCst<'db>>,
    pub where_clauses: Slice<WhereClauseCst<'db>>,
    pub items: Slice<TraitItemCst<'db>>,
    pub span: RelativeSpan,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, AllocStashData)]
pub enum TraitItemCst<'db> {
    Fn(Ptr<FnCstData<'db>>),
    Type(Ptr<TypeAliasCstData<'db>>),
    Const(Ptr<ConstCstData<'db>>),
}
