use sage_stash::{AllocStashData, Ptr, Slice, StashDirect, Stashed};

use crate::body::{BinaryOp, Literal, UnaryOp};
use crate::name::Name;
use crate::sig_ast::TypeRefAst;
use crate::span::RelativeSpan;
use crate::symbol::Symbol;
use crate::types::{Mutability, TokenTree};

/// A resolved function body stored in a `Stash`.
pub type ResolvedBody<'db> = Stashed<Ptr<RBody<'db>>>;

/// What a path resolved to.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, AllocStashData)]
pub enum Res<'db> {
    Def(Symbol<'db>),
    Local(LocalId),
    Err,
}

impl StashDirect for Res<'_> {}

/// Identifies a local variable within a function body.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, AllocStashData)]
pub struct LocalId(pub u32);

impl StashDirect for LocalId {}

/// Metadata about a local variable.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, AllocStashData)]
pub struct LocalVar<'db> {
    pub name: Name<'db>,
    pub span: RelativeSpan,
}

impl StashDirect for LocalVar<'_> {}

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, AllocStashData)]
pub struct RBody<'db> {
    pub root: Ptr<RExpr<'db>>,
    pub locals: Slice<LocalVar<'db>>,
    pub span: RelativeSpan,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, AllocStashData)]
pub struct RExpr<'db> {
    pub kind: RExprKind<'db>,
    pub span: RelativeSpan,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, AllocStashData)]
pub enum RExprKind<'db> {
    Literal(Literal),
    Path(Res<'db>),
    Block(Slice<RStmt<'db>>, Option<Ptr<RExpr<'db>>>),
    Call(Ptr<RExpr<'db>>, Slice<RExpr<'db>>),
    MethodCall(Ptr<RExpr<'db>>, Name<'db>, Slice<RExpr<'db>>),
    Field(Ptr<RExpr<'db>>, Name<'db>),
    Binary(Ptr<RExpr<'db>>, BinaryOp, Ptr<RExpr<'db>>),
    Unary(UnaryOp, Ptr<RExpr<'db>>),
    Ref(Ptr<RExpr<'db>>, Mutability),
    If(Ptr<RExpr<'db>>, Ptr<RExpr<'db>>, Option<Ptr<RExpr<'db>>>),
    IfLet(
        Ptr<RPat<'db>>,
        Ptr<RExpr<'db>>,
        Ptr<RExpr<'db>>,
        Option<Ptr<RExpr<'db>>>,
    ),
    Match(Ptr<RExpr<'db>>, Slice<RMatchArm<'db>>),
    Loop(Ptr<RExpr<'db>>),
    While(Ptr<RExpr<'db>>, Ptr<RExpr<'db>>),
    WhileLet(Ptr<RPat<'db>>, Ptr<RExpr<'db>>, Ptr<RExpr<'db>>),
    For(Ptr<RPat<'db>>, Ptr<RExpr<'db>>, Ptr<RExpr<'db>>),
    Break(Option<Ptr<RExpr<'db>>>),
    Continue,
    Return(Option<Ptr<RExpr<'db>>>),
    Assign(Ptr<RExpr<'db>>, Ptr<RExpr<'db>>),
    Await(Ptr<RExpr<'db>>),
    Try(Ptr<RExpr<'db>>),
    Closure(Slice<RClosureParam<'db>>, Ptr<RExpr<'db>>),
    Tuple(Slice<RExpr<'db>>),
    Array(Slice<RExpr<'db>>),
    Index(Ptr<RExpr<'db>>, Ptr<RExpr<'db>>),
    Cast(Ptr<RExpr<'db>>, Ptr<TypeRefAst<'db>>),
    StructLit(Res<'db>, Slice<RFieldInit<'db>>),
    Range(Option<Ptr<RExpr<'db>>>, Option<Ptr<RExpr<'db>>>),
    MacroCall(Res<'db>, TokenTree<'db>),
    Missing,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, AllocStashData)]
pub struct RStmt<'db> {
    pub kind: RStmtKind<'db>,
    pub span: RelativeSpan,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, AllocStashData)]
pub enum RStmtKind<'db> {
    Let(
        Ptr<RPat<'db>>,
        Option<Ptr<TypeRefAst<'db>>>,
        Option<Ptr<RExpr<'db>>>,
    ),
    Expr(Ptr<RExpr<'db>>),
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, AllocStashData)]
pub struct RPat<'db> {
    pub kind: RPatKind<'db>,
    pub span: RelativeSpan,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, AllocStashData)]
pub enum RPatKind<'db> {
    Wildcard,
    Bind(LocalId, Mutability),
    Path(Res<'db>),
    Tuple(Slice<RPat<'db>>),
    Struct(Res<'db>, Slice<RFieldPat<'db>>),
    TupleStruct(Res<'db>, Slice<RPat<'db>>),
    Ref(Ptr<RPat<'db>>, Mutability),
    Literal(Literal),
    Or(Slice<RPat<'db>>),
    Rest,
    Missing,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, AllocStashData)]
pub struct RFieldInit<'db> {
    pub name: Name<'db>,
    pub value: Ptr<RExpr<'db>>,
    pub span: RelativeSpan,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, AllocStashData)]
pub struct RFieldPat<'db> {
    pub name: Name<'db>,
    pub pat: Ptr<RPat<'db>>,
    pub span: RelativeSpan,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, AllocStashData)]
pub struct RMatchArm<'db> {
    pub pat: Ptr<RPat<'db>>,
    pub guard: Option<Ptr<RExpr<'db>>>,
    pub body: Ptr<RExpr<'db>>,
    pub span: RelativeSpan,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, AllocStashData)]
pub struct RClosureParam<'db> {
    pub pat: Ptr<RPat<'db>>,
    pub ty: Option<Ptr<TypeRefAst<'db>>>,
    pub span: RelativeSpan,
}
