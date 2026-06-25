use sage_stash::{AllocStashData, Ptr, Slice, Stashed};

use crate::cst::Mutability;
use crate::cst::expr::{BinaryOp, Literal, UnaryOp};
use crate::diagnostic::{Diagnostic, ErrorReported};
use crate::name::Name;
use crate::span::RelativeSpan;
use crate::symbol::Symbol;
use crate::ty::Ty;
use crate::types::TokenTree;

// ---------------------------------------------------------------------------
// Resolved-body primitives (shared with check/, ribs, cst/paths)
// ---------------------------------------------------------------------------

/// What a path resolved to.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, AllocStashData)]
pub enum PathResolution<'db> {
    Def(Symbol<'db>),
    Local(LocalId),
    Err(ErrorReported),
}

/// Short alias.
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

// ---------------------------------------------------------------------------
// Typed tree
// ---------------------------------------------------------------------------

pub type TyBody<'db> = Stashed<Ptr<TyBodyData<'db>>>;

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct CheckedBody<'db> {
    pub body: TyBody<'db>,
    pub diagnostics: Vec<Diagnostic<'db>>,
}

unsafe impl salsa::Update for CheckedBody<'_> {
    unsafe fn maybe_update(old_pointer: *mut Self, new_value: Self) -> bool {
        let old = unsafe { &*old_pointer };
        if *old == new_value {
            false
        } else {
            unsafe { *old_pointer = new_value };
            true
        }
    }
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, AllocStashData)]
pub struct TyBodyData<'db> {
    pub root: Ptr<TyExpr<'db>>,
    pub locals: Slice<LocalVar<'db>>,
    pub span: RelativeSpan,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, AllocStashData)]
pub struct TyExpr<'db> {
    pub data: TyExprData<'db>,
    pub ty: Ptr<Ty<'db>>,
    pub span: RelativeSpan,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, AllocStashData)]
pub enum TyExprData<'db> {
    Literal(Literal<'db>),
    Path(Res<'db>),
    Block(Slice<TyStmt<'db>>, Option<Ptr<TyExpr<'db>>>),
    Call(Ptr<TyExpr<'db>>, Slice<Ptr<TyExpr<'db>>>),
    MethodCall(Ptr<TyExpr<'db>>, Name<'db>, Slice<Ptr<TyExpr<'db>>>),
    Field(Ptr<TyExpr<'db>>, Name<'db>),
    Binary(Ptr<TyExpr<'db>>, BinaryOp, Ptr<TyExpr<'db>>),
    Unary(UnaryOp, Ptr<TyExpr<'db>>),
    Ref(Ptr<TyExpr<'db>>, Mutability),
    If(Ptr<TyExpr<'db>>, Ptr<TyExpr<'db>>, Option<Ptr<TyExpr<'db>>>),
    IfLet(
        Ptr<TyPat<'db>>,
        Ptr<TyExpr<'db>>,
        Ptr<TyExpr<'db>>,
        Option<Ptr<TyExpr<'db>>>,
    ),
    Match(Ptr<TyExpr<'db>>, Slice<TyMatchArm<'db>>),
    Loop(Ptr<TyExpr<'db>>),
    While(Ptr<TyExpr<'db>>, Ptr<TyExpr<'db>>),
    WhileLet(Ptr<TyPat<'db>>, Ptr<TyExpr<'db>>, Ptr<TyExpr<'db>>),
    For(Ptr<TyPat<'db>>, Ptr<TyExpr<'db>>, Ptr<TyExpr<'db>>),
    Break(Option<Ptr<TyExpr<'db>>>),
    Continue,
    Return(Option<Ptr<TyExpr<'db>>>),
    Assign(Ptr<TyExpr<'db>>, Ptr<TyExpr<'db>>),
    Await(Ptr<TyExpr<'db>>),
    Try(Ptr<TyExpr<'db>>),
    Closure(Slice<TyClosureParam<'db>>, Ptr<TyExpr<'db>>),
    Tuple(Slice<Ptr<TyExpr<'db>>>),
    Array(Slice<Ptr<TyExpr<'db>>>),
    Index(Ptr<TyExpr<'db>>, Ptr<TyExpr<'db>>),
    Cast(Ptr<TyExpr<'db>>, Ptr<Ty<'db>>),
    StructLit(Res<'db>, Slice<TyFieldInit<'db>>),
    Range(Option<Ptr<TyExpr<'db>>>, Option<Ptr<TyExpr<'db>>>),
    MacroCall(Res<'db>, TokenTree<'db>),
    Missing,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, AllocStashData)]
pub struct TyStmt<'db> {
    pub kind: TyStmtKind<'db>,
    pub span: RelativeSpan,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, AllocStashData)]
pub enum TyStmtKind<'db> {
    Let(
        Ptr<TyPat<'db>>,
        Option<Ptr<Ty<'db>>>,
        Option<Ptr<TyExpr<'db>>>,
    ),
    Expr(Ptr<TyExpr<'db>>),
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, AllocStashData)]
pub struct TyPat<'db> {
    pub kind: TyPatKind<'db>,
    pub ty: Ptr<Ty<'db>>,
    pub span: RelativeSpan,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, AllocStashData)]
pub enum TyPatKind<'db> {
    Wildcard,
    Bind(LocalId, Mutability),
    Path(Res<'db>),
    Tuple(Slice<Ptr<TyPat<'db>>>),
    Struct(Res<'db>, Slice<TyFieldPat<'db>>),
    TupleStruct(Res<'db>, Slice<Ptr<TyPat<'db>>>),
    Ref(Ptr<TyPat<'db>>, Mutability),
    Literal(Literal<'db>),
    Or(Slice<Ptr<TyPat<'db>>>),
    Rest,
    Missing,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, AllocStashData)]
pub struct TyFieldInit<'db> {
    pub name: Name<'db>,
    pub value: Ptr<TyExpr<'db>>,
    pub span: RelativeSpan,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, AllocStashData)]
pub struct TyFieldPat<'db> {
    pub name: Name<'db>,
    pub pat: Ptr<TyPat<'db>>,
    pub span: RelativeSpan,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, AllocStashData)]
pub struct TyMatchArm<'db> {
    pub pat: Ptr<TyPat<'db>>,
    pub guard: Option<Ptr<TyExpr<'db>>>,
    pub body: Ptr<TyExpr<'db>>,
    pub span: RelativeSpan,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, AllocStashData)]
pub struct TyClosureParam<'db> {
    pub pat: Ptr<TyPat<'db>>,
    pub ty: Ptr<Ty<'db>>,
    pub span: RelativeSpan,
}
