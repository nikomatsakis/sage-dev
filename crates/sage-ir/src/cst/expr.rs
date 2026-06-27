use sage_stash::{AllocStashData, Ptr, Slice, StashDirect};

use crate::cst::Mutability;
use crate::cst::paths::Path;
use crate::cst::ty::TypeCst;
use crate::name::Name;
use crate::span::RelativeSpan;

// ---------------------------------------------------------------------------
// Expression primitives (shared with tytree)
// ---------------------------------------------------------------------------

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub enum Literal<'db> {
    Int(Name<'db>),
    Float(Name<'db>),
    String(Name<'db>),
    Bool(bool),
    Char(Name<'db>),
}

impl StashDirect for Literal<'_> {}

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

impl StashDirect for BinaryOp {}

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub enum UnaryOp {
    Not,
    Neg,
    Deref,
}

impl StashDirect for UnaryOp {}

// ---------------------------------------------------------------------------
// CST expression nodes
// ---------------------------------------------------------------------------

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, AllocStashData)]
pub struct ExprCst<'db> {
    pub kind: ExprCstKind<'db>,
    pub span: RelativeSpan,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, AllocStashData)]
pub enum ExprCstKind<'db> {
    Literal(Literal<'db>),
    Path(Ptr<Path<'db>>),
    Block(Slice<StmtCst<'db>>, Option<Ptr<ExprCst<'db>>>),
    Call(Ptr<ExprCst<'db>>, Slice<ExprCst<'db>>),
    MethodCall(Ptr<ExprCst<'db>>, Name<'db>, Slice<ExprCst<'db>>),
    Field(Ptr<ExprCst<'db>>, Name<'db>),
    Binary(Ptr<ExprCst<'db>>, BinaryOp, Ptr<ExprCst<'db>>),
    Unary(UnaryOp, Ptr<ExprCst<'db>>),
    Ref(Ptr<ExprCst<'db>>, Mutability),
    If(
        Ptr<ExprCst<'db>>,
        Ptr<ExprCst<'db>>,
        Option<Ptr<ExprCst<'db>>>,
    ),
    Match(Ptr<ExprCst<'db>>, Slice<MatchArmCst<'db>>),
    Loop(Ptr<ExprCst<'db>>),
    While(Ptr<ExprCst<'db>>, Ptr<ExprCst<'db>>),
    For(Ptr<PatCst<'db>>, Ptr<ExprCst<'db>>, Ptr<ExprCst<'db>>),
    Break(Option<Ptr<ExprCst<'db>>>),
    Continue,
    Return(Option<Ptr<ExprCst<'db>>>),
    Assign(Ptr<ExprCst<'db>>, Ptr<ExprCst<'db>>),
    Await(Ptr<ExprCst<'db>>),
    Try(Ptr<ExprCst<'db>>),
    Closure(Slice<ClosureParamCst<'db>>, Ptr<ExprCst<'db>>),
    Tuple(Slice<ExprCst<'db>>),
    Array(Slice<ExprCst<'db>>),
    Index(Ptr<ExprCst<'db>>, Ptr<ExprCst<'db>>),
    Cast(Ptr<ExprCst<'db>>, Ptr<TypeCst<'db>>),
    StructLit(Ptr<Path<'db>>, Slice<FieldInitCst<'db>>),
    Range(Option<Ptr<ExprCst<'db>>>, Option<Ptr<ExprCst<'db>>>),
    IfLet(
        Ptr<PatCst<'db>>,
        Ptr<ExprCst<'db>>,
        Ptr<ExprCst<'db>>,
        Option<Ptr<ExprCst<'db>>>,
    ),
    WhileLet(Ptr<PatCst<'db>>, Ptr<ExprCst<'db>>, Ptr<ExprCst<'db>>),
    Missing,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, AllocStashData)]
pub struct StmtCst<'db> {
    pub kind: StmtCstKind<'db>,
    pub span: RelativeSpan,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, AllocStashData)]
pub enum StmtCstKind<'db> {
    Let(
        Ptr<PatCst<'db>>,
        Option<Ptr<TypeCst<'db>>>,
        Option<Ptr<ExprCst<'db>>>,
    ),
    Expr(Ptr<ExprCst<'db>>),
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, AllocStashData)]
pub struct PatCst<'db> {
    pub kind: PatCstKind<'db>,
    pub span: RelativeSpan,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, AllocStashData)]
pub enum PatCstKind<'db> {
    Wildcard,
    Bind(Name<'db>, Mutability),
    Path(Ptr<Path<'db>>),
    Tuple(Slice<PatCst<'db>>),
    Struct(Ptr<Path<'db>>, Slice<FieldPatCst<'db>>),
    TupleStruct(Ptr<Path<'db>>, Slice<PatCst<'db>>),
    Ref(Ptr<PatCst<'db>>, Mutability),
    Literal(Literal<'db>),
    Or(Slice<PatCst<'db>>),
    Rest,
    Missing,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, AllocStashData)]
pub struct FieldPatCst<'db> {
    pub name: Name<'db>,
    pub pat: Ptr<PatCst<'db>>,
    pub span: RelativeSpan,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, AllocStashData)]
pub struct MatchArmCst<'db> {
    pub pat: Ptr<PatCst<'db>>,
    pub guard: Option<Ptr<ExprCst<'db>>>,
    pub body: Ptr<ExprCst<'db>>,
    pub span: RelativeSpan,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, AllocStashData)]
pub struct ClosureParamCst<'db> {
    pub pat: Ptr<PatCst<'db>>,
    pub ty: Option<Ptr<TypeCst<'db>>>,
    pub span: RelativeSpan,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, AllocStashData)]
pub struct FieldInitCst<'db> {
    pub name: Name<'db>,
    pub value: Ptr<ExprCst<'db>>,
    pub span: RelativeSpan,
}
