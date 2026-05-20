use sage_stash::{AllocStashData, Slice, StashDirect, Stashed};

use crate::name::Name;
use crate::span::RelativeSpan;

/// A path as written in source: `std::collections::HashMap`.
#[salsa::tracked(debug)]
pub struct Path<'db> {
    #[returns(ref)]
    pub segments: Vec<Name<'db>>,
    pub span: RelativeSpan,
}

/// Unresolved type as written in source.
#[salsa::tracked(debug)]
pub struct TypeRef<'db> {
    pub kind: TypeRefKind<'db>,
    pub span: RelativeSpan,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, salsa::Update)]
pub enum TypeRefKind<'db> {
    Path(Path<'db>),
    Reference(TypeRef<'db>, Mutability),
    Slice(TypeRef<'db>),
    Array(TypeRef<'db>),
    Tuple(TupleTypeRef<'db>),
    Never,
    Infer,
    /// Lowering encountered an unexpected or unsupported node.
    Error,
}

/// Wrapper for tuple type refs — salsa tracked structs can't hold Vec directly
/// in an enum variant, so we use a separate tracked struct.
#[salsa::tracked(debug)]
pub struct TupleTypeRef<'db> {
    #[returns(ref)]
    pub elements: Vec<TypeRef<'db>>,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, salsa::Update)]
pub enum Mutability {
    Shared,
    Mut,
}

impl StashDirect for Mutability {}

/// The syntactic form of an attribute.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, salsa::Update)]
pub enum AttrKind {
    /// `#[path(args)]` or `#![path(args)]`
    Normal,
    /// `/// text` or `/** text */`
    DocComment,
}

/// A single flattened use import (stash-allocated).
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, AllocStashData)]
pub struct UseImportAst<'db> {
    /// The full path as written, e.g. [foo, bar].
    pub path: Slice<Name<'db>>,
    pub kind: UseKind<'db>,
    pub span: RelativeSpan,
}

/// The stashed collection of use imports for a `use` declaration.
pub type UseImports<'db> = Stashed<Slice<UseImportAst<'db>>>;

/// What a use import brings into scope.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, salsa::Update, AllocStashData)]
pub enum UseKind<'db> {
    /// `use foo::bar` or `use foo::bar as baz` — imports under the given name.
    Named(Name<'db>),
    /// `use foo::bar::*` — glob import.
    Glob,
    /// `use foo::Bar as _` — unnamed import.
    Unnamed,
}

/// An attribute: `#[foo]`, `#[derive(Debug)]`, `/// doc comment`, etc.
#[salsa::tracked(debug)]
pub struct Attr<'db> {
    pub kind: AttrKind,
    /// For normal attrs: the path (`derive`, `cfg`, etc.).
    /// For doc comments: path is `doc`.
    pub path: Path<'db>,
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
