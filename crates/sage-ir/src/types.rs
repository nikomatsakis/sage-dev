use sage_stash::StashDirect;

use crate::name::Name;
use crate::span::RelativeSpan;

/// The syntactic form of an attribute.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, salsa::Update)]
pub enum AttrKind {
    /// `#[path(args)]` or `#![path(args)]`
    Normal,
    /// `/// text` or `/** text */`
    DocComment,
}

/// An attribute: `#[foo]`, `#[derive(Debug)]`, `/// doc comment`, etc.
#[salsa::tracked(debug)]
pub struct Attr<'db> {
    pub kind: AttrKind,
    /// For normal attrs: the path (`derive`, `cfg`, etc.).
    /// For doc comments: path is `doc`.
    #[returns(ref)]
    pub path: Vec<Name<'db>>,
    /// For normal attrs: the arguments inside parens, if any.
    /// For doc comments: the comment text.
    pub args: Option<TokenTree<'db>>,
    pub span: RelativeSpan,
    /// True for inner attributes (`#![...]`) or inner doc comments (`//!`).
    pub is_inner: bool,
}

/// Raw token tree — the unparsed arguments of a macro invocation or attribute.
#[salsa::tracked(debug)]
pub struct TokenTree<'db> {
    #[returns(ref)]
    pub text: String,
    pub span: RelativeSpan,
}

impl StashDirect for TokenTree<'_> {}
