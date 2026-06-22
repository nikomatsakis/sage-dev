use sage_stash::{AllocStashData, Ptr, Slice};

use crate::cst::paths::Path;
use crate::span::RelativeSpan;

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, AllocStashData)]
pub struct AttrCst<'db> {
    pub kind: AttrCstKind,
    pub path: Ptr<Path<'db>>,
    /// Raw token tree bytes (including delimiters) for normal attrs,
    /// or comment text bytes for doc comments. Empty if no arguments.
    pub args: Slice<u8>,
    pub is_inner: bool,
    pub span: RelativeSpan,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, AllocStashData)]
pub enum AttrCstKind {
    Normal,
    DocComment,
}
