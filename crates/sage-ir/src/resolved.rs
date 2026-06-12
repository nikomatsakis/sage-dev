use sage_stash::{AllocStashData, Ptr, Slice, Stashed};

use crate::body::{BinaryOp, Literal, UnaryOp};
use crate::name::Name;
use crate::sig_ast::TypeRefAst;
use crate::span::RelativeSpan;
use crate::symbol::Symbol;
use crate::types::{Mutability, TokenTree};

/// A resolved function body stored in a `Stash`.
pub type ResolvedBody<'db> = Stashed<Ptr<CheckedBody<'db>>>;

/// What a path resolved to.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, AllocStashData)]
pub enum PathResolution<'db> {
    Def(Symbol<'db>),
    Local(LocalId),
    Err,
}

/// Short alias for use in match arms and type signatures.
pub type Res<'db> = PathResolution<'db>;

/// Identifies a local variable within a function body.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, AllocStashData)]
pub struct LocalId(pub u32);

/// Metadata about a local variable.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, AllocStashData)]
pub struct LocalVar<'db> {
    pub name: Name<'db>,
    pub span: RelativeSpan,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, AllocStashData)]
pub struct CheckedBody<'db> {
    pub root: Ptr<CheckedExpr<'db>>,
    pub locals: Slice<LocalVar<'db>>,
    pub span: RelativeSpan,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, AllocStashData)]
pub struct CheckedExpr<'db> {
    pub kind: CheckedExprKind<'db>,
    pub span: RelativeSpan,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, AllocStashData)]
pub enum CheckedExprKind<'db> {
    Literal(Literal),
    Path(Res<'db>),
    Block(Slice<CheckedStmt<'db>>, Option<Ptr<CheckedExpr<'db>>>),
    Call(Ptr<CheckedExpr<'db>>, Slice<CheckedExpr<'db>>),
    MethodCall(Ptr<CheckedExpr<'db>>, Name<'db>, Slice<CheckedExpr<'db>>),
    Field(Ptr<CheckedExpr<'db>>, Name<'db>),
    Binary(Ptr<CheckedExpr<'db>>, BinaryOp, Ptr<CheckedExpr<'db>>),
    Unary(UnaryOp, Ptr<CheckedExpr<'db>>),
    Ref(Ptr<CheckedExpr<'db>>, Mutability),
    If(
        Ptr<CheckedExpr<'db>>,
        Ptr<CheckedExpr<'db>>,
        Option<Ptr<CheckedExpr<'db>>>,
    ),
    IfLet(
        Ptr<CheckedPat<'db>>,
        Ptr<CheckedExpr<'db>>,
        Ptr<CheckedExpr<'db>>,
        Option<Ptr<CheckedExpr<'db>>>,
    ),
    Match(Ptr<CheckedExpr<'db>>, Slice<CheckedMatchArm<'db>>),
    Loop(Ptr<CheckedExpr<'db>>),
    While(Ptr<CheckedExpr<'db>>, Ptr<CheckedExpr<'db>>),
    WhileLet(
        Ptr<CheckedPat<'db>>,
        Ptr<CheckedExpr<'db>>,
        Ptr<CheckedExpr<'db>>,
    ),
    For(
        Ptr<CheckedPat<'db>>,
        Ptr<CheckedExpr<'db>>,
        Ptr<CheckedExpr<'db>>,
    ),
    Break(Option<Ptr<CheckedExpr<'db>>>),
    Continue,
    Return(Option<Ptr<CheckedExpr<'db>>>),
    Assign(Ptr<CheckedExpr<'db>>, Ptr<CheckedExpr<'db>>),
    Await(Ptr<CheckedExpr<'db>>),
    Try(Ptr<CheckedExpr<'db>>),
    Closure(Slice<CheckedClosureParam<'db>>, Ptr<CheckedExpr<'db>>),
    Tuple(Slice<CheckedExpr<'db>>),
    Array(Slice<CheckedExpr<'db>>),
    Index(Ptr<CheckedExpr<'db>>, Ptr<CheckedExpr<'db>>),
    Cast(Ptr<CheckedExpr<'db>>, Ptr<TypeRefAst<'db>>),
    StructLit(Res<'db>, Slice<CheckedFieldInit<'db>>),
    Range(Option<Ptr<CheckedExpr<'db>>>, Option<Ptr<CheckedExpr<'db>>>),
    MacroCall(Res<'db>, TokenTree<'db>),
    Missing,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, AllocStashData)]
pub struct CheckedStmt<'db> {
    pub kind: CheckedStmtKind<'db>,
    pub span: RelativeSpan,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, AllocStashData)]
pub enum CheckedStmtKind<'db> {
    Let(
        Ptr<CheckedPat<'db>>,
        Option<Ptr<TypeRefAst<'db>>>,
        Option<Ptr<CheckedExpr<'db>>>,
    ),
    Expr(Ptr<CheckedExpr<'db>>),
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, AllocStashData)]
pub struct CheckedPat<'db> {
    pub kind: CheckedPatKind<'db>,
    pub span: RelativeSpan,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, AllocStashData)]
pub enum CheckedPatKind<'db> {
    Wildcard,
    Bind(LocalId, Mutability),
    Path(Res<'db>),
    Tuple(Slice<CheckedPat<'db>>),
    Struct(Res<'db>, Slice<CheckedFieldPat<'db>>),
    TupleStruct(Res<'db>, Slice<CheckedPat<'db>>),
    Ref(Ptr<CheckedPat<'db>>, Mutability),
    Literal(Literal),
    Or(Slice<CheckedPat<'db>>),
    Rest,
    Missing,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, AllocStashData)]
pub struct CheckedFieldInit<'db> {
    pub name: Name<'db>,
    pub value: Ptr<CheckedExpr<'db>>,
    pub span: RelativeSpan,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, AllocStashData)]
pub struct CheckedFieldPat<'db> {
    pub name: Name<'db>,
    pub pat: Ptr<CheckedPat<'db>>,
    pub span: RelativeSpan,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, AllocStashData)]
pub struct CheckedMatchArm<'db> {
    pub pat: Ptr<CheckedPat<'db>>,
    pub guard: Option<Ptr<CheckedExpr<'db>>>,
    pub body: Ptr<CheckedExpr<'db>>,
    pub span: RelativeSpan,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, AllocStashData)]
pub struct CheckedClosureParam<'db> {
    pub pat: Ptr<CheckedPat<'db>>,
    pub ty: Option<Ptr<TypeRefAst<'db>>>,
    pub span: RelativeSpan,
}
