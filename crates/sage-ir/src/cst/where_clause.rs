use sage_stash::{AllocStashData, Ptr, Slice};

use crate::cst::generics::TypeBoundCst;
use crate::cst::ty::TypeCst;
use crate::span::RelativeSpan;

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, AllocStashData)]
pub struct WhereClauseCst<'db> {
    pub subject: Ptr<TypeCst<'db>>,
    pub bounds: Slice<TypeBoundCst<'db>>,
    pub span: RelativeSpan,
}
