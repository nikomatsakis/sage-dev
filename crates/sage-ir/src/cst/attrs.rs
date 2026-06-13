use sage_stash::{AllocStashData, Slice};

use crate::name::Name;
use crate::span::RelativeSpan;

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, AllocStashData)]
pub struct AttrCst<'db> {
    pub kind: AttrCstKind,
    pub path: Slice<Name<'db>>,
    /// For normal attrs: arguments inside parens as interned text.
    /// For doc comments: the comment text.
    pub args: Option<Name<'db>>,
    pub is_inner: bool,
    pub span: RelativeSpan,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, AllocStashData)]
pub enum AttrCstKind {
    Normal,
    DocComment,
}
