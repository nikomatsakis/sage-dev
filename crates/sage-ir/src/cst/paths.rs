use sage_stash::{AllocStashData, Slice};

use crate::cst::ty::TypeCst;
use crate::name::Name;
use crate::span::RelativeSpan;

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, AllocStashData)]
pub struct PathCst<'db> {
    pub segments: Slice<PathSegmentCst<'db>>,
    pub span: RelativeSpan,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, AllocStashData)]
pub struct PathSegmentCst<'db> {
    pub name: Name<'db>,
    pub type_args: Slice<TypeCst<'db>>,
    pub span: RelativeSpan,
}
