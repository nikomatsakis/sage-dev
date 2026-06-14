use sage_stash::{AllocStashData, Ptr, Slice, StashDirect};

use crate::cst::paths::PathCst;
use crate::cst::ty::TypeCst;
use crate::name::Name;
use crate::span::RelativeSpan;
use crate::types::Mutability;

// ---------------------------------------------------------------------------
// Expression primitives (shared with tytree)
// ---------------------------------------------------------------------------

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub enum Literal {
    Int,
    Float,
    String,
    Bool(bool),
    Char,
}

impl StashDirect for Literal {}

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
    Literal(Literal),
    Path(Ptr<PathCst<'db>>),
    Block(Slice<StmtCst<'db>>, Option<Ptr<ExprCst<'db>>>),
    Call(Ptr<ExprCst<'db>>, Slice<ExprCst<'db>>),
    MethodCall(Ptr<ExprCst<'db>>, Name<'db>, Slice<ExprCst<'db>>),
    Field(Ptr<ExprCst<'db>>, Name<'db>),
    Binary(Ptr<ExprCst<'db>>, BinaryOp, Ptr<ExprCst<'db>>),
    Unary(UnaryOp, Ptr<ExprCst<'db>>),
    Ref(Ptr<ExprCst<'db>>, Mutability),
    If(Ptr<ExprCst<'db>>, Ptr<ExprCst<'db>>, Option<Ptr<ExprCst<'db>>>),
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
    StructLit(Ptr<PathCst<'db>>, Slice<FieldInitCst<'db>>),
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
    Path(Ptr<PathCst<'db>>),
    Tuple(Slice<PatCst<'db>>),
    Struct(Ptr<PathCst<'db>>, Slice<FieldPatCst<'db>>),
    TupleStruct(Ptr<PathCst<'db>>, Slice<PatCst<'db>>),
    Ref(Ptr<PatCst<'db>>, Mutability),
    Literal(Literal),
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

// ---------------------------------------------------------------------------
// Body checking: ExprCst/StmtCst/PatCst → TyExpr/TyStmt/TyPat
// ---------------------------------------------------------------------------

use crate::check::BodyCtx;
use crate::resolve::Namespace;
use crate::tytree::*;

impl<'db> ExprCst<'db> {
    pub(crate) fn check(&self, cx: &mut BodyCtx<'_, 'db>) -> Ptr<TyExpr<'db>> {
        let span = self.span;
        let (kind, ty) = match &self.kind {
            ExprCstKind::Literal(lit) => {
                let ty = check_literal_ty(cx, *lit);
                (TyExprKind::Literal(*lit), ty)
            }
            ExprCstKind::Path(path_ptr) => {
                let path = cx.src[*path_ptr];
                let res = cx.resolve_path(path, Namespace::Value);
                let ty = res_to_ty(cx, res);
                (TyExprKind::Path(res), ty)
            }
            ExprCstKind::Block(stmts, tail) => {
                cx.resolver.ribs.push_scope();
                let rstmts: Vec<_> = cx.src[*stmts]
                    .iter()
                    .map(|s| s.check(cx))
                    .collect();
                let stmts_slice = cx.stash_mut().alloc_slice(&rstmts);
                let (tail_ptr, ty) = match tail {
                    Some(t) => {
                        let te = cx.src[*t].check(cx);
                        let ty = cx.stash()[te].ty;
                        (Some(te), ty)
                    }
                    None => (None, cx.unit_ty()),
                };
                cx.resolver.ribs.pop_scope();
                (TyExprKind::Block(stmts_slice, tail_ptr), ty)
            }
            ExprCstKind::Call(func, args) => {
                let rf = cx.src[*func].check(cx);
                let rargs: Vec<_> = cx.src[*args]
                    .iter()
                    .map(|a| a.check_val(cx))
                    .collect();
                let args_slice = cx.stash_mut().alloc_slice(&rargs);
                let ty = cx.fresh_ty_var(); // TODO: look up fn return type
                (TyExprKind::Call(rf, args_slice), ty)
            }
            ExprCstKind::MethodCall(obj, name, args) => {
                let ro = cx.src[*obj].check(cx);
                let rargs: Vec<_> = cx.src[*args]
                    .iter()
                    .map(|a| a.check_val(cx))
                    .collect();
                let args_slice = cx.stash_mut().alloc_slice(&rargs);
                let ty = cx.fresh_ty_var(); // TODO: method resolution
                (TyExprKind::MethodCall(ro, *name, args_slice), ty)
            }
            ExprCstKind::Field(obj, name) => {
                let ro = cx.src[*obj].check(cx);
                let ty = cx.fresh_ty_var(); // TODO: field type lookup
                (TyExprKind::Field(ro, *name), ty)
            }
            ExprCstKind::Binary(lhs, op, rhs) => {
                let rl = cx.src[*lhs].check(cx);
                let rr = cx.src[*rhs].check(cx);
                let lhs_ty = cx.stash()[rl].ty;
                let rhs_ty = cx.stash()[rr].ty;
                let ty = check_binary_op_ty(cx, *op, lhs_ty, rhs_ty);
                (TyExprKind::Binary(rl, *op, rr), ty)
            }
            ExprCstKind::Unary(op, operand) => {
                let ro = cx.src[*operand].check(cx);
                let ty = cx.stash()[ro].ty;
                (TyExprKind::Unary(*op, ro), ty)
            }
            ExprCstKind::Ref(inner, m) => {
                let ri = cx.src[*inner].check(cx);
                let inner_ty = cx.stash()[ri].ty;
                let ty = cx.alloc_ty(TyData::Ref(inner_ty, *m, crate::ty::Lifetime::Erased));
                (TyExprKind::Ref(ri, *m), ty)
            }
            ExprCstKind::If(cond, then, else_) => {
                let rc = cx.src[*cond].check(cx);
                let cond_ty = cx.stash()[rc].ty;
                let bool_ty = cx.alloc_ty(TyData::Bool);
                cx.require_eq(cond_ty, bool_ty);

                let result_ty = cx.fresh_ty_var();
                let rt = cx.src[*then].check(cx);
                let then_ty = cx.stash()[rt].ty;
                cx.require_coerce(then_ty, result_ty);

                let re = match else_ {
                    Some(e) => {
                        let re = cx.src[*e].check(cx);
                        let else_ty = cx.stash()[re].ty;
                        cx.require_coerce(else_ty, result_ty);
                        Some(re)
                    }
                    None => {
                        let unit = cx.unit_ty();
                        cx.require_eq(result_ty, unit);
                        None
                    }
                };
                (TyExprKind::If(rc, rt, re), result_ty)
            }
            ExprCstKind::IfLet(pat, scrutinee, then, else_) => {
                let rs = cx.src[*scrutinee].check(cx);
                cx.resolver.ribs.push_scope();
                let rp = cx.src[*pat].check(cx);
                let rt = cx.src[*then].check(cx);
                cx.resolver.ribs.pop_scope();

                let result_ty = cx.fresh_ty_var();
                let then_ty = cx.stash()[rt].ty;
                cx.require_coerce(then_ty, result_ty);

                let re = match else_ {
                    Some(e) => {
                        let re = cx.src[*e].check(cx);
                        let else_ty = cx.stash()[re].ty;
                        cx.require_coerce(else_ty, result_ty);
                        Some(re)
                    }
                    None => {
                        let unit = cx.unit_ty();
                        cx.require_eq(result_ty, unit);
                        None
                    }
                };
                (TyExprKind::IfLet(rp, rs, rt, re), result_ty)
            }
            ExprCstKind::Match(scrutinee, arms) => {
                let rs = cx.src[*scrutinee].check(cx);
                let result_ty = cx.fresh_ty_var();
                let rarms: Vec<_> = cx.src[*arms]
                    .iter()
                    .map(|arm| arm.check(cx, result_ty))
                    .collect();
                let arms_slice = cx.stash_mut().alloc_slice(&rarms);
                (TyExprKind::Match(rs, arms_slice), result_ty)
            }
            ExprCstKind::Loop(body) => {
                let rb = cx.src[*body].check(cx);
                let ty = cx.alloc_ty(TyData::Never);
                (TyExprKind::Loop(rb), ty)
            }
            ExprCstKind::While(cond, body) => {
                let rc = cx.src[*cond].check(cx);
                let cond_ty = cx.stash()[rc].ty;
                let bool_ty = cx.alloc_ty(TyData::Bool);
                cx.require_eq(cond_ty, bool_ty);
                let rb = cx.src[*body].check(cx);
                let ty = cx.unit_ty();
                (TyExprKind::While(rc, rb), ty)
            }
            ExprCstKind::WhileLet(pat, scrutinee, body) => {
                let rs = cx.src[*scrutinee].check(cx);
                cx.resolver.ribs.push_scope();
                let rp = cx.src[*pat].check(cx);
                let rb = cx.src[*body].check(cx);
                cx.resolver.ribs.pop_scope();
                let ty = cx.unit_ty();
                (TyExprKind::WhileLet(rp, rs, rb), ty)
            }
            ExprCstKind::For(pat, iter, body) => {
                let ri = cx.src[*iter].check(cx);
                cx.resolver.ribs.push_scope();
                let rp = cx.src[*pat].check(cx);
                let rb = cx.src[*body].check(cx);
                cx.resolver.ribs.pop_scope();
                let ty = cx.unit_ty();
                (TyExprKind::For(rp, ri, rb), ty)
            }
            ExprCstKind::Break(val) => {
                let rv = val.map(|v| cx.src[v].check(cx));
                let ty = cx.alloc_ty(TyData::Never);
                (TyExprKind::Break(rv), ty)
            }
            ExprCstKind::Continue => {
                let ty = cx.alloc_ty(TyData::Never);
                (TyExprKind::Continue, ty)
            }
            ExprCstKind::Return(val) => {
                let rv = val.map(|v| cx.src[v].check(cx));
                // TODO: require_coerce to function return type
                let ty = cx.alloc_ty(TyData::Never);
                (TyExprKind::Return(rv), ty)
            }
            ExprCstKind::Assign(lhs, rhs) => {
                let rl = cx.src[*lhs].check(cx);
                let rr = cx.src[*rhs].check(cx);
                let lhs_ty = cx.stash()[rl].ty;
                let rhs_ty = cx.stash()[rr].ty;
                cx.require_coerce(rhs_ty, lhs_ty);
                let ty = cx.unit_ty();
                (TyExprKind::Assign(rl, rr), ty)
            }
            ExprCstKind::Await(inner) => {
                let ri = cx.src[*inner].check(cx);
                let ty = cx.fresh_ty_var(); // TODO: extract Output type
                (TyExprKind::Await(ri), ty)
            }
            ExprCstKind::Try(inner) => {
                let ri = cx.src[*inner].check(cx);
                let ty = cx.fresh_ty_var(); // TODO: extract Ok type
                (TyExprKind::Try(ri), ty)
            }
            ExprCstKind::Closure(params, body) => {
                cx.resolver.ribs.push_scope();
                let rparams: Vec<_> = cx.src[*params]
                    .iter()
                    .map(|p| {
                        let rp = cx.src[p.pat].check(cx);
                        let param_ty = cx.stash()[rp].ty;
                        TyClosureParam {
                            pat: rp,
                            ty: param_ty,
                            span: p.span,
                        }
                    })
                    .collect();
                let rb = cx.src[*body].check(cx);
                cx.resolver.ribs.pop_scope();
                let params_slice = cx.stash_mut().alloc_slice(&rparams);
                let ty = cx.fresh_ty_var(); // TODO: construct fn type
                (TyExprKind::Closure(params_slice, rb), ty)
            }
            ExprCstKind::Tuple(elems) => {
                let relems: Vec<_> = cx.src[*elems]
                    .iter()
                    .map(|e| e.check_val(cx))
                    .collect();
                let elem_tys: Vec<Ptr<Ty<'db>>> = relems
                    .iter()
                    .map(|e| cx.stash()[*e].ty)
                    .collect();
                let elems_slice = cx.stash_mut().alloc_slice(&relems);
                let ty_elems = cx.stash_mut().alloc_slice(&elem_tys);
                let ty = cx.alloc_ty(TyData::Tuple(ty_elems));
                (TyExprKind::Tuple(elems_slice), ty)
            }
            ExprCstKind::Array(elems) => {
                let result_ty = cx.fresh_ty_var();
                let relems: Vec<_> = cx.src[*elems]
                    .iter()
                    .map(|e| {
                        let re = e.check_val(cx);
                        let elem_ty = cx.stash()[re].ty;
                        cx.require_coerce(elem_ty, result_ty);
                        re
                    })
                    .collect();
                let elems_slice = cx.stash_mut().alloc_slice(&relems);
                let ty = cx.alloc_ty(TyData::Slice(result_ty));
                (TyExprKind::Array(elems_slice), ty)
            }
            ExprCstKind::Index(obj, idx) => {
                let ro = cx.src[*obj].check(cx);
                let ri = cx.src[*idx].check(cx);
                let ty = cx.fresh_ty_var(); // TODO: Index trait resolution
                (TyExprKind::Index(ro, ri), ty)
            }
            ExprCstKind::Cast(expr, ty_cst) => {
                let re = cx.src[*expr].check(cx);
                let target_ty = cx.src[*ty_cst].check_in_body(cx);
                let target = cx.stash_mut().alloc(target_ty);
                (TyExprKind::Cast(re, target), target)
            }
            ExprCstKind::StructLit(path_ptr, fields) => {
                let path = cx.src[*path_ptr];
                let res = cx.resolve_path(path, Namespace::Type);
                let rfields: Vec<_> = cx.src[*fields]
                    .iter()
                    .map(|fi| TyFieldInit {
                        name: fi.name,
                        value: cx.src[fi.value].check(cx),
                        span: fi.span,
                    })
                    .collect();
                let fields_slice = cx.stash_mut().alloc_slice(&rfields);
                let ty = cx.fresh_ty_var(); // TODO: struct type from resolution
                (TyExprKind::StructLit(res, fields_slice), ty)
            }
            ExprCstKind::Range(lo, hi) => {
                let rl = lo.map(|l| cx.src[l].check(cx));
                let rh = hi.map(|h| cx.src[h].check(cx));
                let ty = cx.fresh_ty_var(); // TODO: Range type
                (TyExprKind::Range(rl, rh), ty)
            }
            ExprCstKind::Missing => {
                let ty = cx.alloc_ty(TyData::Error);
                (TyExprKind::Missing, ty)
            }
        };
        cx.alloc_expr(kind, ty, span)
    }

    pub(crate) fn check_val(&self, cx: &mut BodyCtx<'_, 'db>) -> Ptr<TyExpr<'db>> {
        self.check(cx)
    }
}

impl<'db> StmtCst<'db> {
    pub(crate) fn check(&self, cx: &mut BodyCtx<'_, 'db>) -> TyStmt<'db> {
        let span = self.span;
        let kind = match &self.kind {
            StmtCstKind::Let(pat, ty_ann, init) => {
                let rinit = init.map(|e| cx.src[e].check(cx));
                let rpat = cx.src[*pat].check(cx);
                let pat_ty = cx.stash()[rpat].ty;

                if let Some(init_ptr) = rinit {
                    let init_ty = cx.stash()[init_ptr].ty;
                    cx.require_coerce(init_ty, pat_ty);
                }

                let ty_ann_ptr = ty_ann.map(|t| {
                    let resolved = cx.src[t].check_in_body(cx);
                    let ptr = cx.stash_mut().alloc(resolved);
                    ptr
                });

                TyStmtKind::Let(rpat, ty_ann_ptr, rinit)
            }
            StmtCstKind::Expr(e) => {
                TyStmtKind::Expr(cx.src[*e].check(cx))
            }
        };
        TyStmt { kind, span }
    }
}

impl<'db> PatCst<'db> {
    pub(crate) fn check(&self, cx: &mut BodyCtx<'_, 'db>) -> Ptr<TyPat<'db>> {
        let span = self.span;
        let ty = cx.fresh_ty_var();
        let kind = match &self.kind {
            PatCstKind::Wildcard => TyPatKind::Wildcard,
            PatCstKind::Bind(name, mutability) => {
                let id = cx.add_binding(*name, span);
                TyPatKind::Bind(id, *mutability)
            }
            PatCstKind::Path(path_ptr) => {
                let path = cx.src[*path_ptr];
                let res = cx.resolve_path(path, Namespace::Value);
                TyPatKind::Path(res)
            }
            PatCstKind::Tuple(pats) => {
                let rpats: Vec<_> = cx.src[*pats]
                    .iter()
                    .map(|p| p.check(cx))
                    .collect();
                let pats_slice = cx.stash_mut().alloc_slice(&rpats);
                TyPatKind::Tuple(pats_slice)
            }
            PatCstKind::Struct(path_ptr, fields) => {
                let path = cx.src[*path_ptr];
                let res = cx.resolve_path(path, Namespace::Type);
                let rfields: Vec<_> = cx.src[*fields]
                    .iter()
                    .map(|fp| TyFieldPat {
                        name: fp.name,
                        pat: cx.src[fp.pat].check(cx),
                        span: fp.span,
                    })
                    .collect();
                let fields_slice = cx.stash_mut().alloc_slice(&rfields);
                TyPatKind::Struct(res, fields_slice)
            }
            PatCstKind::TupleStruct(path_ptr, pats) => {
                let path = cx.src[*path_ptr];
                let res = cx.resolve_path(path, Namespace::Value);
                let rpats: Vec<_> = cx.src[*pats]
                    .iter()
                    .map(|p| p.check(cx))
                    .collect();
                let pats_slice = cx.stash_mut().alloc_slice(&rpats);
                TyPatKind::TupleStruct(res, pats_slice)
            }
            PatCstKind::Ref(inner, m) => {
                let ri = cx.src[*inner].check(cx);
                TyPatKind::Ref(ri, *m)
            }
            PatCstKind::Literal(lit) => TyPatKind::Literal(*lit),
            PatCstKind::Or(pats) => {
                let rpats: Vec<_> = cx.src[*pats]
                    .iter()
                    .map(|p| p.check(cx))
                    .collect();
                let pats_slice = cx.stash_mut().alloc_slice(&rpats);
                TyPatKind::Or(pats_slice)
            }
            PatCstKind::Rest => TyPatKind::Rest,
            PatCstKind::Missing => TyPatKind::Missing,
        };
        cx.stash_mut().alloc(TyPat { kind, ty, span })
    }
}

impl<'db> MatchArmCst<'db> {
    pub(crate) fn check(&self, cx: &mut BodyCtx<'_, 'db>, result_ty: Ptr<Ty<'db>>) -> TyMatchArm<'db> {
        cx.resolver.ribs.push_scope();
        let rp = cx.src[self.pat].check(cx);
        let rg = self.guard.map(|g| cx.src[g].check(cx));
        let rb = cx.src[self.body].check(cx);
        let body_ty = cx.stash()[rb].ty;
        cx.require_coerce(body_ty, result_ty);
        cx.resolver.ribs.pop_scope();
        TyMatchArm {
            pat: rp,
            guard: rg,
            body: rb,
            span: self.span,
        }
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

use crate::ty::{Ty, TyData};

fn check_literal_ty<'db>(cx: &mut BodyCtx<'_, 'db>, lit: Literal) -> Ptr<Ty<'db>> {
    match lit {
        Literal::Bool(_) => cx.alloc_ty(TyData::Bool),
        Literal::Int => cx.fresh_ty_var(),
        Literal::Float => cx.fresh_ty_var(),
        Literal::String => {
            let str_ty = cx.alloc_ty(TyData::Str);
            cx.alloc_ty(TyData::Ref(
                str_ty,
                crate::types::Mutability::Shared,
                crate::ty::Lifetime::Static,
            ))
        }
        Literal::Char => cx.alloc_ty(TyData::Char),
    }
}

fn res_to_ty<'db>(cx: &mut BodyCtx<'_, 'db>, res: Res<'db>) -> Ptr<Ty<'db>> {
    match res {
        Res::Local(LocalId(id)) => cx.local_type(id),
        Res::Def(_) => cx.fresh_ty_var(), // TODO: look up symbol's type
        Res::Err => cx.alloc_ty(TyData::Error),
    }
}

fn check_binary_op_ty<'db>(
    cx: &mut BodyCtx<'_, 'db>,
    op: BinaryOp,
    lhs_ty: Ptr<Ty<'db>>,
    rhs_ty: Ptr<Ty<'db>>,
) -> Ptr<Ty<'db>> {
    cx.require_eq(rhs_ty, lhs_ty);
    match op {
        BinaryOp::Eq
        | BinaryOp::Ne
        | BinaryOp::Lt
        | BinaryOp::Le
        | BinaryOp::Gt
        | BinaryOp::Ge
        | BinaryOp::And
        | BinaryOp::Or => cx.alloc_ty(TyData::Bool),

        BinaryOp::Add
        | BinaryOp::Sub
        | BinaryOp::Mul
        | BinaryOp::Div
        | BinaryOp::Rem
        | BinaryOp::BitAnd
        | BinaryOp::BitOr
        | BinaryOp::BitXor
        | BinaryOp::Shl
        | BinaryOp::Shr => lhs_ty,
    }
}
