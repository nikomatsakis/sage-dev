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

use crate::check::BodyCheck;
use crate::resolve::Namespace;
use crate::tytree::*;

impl<'db> ExprCst<'db> {
    pub(crate) fn check(&self, check: &mut BodyCheck<'_, 'db>) -> Ptr<TyExpr<'db>> {
        let span = self.span;
        let (kind, ty) = match &self.kind {
            ExprCstKind::Literal(lit) => {
                let ty = check_literal_ty(check, *lit);
                (TyExprData::Literal(*lit), ty)
            }
            ExprCstKind::Path(path_ptr) => {
                let path = check.src[*path_ptr];
                let res = check.resolve_path(path, Namespace::Value);
                let ty = res_to_ty(check, res);
                (TyExprData::Path(res), ty)
            }
            ExprCstKind::Block(stmts, tail) => {
                check.resolver.ribs.push_scope();
                let rstmts: Vec<_> = check.src[*stmts].iter().map(|s| s.check(check)).collect();
                let stmts_slice = check.stash_mut().alloc_slice(&rstmts);
                let (tail_ptr, ty) = match tail {
                    Some(t) => {
                        let te = check.src[*t].check(check);
                        let ty = check.stash()[te].ty;
                        (Some(te), ty)
                    }
                    None => (None, check.unit_ty()),
                };
                check.resolver.ribs.pop_scope();
                (TyExprData::Block(stmts_slice, tail_ptr), ty)
            }
            ExprCstKind::Call(func, args) => {
                let rf = check.src[*func].check(check);
                let rargs: Vec<_> = check.src[*args]
                    .iter()
                    .map(|a| a.check_val(check))
                    .collect();
                let args_slice = check.stash_mut().alloc_slice(&rargs);
                let ty = check.fresh_ty_var(); // TODO: look up fn return type
                (TyExprData::Call(rf, args_slice), ty)
            }
            ExprCstKind::MethodCall(obj, name, args) => {
                let ro = check.src[*obj].check(check);
                let rargs: Vec<_> = check.src[*args]
                    .iter()
                    .map(|a| a.check_val(check))
                    .collect();
                let args_slice = check.stash_mut().alloc_slice(&rargs);
                let ty = check.fresh_ty_var(); // TODO: method resolution
                (TyExprData::MethodCall(ro, *name, args_slice), ty)
            }
            ExprCstKind::Field(obj, name) => {
                let ro = check.src[*obj].check(check);
                let ty = check.fresh_ty_var(); // TODO: field type lookup
                (TyExprData::Field(ro, *name), ty)
            }
            ExprCstKind::Binary(lhs, op, rhs) => {
                let rl = check.src[*lhs].check(check);
                let rr = check.src[*rhs].check(check);
                let lhs_ty = check.stash()[rl].ty;
                let rhs_ty = check.stash()[rr].ty;
                let ty = check_binary_op_ty(check, *op, lhs_ty, rhs_ty);
                (TyExprData::Binary(rl, *op, rr), ty)
            }
            ExprCstKind::Unary(op, operand) => {
                let ro = check.src[*operand].check(check);
                let ty = check.stash()[ro].ty;
                (TyExprData::Unary(*op, ro), ty)
            }
            ExprCstKind::Ref(inner, m) => {
                let ri = check.src[*inner].check(check);
                let inner_ty = check.stash()[ri].ty;
                let ty = check.alloc_ty(Ty::Ref(inner_ty, *m, crate::ty::Lifetime::Erased));
                (TyExprData::Ref(ri, *m), ty)
            }
            ExprCstKind::If(cond, then, else_) => {
                let rc = check.src[*cond].check(check);
                let cond_ty = check.stash()[rc].ty;
                let bool_ty = check.alloc_ty(Ty::Bool);
                check.require_eq(cond_ty, bool_ty);

                let result_ty = check.fresh_ty_var();
                let rt = check.src[*then].check(check);
                let then_ty = check.stash()[rt].ty;
                check.require_coerce(then_ty, result_ty);

                let re = match else_ {
                    Some(e) => {
                        let re = check.src[*e].check(check);
                        let else_ty = check.stash()[re].ty;
                        check.require_coerce(else_ty, result_ty);
                        Some(re)
                    }
                    None => {
                        let unit = check.unit_ty();
                        check.require_eq(result_ty, unit);
                        None
                    }
                };
                (TyExprData::If(rc, rt, re), result_ty)
            }
            ExprCstKind::IfLet(pat, scrutinee, then, else_) => {
                let rs = check.src[*scrutinee].check(check);
                check.resolver.ribs.push_scope();
                let rp = check.src[*pat].check(check);
                let rt = check.src[*then].check(check);
                check.resolver.ribs.pop_scope();

                let result_ty = check.fresh_ty_var();
                let then_ty = check.stash()[rt].ty;
                check.require_coerce(then_ty, result_ty);

                let re = match else_ {
                    Some(e) => {
                        let re = check.src[*e].check(check);
                        let else_ty = check.stash()[re].ty;
                        check.require_coerce(else_ty, result_ty);
                        Some(re)
                    }
                    None => {
                        let unit = check.unit_ty();
                        check.require_eq(result_ty, unit);
                        None
                    }
                };
                (TyExprData::IfLet(rp, rs, rt, re), result_ty)
            }
            ExprCstKind::Match(scrutinee, arms) => {
                let rs = check.src[*scrutinee].check(check);
                let result_ty = check.fresh_ty_var();
                let rarms: Vec<_> = check.src[*arms]
                    .iter()
                    .map(|arm| arm.check(check, result_ty))
                    .collect();
                let arms_slice = check.stash_mut().alloc_slice(&rarms);
                (TyExprData::Match(rs, arms_slice), result_ty)
            }
            ExprCstKind::Loop(body) => {
                let rb = check.src[*body].check(check);
                let ty = check.alloc_ty(Ty::Never);
                (TyExprData::Loop(rb), ty)
            }
            ExprCstKind::While(cond, body) => {
                let rc = check.src[*cond].check(check);
                let cond_ty = check.stash()[rc].ty;
                let bool_ty = check.alloc_ty(Ty::Bool);
                check.require_eq(cond_ty, bool_ty);
                let rb = check.src[*body].check(check);
                let ty = check.unit_ty();
                (TyExprData::While(rc, rb), ty)
            }
            ExprCstKind::WhileLet(pat, scrutinee, body) => {
                let rs = check.src[*scrutinee].check(check);
                check.resolver.ribs.push_scope();
                let rp = check.src[*pat].check(check);
                let rb = check.src[*body].check(check);
                check.resolver.ribs.pop_scope();
                let ty = check.unit_ty();
                (TyExprData::WhileLet(rp, rs, rb), ty)
            }
            ExprCstKind::For(pat, iter, body) => {
                let ri = check.src[*iter].check(check);
                check.resolver.ribs.push_scope();
                let rp = check.src[*pat].check(check);
                let rb = check.src[*body].check(check);
                check.resolver.ribs.pop_scope();
                let ty = check.unit_ty();
                (TyExprData::For(rp, ri, rb), ty)
            }
            ExprCstKind::Break(val) => {
                let rv = val.map(|v| check.src[v].check(check));
                let ty = check.alloc_ty(Ty::Never);
                (TyExprData::Break(rv), ty)
            }
            ExprCstKind::Continue => {
                let ty = check.alloc_ty(Ty::Never);
                (TyExprData::Continue, ty)
            }
            ExprCstKind::Return(val) => {
                let rv = val.map(|v| check.src[v].check(check));
                // TODO: require_coerce to function return type
                let ty = check.alloc_ty(Ty::Never);
                (TyExprData::Return(rv), ty)
            }
            ExprCstKind::Assign(lhs, rhs) => {
                let rl = check.src[*lhs].check(check);
                let rr = check.src[*rhs].check(check);
                let lhs_ty = check.stash()[rl].ty;
                let rhs_ty = check.stash()[rr].ty;
                check.require_coerce(rhs_ty, lhs_ty);
                let ty = check.unit_ty();
                (TyExprData::Assign(rl, rr), ty)
            }
            ExprCstKind::Await(inner) => {
                let ri = check.src[*inner].check(check);
                let ty = check.fresh_ty_var(); // TODO: extract Output type
                (TyExprData::Await(ri), ty)
            }
            ExprCstKind::Try(inner) => {
                let ri = check.src[*inner].check(check);
                let ty = check.fresh_ty_var(); // TODO: extract Ok type
                (TyExprData::Try(ri), ty)
            }
            ExprCstKind::Closure(params, body) => {
                check.resolver.ribs.push_scope();
                let rparams: Vec<_> = check.src[*params]
                    .iter()
                    .map(|p| {
                        let rp = check.src[p.pat].check(check);
                        let param_ty = check.stash()[rp].ty;
                        TyClosureParam {
                            pat: rp,
                            ty: param_ty,
                            span: p.span,
                        }
                    })
                    .collect();
                let rb = check.src[*body].check(check);
                check.resolver.ribs.pop_scope();
                let params_slice = check.stash_mut().alloc_slice(&rparams);
                let ty = check.fresh_ty_var(); // TODO: construct fn type
                (TyExprData::Closure(params_slice, rb), ty)
            }
            ExprCstKind::Tuple(elems) => {
                let relems: Vec<_> = check.src[*elems]
                    .iter()
                    .map(|e| e.check_val(check))
                    .collect();
                let elem_tys: Vec<Ptr<Ty<'db>>> =
                    relems.iter().map(|e| check.stash()[*e].ty).collect();
                let elems_slice = check.stash_mut().alloc_slice(&relems);
                let ty_elems = check.stash_mut().alloc_slice(&elem_tys);
                let ty = check.alloc_ty(Ty::Tuple(ty_elems));
                (TyExprData::Tuple(elems_slice), ty)
            }
            ExprCstKind::Array(elems) => {
                let result_ty = check.fresh_ty_var();
                let relems: Vec<_> = check.src[*elems]
                    .iter()
                    .map(|e| {
                        let re = e.check_val(check);
                        let elem_ty = check.stash()[re].ty;
                        check.require_coerce(elem_ty, result_ty);
                        re
                    })
                    .collect();
                let elems_slice = check.stash_mut().alloc_slice(&relems);
                let ty = check.alloc_ty(Ty::Slice(result_ty));
                (TyExprData::Array(elems_slice), ty)
            }
            ExprCstKind::Index(obj, idx) => {
                let ro = check.src[*obj].check(check);
                let ri = check.src[*idx].check(check);
                let ty = check.fresh_ty_var(); // TODO: Index trait resolution
                (TyExprData::Index(ro, ri), ty)
            }
            ExprCstKind::Cast(expr, ty_cst) => {
                let re = check.src[*expr].check(check);
                let target_ty = check.src[*ty_cst].check(check);
                let target = check.stash_mut().alloc(target_ty);
                (TyExprData::Cast(re, target), target)
            }
            ExprCstKind::StructLit(path_ptr, fields) => {
                let path = check.src[*path_ptr];
                let res = check.resolve_path(path, Namespace::Type);
                let rfields: Vec<_> = check.src[*fields]
                    .iter()
                    .map(|fi| TyFieldInit {
                        name: fi.name,
                        value: check.src[fi.value].check(check),
                        span: fi.span,
                    })
                    .collect();
                let fields_slice = check.stash_mut().alloc_slice(&rfields);
                let ty = check.fresh_ty_var(); // TODO: struct type from resolution
                (TyExprData::StructLit(res, fields_slice), ty)
            }
            ExprCstKind::Range(lo, hi) => {
                let rl = lo.map(|l| check.src[l].check(check));
                let rh = hi.map(|h| check.src[h].check(check));
                let ty = check.fresh_ty_var(); // TODO: Range type
                (TyExprData::Range(rl, rh), ty)
            }
            ExprCstKind::Missing => {
                let ty = check.alloc_ty(Ty::Error);
                (TyExprData::Missing, ty)
            }
        };
        check.alloc_expr(kind, ty, span)
    }

    pub(crate) fn check_val(&self, cx: &mut BodyCheck<'_, 'db>) -> Ptr<TyExpr<'db>> {
        self.check(cx)
    }
}

impl<'db> StmtCst<'db> {
    pub(crate) fn check(&self, cx: &mut BodyCheck<'_, 'db>) -> TyStmt<'db> {
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
                    let resolved = cx.src[t].check(cx);
                    let ptr = cx.stash_mut().alloc(resolved);
                    ptr
                });

                TyStmtKind::Let(rpat, ty_ann_ptr, rinit)
            }
            StmtCstKind::Expr(e) => TyStmtKind::Expr(cx.src[*e].check(cx)),
        };
        TyStmt { kind, span }
    }
}

impl<'db> PatCst<'db> {
    pub(crate) fn check(&self, cx: &mut BodyCheck<'_, 'db>) -> Ptr<TyPat<'db>> {
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
                let rpats: Vec<_> = cx.src[*pats].iter().map(|p| p.check(cx)).collect();
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
                let rpats: Vec<_> = cx.src[*pats].iter().map(|p| p.check(cx)).collect();
                let pats_slice = cx.stash_mut().alloc_slice(&rpats);
                TyPatKind::TupleStruct(res, pats_slice)
            }
            PatCstKind::Ref(inner, m) => {
                let ri = cx.src[*inner].check(cx);
                TyPatKind::Ref(ri, *m)
            }
            PatCstKind::Literal(lit) => TyPatKind::Literal(*lit),
            PatCstKind::Or(pats) => {
                let rpats: Vec<_> = cx.src[*pats].iter().map(|p| p.check(cx)).collect();
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
    pub(crate) fn check(
        &self,
        cx: &mut BodyCheck<'_, 'db>,
        result_ty: Ptr<Ty<'db>>,
    ) -> TyMatchArm<'db> {
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

use crate::ty::Ty;

fn check_literal_ty<'db>(cx: &mut BodyCheck<'_, 'db>, lit: Literal) -> Ptr<Ty<'db>> {
    match lit {
        Literal::Bool(_) => cx.alloc_ty(Ty::Bool),
        Literal::Int => cx.fresh_ty_var(),
        Literal::Float => cx.fresh_ty_var(),
        Literal::String => {
            let str_ty = cx.alloc_ty(Ty::Str);
            cx.alloc_ty(Ty::Ref(
                str_ty,
                crate::types::Mutability::Shared,
                crate::ty::Lifetime::Static,
            ))
        }
        Literal::Char => cx.alloc_ty(Ty::Char),
    }
}

fn res_to_ty<'db>(cx: &mut BodyCheck<'_, 'db>, res: Res<'db>) -> Ptr<Ty<'db>> {
    match res {
        Res::Local(LocalId(id)) => cx.local_type(id),
        Res::Def(_) => cx.fresh_ty_var(), // TODO: look up symbol's type
        Res::Err => cx.alloc_ty(Ty::Error),
    }
}

fn check_binary_op_ty<'db>(
    cx: &mut BodyCheck<'_, 'db>,
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
        | BinaryOp::Or => cx.alloc_ty(Ty::Bool),

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
