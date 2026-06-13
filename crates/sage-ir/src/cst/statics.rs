use sage_stash::{AllocStashData, Ptr, Stashed};

use crate::cst::expr::ExprCst;
use crate::cst::ty::TypeCst;
use crate::name::Name;
use crate::span::RelativeSpan;

pub type StaticCst<'db> = Stashed<Ptr<StaticCstData<'db>>>;

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, AllocStashData)]
pub struct StaticCstData<'db> {
    pub name: Name<'db>,
    pub is_mut: bool,
    pub ty: Option<Ptr<TypeCst<'db>>>,
    pub value: Option<Ptr<ExprCst<'db>>>,
    pub span: RelativeSpan,
}
