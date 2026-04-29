use sage_stash::{AllocStashData, Ptr, Slice, Stash, Stashed};

use crate::item::FunctionItem;
use crate::name::Name;
use crate::span::SpanIndices;
use crate::types::{Mutability, Path, TypeRef};

/// Function body: `Stashed<Ptr<Expr<'db>>>`.
/// The stash owns all expressions, statements, and patterns.
/// `PartialEq` compares by value through the stash.
pub type FunctionBody<'db> = Stashed<Ptr<Expr<'db>>>;

/// On-demand body lowering. Re-walks the CST to build the body.
#[salsa::tracked(returns(ref))]
pub fn function_body<'db>(db: &'db dyn crate::Db, func: FunctionItem<'db>) -> FunctionBody<'db> {
    // TODO: implement CST → body lowering
    let _ = (db, func);
    Stashed::new(Stash::new(), Ptr::DANGLING)
}

// -- Arena-allocated body types --

#[derive(Copy, Clone, PartialEq, Eq, Hash, AllocStashData)]
pub struct Expr<'db> {
    pub kind: ExprKind<'db>,
    pub span: SpanIndices,
}

#[derive(Copy, Clone, PartialEq, Eq, Hash, AllocStashData)]
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
    MacroCall(Path<'db>),
    Missing,
}

/// Closure parameter (arena-allocated, distinct from signature `types::Param`).
#[derive(Copy, Clone, PartialEq, Eq, Hash, AllocStashData)]
pub struct ClosureParam<'db> {
    pub pat: Ptr<Pat<'db>>,
    pub ty: Option<TypeRef<'db>>,
    pub span: SpanIndices,
}

#[derive(Copy, Clone, PartialEq, Eq, Hash, AllocStashData)]
pub struct FieldInit<'db> {
    pub name: Name<'db>,
    pub value: Ptr<Expr<'db>>,
    pub span: SpanIndices,
}

#[derive(Copy, Clone, PartialEq, Eq, Hash, AllocStashData)]
pub struct MatchArm<'db> {
    pub pat: Ptr<Pat<'db>>,
    pub guard: Option<Ptr<Expr<'db>>>,
    pub body: Ptr<Expr<'db>>,
    pub span: SpanIndices,
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

// -- Statements --

#[derive(Copy, Clone, PartialEq, Eq, Hash, AllocStashData)]
pub struct Stmt<'db> {
    pub kind: StmtKind<'db>,
    pub span: SpanIndices,
}

#[derive(Copy, Clone, PartialEq, Eq, Hash, AllocStashData)]
pub enum StmtKind<'db> {
    Let(Ptr<Pat<'db>>, Option<TypeRef<'db>>, Option<Ptr<Expr<'db>>>),
    Expr(Ptr<Expr<'db>>),
}

// -- Patterns --

#[derive(Copy, Clone, PartialEq, Eq, Hash, AllocStashData)]
pub struct Pat<'db> {
    pub kind: PatKind<'db>,
    pub span: SpanIndices,
}

#[derive(Copy, Clone, PartialEq, Eq, Hash, AllocStashData)]
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

#[derive(Copy, Clone, PartialEq, Eq, Hash, AllocStashData)]
pub struct FieldPat<'db> {
    pub name: Name<'db>,
    pub pat: Ptr<Pat<'db>>,
    pub span: SpanIndices,
}
