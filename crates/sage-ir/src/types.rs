use crate::name::Name;
use crate::span::SpanIndices;

/// A path as written in source: `std::collections::HashMap`.
#[salsa::tracked]
pub struct Path<'db> {
    #[returns(ref)]
    pub segments: Vec<Name<'db>>,
    pub span: SpanIndices,
}

/// Unresolved type as written in source.
#[salsa::tracked]
pub struct TypeRef<'db> {
    pub kind: TypeRefKind<'db>,
    pub span: SpanIndices,
}

#[derive(Copy, Clone, PartialEq, Eq, Hash, salsa::Update)]
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
#[salsa::tracked]
pub struct TupleTypeRef<'db> {
    #[returns(ref)]
    pub elements: Vec<TypeRef<'db>>,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, salsa::Update)]
pub enum Mutability {
    Shared,
    Mut,
}

/// A function parameter's signature-level data.
#[salsa::tracked]
pub struct Param<'db> {
    pub name: Option<Name<'db>>,
    pub ty: TypeRef<'db>,
    pub span: SpanIndices,
}

/// A struct/enum field.
#[salsa::tracked]
pub struct FieldDef<'db> {
    pub name: Name<'db>,
    pub ty: TypeRef<'db>,
    pub span: SpanIndices,
}

/// An enum variant.
#[salsa::tracked]
pub struct VariantDef<'db> {
    pub name: Name<'db>,
    #[returns(ref)]
    pub fields: Vec<FieldDef<'db>>,
    pub span: SpanIndices,
}

/// The syntactic form of an attribute.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, salsa::Update)]
pub enum AttrKind {
    /// `#[path(args)]` or `#![path(args)]`
    Normal,
    /// `/// text` or `/** text */`
    DocComment,
}

/// An attribute: `#[foo]`, `#[derive(Debug)]`, `/// doc comment`, etc.
#[salsa::tracked]
pub struct Attr<'db> {
    pub kind: AttrKind,
    /// For normal attrs: the path (`derive`, `cfg`, etc.).
    /// For doc comments: path is `doc`.
    pub path: Path<'db>,
    /// For normal attrs: the arguments inside parens, if any.
    /// For doc comments: the comment text.
    pub args: Option<TokenTree<'db>>,
    pub span: SpanIndices,
    /// True for inner attributes (`#![...]`) or inner doc comments (`//!`).
    pub is_inner: bool,
}

/// Raw token tree — the unparsed arguments of a macro invocation or attribute.
#[salsa::tracked]
pub struct TokenTree<'db> {
    #[returns(ref)]
    pub text: String,
    pub span: SpanIndices,
}
