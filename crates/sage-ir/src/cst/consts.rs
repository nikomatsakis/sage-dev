use sage_stash::{AllocStashData, Ptr, Stashed};

use crate::cst::expr::ExprCst;
use crate::cst::ty::TypeCst;
use crate::name::Name;
use crate::span::RelativeSpan;

pub type ConstCst<'db> = Stashed<Ptr<ConstCstData<'db>>>;

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, AllocStashData)]
pub struct ConstCstData<'db> {
    pub name: Name<'db>,
    pub ty: Option<Ptr<TypeCst<'db>>>,
    pub value: Option<Ptr<ExprCst<'db>>>,
    pub span: RelativeSpan,
}
