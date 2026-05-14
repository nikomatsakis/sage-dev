use sage_stash::{AllocStashData, Ptr, Slice, Stashed};

use crate::name::Name;
use crate::span::RelativeSpan;
use crate::types::{Mutability, Path, TokenTree, TypeRef};

/// A function body stored in a `Stash`.
pub type FunctionBody<'db> = Stashed<Ptr<Body<'db>>>;

/// The root of a function body.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, AllocStashData)]
pub struct Body<'db> {
    pub root: Ptr<Expr<'db>>,
    pub span: RelativeSpan,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, AllocStashData)]
pub struct Expr<'db> {
    pub kind: ExprKind<'db>,
    pub span: RelativeSpan,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, AllocStashData)]
pub enum ExprKind<'db> {
    Literal(Literal),
    Path(Path<'db>),
    Block(Slice<Stmt<'db>>, Option<Ptr<Expr<'db>>>),
    Call(Ptr<Expr<'db>>, Slice<Expr<'db>>),
    MethodCall(Ptr<Expr<'db>>, Name<'db>, Slice<Expr<'db>>),
    Field(Ptr<Expr<'db>>, Name<'db>),
    Binary(Ptr<Expr<'db>>, BinaryOp, Ptr<Expr<'db>>),
    Unary(UnaryOp, Ptr<Expr<'db>>),
    Ref(Ptr<Expr<'db>>, Mutability),
    If(Ptr<Expr<'db>>, Ptr<Expr<'db>>, Option<Ptr<Expr<'db>>>),
    Match(Ptr<Expr<'db>>, Slice<MatchArm<'db>>),
    Loop(Ptr<Expr<'db>>),
    While(Ptr<Expr<'db>>, Ptr<Expr<'db>>),
    For(Ptr<Pat<'db>>, Ptr<Expr<'db>>, Ptr<Expr<'db>>),
    Break(Option<Ptr<Expr<'db>>>),
    Continue,
    Return(Option<Ptr<Expr<'db>>>),
    Assign(Ptr<Expr<'db>>, Ptr<Expr<'db>>),
    Await(Ptr<Expr<'db>>),
    Try(Ptr<Expr<'db>>),
    Closure(Slice<ClosureParam<'db>>, Ptr<Expr<'db>>),
    Tuple(Slice<Expr<'db>>),
    Array(Slice<Expr<'db>>),
    Index(Ptr<Expr<'db>>, Ptr<Expr<'db>>),
    Cast(Ptr<Expr<'db>>, TypeRef<'db>),
    StructLit(Path<'db>, Slice<FieldInit<'db>>),
    Range(Option<Ptr<Expr<'db>>>, Option<Ptr<Expr<'db>>>),
    MacroCall(Path<'db>, TokenTree<'db>),
    IfLet(
        Ptr<Pat<'db>>,
        Ptr<Expr<'db>>,
        Ptr<Expr<'db>>,
        Option<Ptr<Expr<'db>>>,
    ),
    WhileLet(Ptr<Pat<'db>>, Ptr<Expr<'db>>, Ptr<Expr<'db>>),
    Missing,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, AllocStashData)]
pub struct ClosureParam<'db> {
    pub pat: Ptr<Pat<'db>>,
    pub ty: Option<TypeRef<'db>>,
    pub span: RelativeSpan,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, AllocStashData)]
pub struct FieldInit<'db> {
    pub name: Name<'db>,
    pub value: Ptr<Expr<'db>>,
    pub span: RelativeSpan,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, AllocStashData)]
pub struct MatchArm<'db> {
    pub pat: Ptr<Pat<'db>>,
    pub guard: Option<Ptr<Expr<'db>>>,
    pub body: Ptr<Expr<'db>>,
    pub span: RelativeSpan,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub enum Literal {
    Int,
    Float,
    String,
    Bool(bool),
    Char,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub enum BinaryOp {
    Add,
    Sub,
    Mul,
    Div,
    Rem,
    And,
    Or,
    BitAnd,
    BitOr,
    BitXor,
    Shl,
    Shr,
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub enum UnaryOp {
    Not,
    Neg,
    Deref,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, AllocStashData)]
pub struct Stmt<'db> {
    pub kind: StmtKind<'db>,
    pub span: RelativeSpan,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, AllocStashData)]
pub enum StmtKind<'db> {
    Let(Ptr<Pat<'db>>, Option<TypeRef<'db>>, Option<Ptr<Expr<'db>>>),
    Expr(Ptr<Expr<'db>>),
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, AllocStashData)]
pub struct Pat<'db> {
    pub kind: PatKind<'db>,
    pub span: RelativeSpan,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, AllocStashData)]
pub enum PatKind<'db> {
    Wildcard,
    Bind(Name<'db>, Mutability),
    Path(Path<'db>),
    Tuple(Slice<Pat<'db>>),
    Struct(Path<'db>, Slice<FieldPat<'db>>),
    TupleStruct(Path<'db>, Slice<Pat<'db>>),
    Ref(Ptr<Pat<'db>>, Mutability),
    Literal(Literal),
    Or(Slice<Pat<'db>>),
    Rest,
    Missing,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, AllocStashData)]
pub struct FieldPat<'db> {
    pub name: Name<'db>,
    pub pat: Ptr<Pat<'db>>,
    pub span: RelativeSpan,
}
