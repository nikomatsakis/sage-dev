#![feature(register_tool)]
#![register_tool(sage)]
//! Build-time correspondence checks and runtime contracts for Sage's design.
mod browser;
pub mod check;
pub mod contracts;
pub mod correspondence;
pub mod evidence;
mod parser;
pub mod review;
#[cfg(feature = "salsa")]
pub mod salsa_contracts;
pub mod spec;
pub mod workspace;
pub use browser::{
    render_book_browser, render_browser, render_browser_with_workspace, render_spec_markdown,
};
pub use check::{CheckReport, check};
mod validate;

pub use parser::parse;
pub use validate::validate;

/// A byte range in the original UTF-8 source.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
pub struct Span {
    pub start: usize,
    pub end: usize,
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
pub struct Diagnostic {
    pub file: String,
    pub span: Span,
    pub message: String,
}

impl std::fmt::Display for Diagnostic {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{}:{}..{}: {}",
            self.file, self.span.start, self.span.end, self.message
        )
    }
}
impl std::error::Error for Diagnostic {}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
pub struct Path {
    /// `crate`, `self`, an external crate prefixed with `$`, or a local type name.
    pub root: String,
    pub segments: Vec<String>,
    pub span: Span,
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
#[sage::item(crate::language::Document)]
pub struct Document {
    pub file: String,
    pub header: Header,
    pub declarations: Vec<Declaration>,
    /// Triple-quoted Markdown with exact source provenance and local entity ownership.
    pub prose: Vec<ProseBlock>,
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
pub struct ProseBlock {
    pub text: String,
    /// Contents only, excluding the triple quotes.
    pub span: Span,
    /// Declaration/member path relative to this document's model namespace.
    pub owner: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
pub enum Header {
    Crate {
        name: String,
        purpose: String,
        dependencies: Vec<String>,
        modules: Vec<String>,
    },
    Module(Path),
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
pub enum Declaration {
    Type(TypeDecl),
    Class(Class),
}

/// Semantic value declarations; recursive values have no storage-layout restrictions.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
pub struct TypeDecl {
    pub name: String,
    pub parameters: Vec<String>,
    pub description: Option<String>,
    pub shape: TypeShape,
    pub span: Span,
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
pub enum TypeShape {
    Opaque,
    Struct(Vec<ValueField>),
    Enum(Vec<Variant>),
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
pub struct ValueField {
    pub name: String,
    pub description: Option<String>,
    pub ty: TypeExpr,
    pub span: Span,
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
pub struct Variant {
    pub name: String,
    pub description: Option<String>,
    pub fields: VariantFields,
    pub span: Span,
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
pub enum VariantFields {
    Unit,
    Tuple(Vec<TypeExpr>),
    Named(Vec<ValueField>),
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
pub struct TypeExpr {
    pub kind: TypeExprKind,
    pub span: Span,
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
pub enum TypeExprKind {
    Named {
        path: Path,
        arguments: Vec<TypeExpr>,
    },
    Tuple(Vec<TypeExpr>),
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
pub enum ClassKind {
    Class,
    Abstract,
    Context,
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
pub struct Class {
    pub name: String,
    pub kind: ClassKind,
    pub base: Option<Path>,
    pub description: String,
    pub fields: Vec<Field>,
    pub methods: Vec<Method>,
    pub span: Span,
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
pub enum FieldKind {
    Identity,
    Input,
    State,
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
pub struct Field {
    pub name: String,
    pub kind: FieldKind,
    pub ty: TypeExpr,
    pub span: Span,
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
pub enum MethodKind {
    Function,
    Query,
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
pub struct Method {
    pub name: String,
    pub kind: MethodKind,
    pub is_override: bool,
    pub result: TypeExpr,
    pub description: String,
    /// None is an abstract declaration; Some(empty) is a leaf computation.
    pub envelope: Option<Vec<Effect>>,
    pub span: Span,
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
pub enum Effect {
    Invokes(Path),
    ForbidTransitive(Path),
    Reads(Path),
    Creates { name: String, context: Path },
}

// Source anchors for architectural modules and distributed representations.
mod sage_model {
    /// Architectural crate anchor.
    #[sage::item(crate)]
    mod crate_model {}
    /// Architectural module anchor.
    #[sage::item(crate::language)]
    mod language_model {}
    /// Logical representation; implementation details are documented in the Sage declaration.
    #[sage::item(crate::language::Diagnostics)]
    mod language_diagnostics_model {}
    /// Groups the parse and validate free functions, not a Rust type.
    #[sage::item(crate::language::Language)]
    mod language_language_model {}
}
