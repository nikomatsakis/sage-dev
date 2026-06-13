use sage_stash::AllocStashData;

use crate::name::Name;
use crate::span::RelativeSpan;

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, AllocStashData)]
pub struct ExprCst<'db> {
    pub kind: ExprCstKind<'db>,
    pub span: RelativeSpan,
}

/// Placeholder — will be expanded as body lowering moves to CST.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, AllocStashData)]
pub enum ExprCstKind<'db> {
    Todo(Name<'db>),
}
