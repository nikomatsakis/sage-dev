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
                let path = check.source_stash[*path_ptr];
                let res = check.resolve_path(path, Namespace::Value, span);
                let ty = res_to_ty(check, res);
                (TyExprData::Path(res), ty)
            }
            ExprCstKind::Block(stmts, tail) => {
                check.resolver.ribs.push_scope();
                let rstmts: Vec<_> = check.source_stash[*stmts]
                    .iter()
                    .map(|s| s.check(check))
                    .collect();
                let stmts_slice = check.stash_mut().alloc_slice(&rstmts);
                let (tail_ptr, ty) = match tail {
                    Some(t) => {
                        let te = check.source_stash[*t].check(check);
                        let ty = check.stash()[te].ty;
                        (Some(te), ty)
                    }
                    None => (None, check.unit_ty()),
                };
                check.resolver.ribs.pop_scope();
                (TyExprData::Block(stmts_slice, tail_ptr), ty)
            }
            ExprCstKind::Call(func, args) => {
                let rf = check.source_stash[*func].check(check);
                let rargs: Vec<_> = check.source_stash[*args]
                    .iter()
                    .map(|a| a.check_val(check))
                    .collect();
                let args_slice = check.stash_mut().alloc_slice(&rargs);
                let ty = check_call_ty(check, rf, args_slice, span);
                (TyExprData::Call(rf, args_slice), ty)
            }
            ExprCstKind::MethodCall(obj, name, args) => {
                let ro = check.source_stash[*obj].check(check);
                let rargs: Vec<_> = check.source_stash[*args]
                    .iter()
                    .map(|a| a.check_val(check))
                    .collect();
                let args_slice = check.stash_mut().alloc_slice(&rargs);
                let ty = check.fresh_ty_var(); // TODO: method resolution
                (TyExprData::MethodCall(ro, *name, args_slice), ty)
            }
            ExprCstKind::Field(obj, name) => {
                let ro = check.source_stash[*obj].check(check);
                let ty = lookup_field_ty(check, ro, *name);
                (TyExprData::Field(ro, *name), ty)
            }
            ExprCstKind::Binary(lhs, op, rhs) => {
                let rl = check.source_stash[*lhs].check(check);
                let rr = check.source_stash[*rhs].check(check);
                let lhs_ty = check.stash()[rl].ty;
                let rhs_ty = check.stash()[rr].ty;
                let ty = check_binary_op_ty(check, *op, lhs_ty, rhs_ty, span);
                (TyExprData::Binary(rl, *op, rr), ty)
            }
            ExprCstKind::Unary(op, operand) => {
                let ro = check.source_stash[*operand].check(check);
                let ty = check.stash()[ro].ty;
                (TyExprData::Unary(*op, ro), ty)
            }
            ExprCstKind::Ref(inner, m) => {
                let ri = check.source_stash[*inner].check(check);
                let inner_ty = check.stash()[ri].ty;
                let ty = check.alloc_ty(Ty::Ref(inner_ty, *m, crate::ty::Lifetime::Erased));
                (TyExprData::Ref(ri, *m), ty)
            }
            ExprCstKind::If(cond, then, else_) => {
                let rc = check.source_stash[*cond].check(check);
                let cond_ty = check.stash()[rc].ty;
                let bool_ty = check.alloc_ty(Ty::Bool);
                let cond_span = check.source_stash[*cond].span;
                if let Err(e) = check.require_eq(cond_ty, bool_ty, cond_span) {
                    check.catch(e);
                }

                let result_ty = check.fresh_ty_var();
                let rt = check.source_stash[*then].check(check);
                let then_ty = check.stash()[rt].ty;
                let then_span = check.source_stash[*then].span;
                if let Err(e) = check.require_coerce(then_ty, result_ty, then_span) {
                    check.catch(e);
                }

                let re = match else_ {
                    Some(e) => {
                        let re = check.source_stash[*e].check(check);
                        let else_ty = check.stash()[re].ty;
                        let else_span = check.source_stash[*e].span;
                        if let Err(err) = check.require_coerce(else_ty, result_ty, else_span) {
                            check.catch(err);
                        }
                        Some(re)
                    }
                    None => {
                        let unit = check.unit_ty();
                        if let Err(e) = check.require_eq(result_ty, unit, span) {
                            check.catch(e);
                        }
                        None
                    }
                };
                (TyExprData::If(rc, rt, re), result_ty)
            }
            ExprCstKind::IfLet(pat, scrutinee, then, else_) => {
                let rs = check.source_stash[*scrutinee].check(check);
                check.resolver.ribs.push_scope();
                let rp = check.source_stash[*pat].check(check);
                let rt = check.source_stash[*then].check(check);
                check.resolver.ribs.pop_scope();

                let result_ty = check.fresh_ty_var();
                let then_ty = check.stash()[rt].ty;
                let then_span = check.source_stash[*then].span;
                if let Err(e) = check.require_coerce(then_ty, result_ty, then_span) {
                    check.catch(e);
                }

                let re = match else_ {
                    Some(e) => {
                        let re = check.source_stash[*e].check(check);
                        let else_ty = check.stash()[re].ty;
                        let else_span = check.source_stash[*e].span;
                        if let Err(err) = check.require_coerce(else_ty, result_ty, else_span) {
                            check.catch(err);
                        }
                        Some(re)
                    }
                    None => {
                        let unit = check.unit_ty();
                        if let Err(e) = check.require_eq(result_ty, unit, span) {
                            check.catch(e);
                        }
                        None
                    }
                };
                (TyExprData::IfLet(rp, rs, rt, re), result_ty)
            }
            ExprCstKind::Match(scrutinee, arms) => {
                let rs = check.source_stash[*scrutinee].check(check);
                let result_ty = check.fresh_ty_var();
                let rarms: Vec<_> = check.source_stash[*arms]
                    .iter()
                    .map(|arm| arm.check(check, result_ty))
                    .collect();
                let arms_slice = check.stash_mut().alloc_slice(&rarms);
                (TyExprData::Match(rs, arms_slice), result_ty)
            }
            ExprCstKind::Loop(body) => {
                let rb = check.source_stash[*body].check(check);
                let ty = check.alloc_ty(Ty::Never);
                (TyExprData::Loop(rb), ty)
            }
            ExprCstKind::While(cond, body) => {
                let rc = check.source_stash[*cond].check(check);
                let cond_ty = check.stash()[rc].ty;
                let bool_ty = check.alloc_ty(Ty::Bool);
                let cond_span = check.source_stash[*cond].span;
                if let Err(e) = check.require_eq(cond_ty, bool_ty, cond_span) {
                    check.catch(e);
                }
                let rb = check.source_stash[*body].check(check);
                let ty = check.unit_ty();
                (TyExprData::While(rc, rb), ty)
            }
            ExprCstKind::WhileLet(pat, scrutinee, body) => {
                let rs = check.source_stash[*scrutinee].check(check);
                check.resolver.ribs.push_scope();
                let rp = check.source_stash[*pat].check(check);
                let rb = check.source_stash[*body].check(check);
                check.resolver.ribs.pop_scope();
                let ty = check.unit_ty();
                (TyExprData::WhileLet(rp, rs, rb), ty)
            }
            ExprCstKind::For(pat, iter, body) => {
                let ri = check.source_stash[*iter].check(check);
                check.resolver.ribs.push_scope();
                let rp = check.source_stash[*pat].check(check);
                let rb = check.source_stash[*body].check(check);
                check.resolver.ribs.pop_scope();
                let ty = check.unit_ty();
                (TyExprData::For(rp, ri, rb), ty)
            }
            ExprCstKind::Break(val) => {
                let rv = val.map(|v| check.source_stash[v].check(check));
                let ty = check.alloc_ty(Ty::Never);
                (TyExprData::Break(rv), ty)
            }
            ExprCstKind::Continue => {
                let ty = check.alloc_ty(Ty::Never);
                (TyExprData::Continue, ty)
            }
            ExprCstKind::Return(val) => {
                let rv = val.map(|v| check.source_stash[v].check(check));
                // TODO: require_coerce to function return type
                let ty = check.alloc_ty(Ty::Never);
                (TyExprData::Return(rv), ty)
            }
            ExprCstKind::Assign(lhs, rhs) => {
                let rl = check.source_stash[*lhs].check(check);
                let rr = check.source_stash[*rhs].check(check);
                let lhs_ty = check.stash()[rl].ty;
                let rhs_ty = check.stash()[rr].ty;
                let rhs_span = check.source_stash[*rhs].span;
                if let Err(e) = check.require_coerce(rhs_ty, lhs_ty, rhs_span) {
                    check.catch(e);
                }
                let ty = check.unit_ty();
                (TyExprData::Assign(rl, rr), ty)
            }
            ExprCstKind::Await(inner) => {
                let ri = check.source_stash[*inner].check(check);
                let ty = check.fresh_ty_var(); // TODO: extract Output type
                (TyExprData::Await(ri), ty)
            }
            ExprCstKind::Try(inner) => {
                let ri = check.source_stash[*inner].check(check);
                let ty = check.fresh_ty_var(); // TODO: extract Ok type
                (TyExprData::Try(ri), ty)
            }
            ExprCstKind::Closure(params, body) => {
                check.resolver.ribs.push_scope();
                let rparams: Vec<_> = check.source_stash[*params]
                    .iter()
                    .map(|p| {
                        let rp = check.source_stash[p.pat].check(check);
                        let param_ty = check.stash()[rp].ty;
                        TyClosureParam {
                            pat: rp,
                            ty: param_ty,
                            span: p.span,
                        }
                    })
                    .collect();
                let rb = check.source_stash[*body].check(check);
                check.resolver.ribs.pop_scope();
                let params_slice = check.stash_mut().alloc_slice(&rparams);
                let ty = check.fresh_ty_var(); // TODO: construct fn type
                (TyExprData::Closure(params_slice, rb), ty)
            }
            ExprCstKind::Tuple(elems) => {
                let relems: Vec<_> = check.source_stash[*elems]
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
                let relems: Vec<_> = check.source_stash[*elems]
                    .iter()
                    .map(|e| {
                        let re = e.check_val(check);
                        let elem_ty = check.stash()[re].ty;
                        if let Err(err) = check.require_coerce(elem_ty, result_ty, e.span) {
                            check.catch(err);
                        }
                        re
                    })
                    .collect();
                let elems_slice = check.stash_mut().alloc_slice(&relems);
                let ty = check.alloc_ty(Ty::Slice(result_ty));
                (TyExprData::Array(elems_slice), ty)
            }
            ExprCstKind::Index(obj, idx) => {
                let ro = check.source_stash[*obj].check(check);
                let ri = check.source_stash[*idx].check(check);
                let ty = check.fresh_ty_var(); // TODO: Index trait resolution
                (TyExprData::Index(ro, ri), ty)
            }
            ExprCstKind::Cast(expr, ty_cst) => {
                let re = check.source_stash[*expr].check(check);
                let target_ty = check.source_stash[*ty_cst].check(check);
                let target = check.stash_mut().alloc(target_ty);
                (TyExprData::Cast(re, target), target)
            }
            ExprCstKind::StructLit(path_ptr, fields) => {
                let path = check.source_stash[*path_ptr];
                let res = check.resolve_path(path, Namespace::Type, span);
                let rfields: Vec<_> = check.source_stash[*fields]
                    .iter()
                    .map(|fi| TyFieldInit {
                        name: fi.name,
                        value: check.source_stash[fi.value].check(check),
                        span: fi.span,
                    })
                    .collect();
                let fields_slice = check.stash_mut().alloc_slice(&rfields);
                let result = struct_lit_ty(check, res);
                if let Some(local) = result.local {
                    check_struct_lit_fields(check, local, result.type_args, fields_slice);
                }
                (TyExprData::StructLit(res, fields_slice), result.ty)
            }
            ExprCstKind::Range(lo, hi) => {
                let rl = lo.map(|l| check.source_stash[l].check(check));
                let rh = hi.map(|h| check.source_stash[h].check(check));
                let ty = check.fresh_ty_var(); // TODO: Range type
                (TyExprData::Range(rl, rh), ty)
            }
            ExprCstKind::Missing => {
                let e = check.report(crate::diagnostic::Diagnostic::error(
                    check.span(span),
                    "syntax error",
                ));
                let ty = check.alloc_ty(Ty::Error(e));
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
                let rinit = init.map(|e| cx.source_stash[e].check(cx));
                let rpat = cx.source_stash[*pat].check(cx);
                let pat_ty = cx.stash()[rpat].ty;

                if let Some(init_ptr) = rinit {
                    let init_ty = cx.stash()[init_ptr].ty;
                    let init_span = cx.stash()[init_ptr].span;
                    if let Err(e) = cx.require_coerce(init_ty, pat_ty, init_span) {
                        cx.catch(e);
                    }
                }

                let ty_ann_ptr = ty_ann.map(|t| {
                    let resolved = cx.source_stash[t].check(cx);
                    cx.stash_mut().alloc(resolved)
                });

                TyStmtKind::Let(rpat, ty_ann_ptr, rinit)
            }
            StmtCstKind::Expr(e) => TyStmtKind::Expr(cx.source_stash[*e].check(cx)),
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
                let local_ty = cx.local_type(id.0);
                cx.assume_eq(ty, local_ty);
                TyPatKind::Bind(id, *mutability)
            }
            PatCstKind::Path(path_ptr) => {
                let path = cx.source_stash[*path_ptr];
                let res = cx.resolve_path(path, Namespace::Value, span);
                TyPatKind::Path(res)
            }
            PatCstKind::Tuple(pats) => {
                let rpats: Vec<_> = cx.source_stash[*pats].iter().map(|p| p.check(cx)).collect();
                let pats_slice = cx.stash_mut().alloc_slice(&rpats);
                TyPatKind::Tuple(pats_slice)
            }
            PatCstKind::Struct(path_ptr, fields) => {
                let path = cx.source_stash[*path_ptr];
                let res = cx.resolve_path(path, Namespace::Type, span);
                let rfields: Vec<_> = cx.source_stash[*fields]
                    .iter()
                    .map(|fp| TyFieldPat {
                        name: fp.name,
                        pat: cx.source_stash[fp.pat].check(cx),
                        span: fp.span,
                    })
                    .collect();
                let fields_slice = cx.stash_mut().alloc_slice(&rfields);
                TyPatKind::Struct(res, fields_slice)
            }
            PatCstKind::TupleStruct(path_ptr, pats) => {
                let path = cx.source_stash[*path_ptr];
                let res = cx.resolve_path(path, Namespace::Value, span);
                let rpats: Vec<_> = cx.source_stash[*pats].iter().map(|p| p.check(cx)).collect();
                let pats_slice = cx.stash_mut().alloc_slice(&rpats);
                TyPatKind::TupleStruct(res, pats_slice)
            }
            PatCstKind::Ref(inner, m) => {
                let ri = cx.source_stash[*inner].check(cx);
                TyPatKind::Ref(ri, *m)
            }
            PatCstKind::Literal(lit) => TyPatKind::Literal(*lit),
            PatCstKind::Or(pats) => {
                let rpats: Vec<_> = cx.source_stash[*pats].iter().map(|p| p.check(cx)).collect();
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
        let rp = cx.source_stash[self.pat].check(cx);
        let rg = self.guard.map(|g| cx.source_stash[g].check(cx));
        let rb = cx.source_stash[self.body].check(cx);
        let body_ty = cx.stash()[rb].ty;
        let body_span = cx.source_stash[self.body].span;
        if let Err(e) = cx.require_coerce(body_ty, result_ty, body_span) {
            cx.catch(e);
        }
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

fn check_literal_ty<'db>(cx: &mut BodyCheck<'_, 'db>, lit: Literal<'db>) -> Ptr<Ty<'db>> {
    match lit {
        Literal::Bool(_) => cx.alloc_ty(Ty::Bool),
        Literal::Int(_) => cx.fresh_ty_var(),
        Literal::Float(_) => cx.fresh_ty_var(),
        Literal::String(_) => {
            let str_ty = cx.alloc_ty(Ty::Str);
            cx.alloc_ty(Ty::Ref(
                str_ty,
                crate::cst::Mutability::Shared,
                crate::ty::Lifetime::Static,
            ))
        }
        Literal::Char(_) => cx.alloc_ty(Ty::Char),
    }
}

fn res_to_ty<'db>(cx: &mut BodyCheck<'_, 'db>, res: Res<'db>) -> Ptr<Ty<'db>> {
    match res {
        Res::Local(LocalId(id)) => cx.local_type(id),
        Res::Def(sym) => def_to_ty(cx, sym),
        Res::Error(e) => cx.alloc_ty(Ty::Error(e)),
    }
}

fn def_to_ty<'db>(cx: &mut BodyCheck<'_, 'db>, sym: crate::symbol::Symbol<'db>) -> Ptr<Ty<'db>> {
    use crate::symbol::SymbolData;
    use crate::ty::BinderExt;

    match sym.data(cx.db) {
        SymbolData::FnSymbol(crate::symbol::FnSymbol::Local(local)) => {
            let sig = local.sig(cx.db);
            let sig_stash = sig.stash();
            let binder = sig.root();

            let type_arg_ptrs: Vec<_> = sig.iter_symbols().map(|_| cx.fresh_ty_var()).collect();
            let type_args: Vec<_> = type_arg_ptrs.iter().map(|&ptr| cx.stash()[ptr]).collect();
            let instantiated =
                crate::ty_fold::instantiate_fn_sig(sig_stash, cx.stash_mut(), &binder, type_args);
            cx.alloc_ty(Ty::FnPtr(instantiated.params, instantiated.ret))
        }
        _ => cx.fresh_ty_var(),
    }
}

fn check_binary_op_ty<'db>(
    cx: &mut BodyCheck<'_, 'db>,
    op: BinaryOp,
    lhs_ty: Ptr<Ty<'db>>,
    rhs_ty: Ptr<Ty<'db>>,
    span: RelativeSpan,
) -> Ptr<Ty<'db>> {
    if let Err(e) = cx.require_eq(rhs_ty, lhs_ty, span) {
        cx.catch(e);
    }
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

struct StructLitResult<'db> {
    ty: Ptr<Ty<'db>>,
    local: Option<crate::local_syms::structs::LocalStructSym<'db>>,
    type_args: Slice<Ptr<Ty<'db>>>,
}

fn struct_lit_ty<'db>(cx: &mut BodyCheck<'_, 'db>, res: Res<'db>) -> StructLitResult<'db> {
    use crate::symbol::SymbolData;
    use crate::ty::BinderExt;

    let sym = match res {
        Res::Def(sym) => sym,
        Res::Error(e) => {
            let empty = cx.stash_mut().alloc_slice(&[]);
            return StructLitResult {
                ty: cx.alloc_ty(Ty::Error(e)),
                local: None,
                type_args: empty,
            };
        }
        Res::Local(_) => {
            let e = cx.report(crate::diagnostic::Diagnostic::error(
                cx.span(crate::span::RelativeSpan { start: 0, end: 0 }),
                "expected struct type",
            ));
            let empty = cx.stash_mut().alloc_slice(&[]);
            return StructLitResult {
                ty: cx.alloc_ty(Ty::Error(e)),
                local: None,
                type_args: empty,
            };
        }
    };

    match sym.data(cx.db) {
        SymbolData::StructSymbol(crate::symbol::StructSymbol::Local(local)) => {
            let sig = local.sig(cx.db);
            let type_args: Vec<_> = sig.iter_symbols().map(|_| cx.fresh_ty_var()).collect();
            let type_args_slice = cx.stash_mut().alloc_slice(&type_args);
            StructLitResult {
                ty: cx.alloc_ty(Ty::Adt(sym, type_args_slice)),
                local: Some(local),
                type_args: type_args_slice,
            }
        }
        SymbolData::StructSymbol(crate::symbol::StructSymbol::Ext(_)) => {
            let type_args_slice = cx.stash_mut().alloc_slice(&[]);
            StructLitResult {
                ty: cx.alloc_ty(Ty::Adt(sym, type_args_slice)),
                local: None,
                type_args: type_args_slice,
            }
        }
        _ => {
            let e = cx.report(crate::diagnostic::Diagnostic::error(
                cx.span(crate::span::RelativeSpan { start: 0, end: 0 }),
                "expected struct type",
            ));
            let empty = cx.stash_mut().alloc_slice(&[]);
            StructLitResult {
                ty: cx.alloc_ty(Ty::Error(e)),
                local: None,
                type_args: empty,
            }
        }
    }
}

fn check_struct_lit_fields<'db>(
    cx: &mut BodyCheck<'_, 'db>,
    local: crate::local_syms::structs::LocalStructSym<'db>,
    type_args: Slice<Ptr<Ty<'db>>>,
    fields: Slice<TyFieldInit<'db>>,
) {
    use crate::ty::BinderExt;
    use crate::ty_fold::{SubstTarget, Substitute, TyFolder};
    use rustc_hash::FxHashMap;

    let sig = local.sig(cx.db);
    let generic_params: Vec<_> = sig.iter_symbols().collect();
    let type_arg_ptrs: Vec<_> = cx.stash()[type_args].to_vec();

    let mut subst = FxHashMap::default();
    for (param, &arg_ptr) in generic_params.iter().zip(type_arg_ptrs.iter()) {
        subst.insert(*param, SubstTarget::Ty(cx.stash()[arg_ptr]));
    }

    let fields_stashed = local.fields(cx.db);
    let fields_stash = fields_stashed.stash();
    let struct_fields = fields_stashed.root();
    let field_sigs = &fields_stash[struct_fields.fields];

    let init_fields: Vec<_> = cx.stash()[fields].to_vec();
    for field_init in &init_fields {
        for field_sig in field_sigs {
            if field_sig.name == field_init.name {
                let declared_ty = if subst.is_empty() {
                    use sage_stash::StashCopy;
                    field_sig.ty.stash_copy(fields_stash, cx.stash_mut())
                } else {
                    let mut folder = Substitute::new(fields_stash, cx.stash_mut(), subst.clone());
                    let ty_data = folder.fold_ty(fields_stash[field_sig.ty]);
                    cx.stash_mut().alloc(ty_data)
                };
                let init_ty = cx.stash()[field_init.value].ty;
                if let Err(e) = cx.require_coerce(init_ty, declared_ty, field_init.span) {
                    let e = e.with_context(crate::check::body::ErrorContext::FieldInit {
                        field_span: field_init.span,
                    });
                    cx.catch(e);
                }
                break;
            }
        }
    }
}

fn lookup_field_ty<'db>(
    cx: &mut BodyCheck<'_, 'db>,
    obj: Ptr<TyExpr<'db>>,
    field_name: Name<'db>,
) -> Ptr<Ty<'db>> {
    use crate::symbol::SymbolData;
    use crate::ty::BinderExt;
    use crate::ty_fold::{SubstTarget, Substitute, TyFolder};
    use rustc_hash::FxHashMap;

    let obj_ty_ptr = cx.stash()[obj].ty;
    let obj_ty_ptr = cx.find_mut(obj_ty_ptr);
    let obj_ty = cx.stash()[obj_ty_ptr];

    let (sym, type_args) = match obj_ty {
        Ty::Adt(sym, type_args) => (sym, type_args),
        _ => return cx.fresh_ty_var(),
    };

    match sym.data(cx.db) {
        SymbolData::StructSymbol(crate::symbol::StructSymbol::Local(local)) => {
            let sig = local.sig(cx.db);
            let generic_params: Vec<_> = sig.iter_symbols().collect();
            let type_arg_ptrs: Vec<_> = cx.stash()[type_args].to_vec();

            let fields_stashed = local.fields(cx.db);
            let fields_stash = fields_stashed.stash();
            let struct_fields = fields_stashed.root();
            let field_sigs = &fields_stash[struct_fields.fields];

            for field_sig in field_sigs {
                if field_sig.name == field_name {
                    let mut subst = FxHashMap::default();
                    for (param, &arg_ptr) in generic_params.iter().zip(type_arg_ptrs.iter()) {
                        subst.insert(*param, SubstTarget::Ty(cx.stash()[arg_ptr]));
                    }
                    let mut folder = Substitute::new(fields_stash, cx.stash_mut(), subst);
                    let field_ty_data = folder.fold_ty(fields_stash[field_sig.ty]);
                    return cx.stash_mut().alloc(field_ty_data);
                }
            }
            cx.fresh_ty_var()
        }
        _ => cx.fresh_ty_var(),
    }
}

fn check_call_ty<'db>(
    cx: &mut BodyCheck<'_, 'db>,
    callee: Ptr<TyExpr<'db>>,
    arg_exprs: Slice<Ptr<TyExpr<'db>>>,
    call_span: RelativeSpan,
) -> Ptr<Ty<'db>> {
    let callee_ty_ptr = cx.stash()[callee].ty;
    let callee_ty_ptr = cx.find_mut(callee_ty_ptr);
    let callee_ty = cx.stash()[callee_ty_ptr];

    match callee_ty {
        Ty::FnPtr(params, ret) => {
            let param_tys: Vec<_> = cx.stash()[params].to_vec();
            let arg_ptrs: Vec<_> = cx.stash()[arg_exprs].to_vec();
            for (param_ty, arg_expr) in param_tys.iter().zip(arg_ptrs.iter()) {
                let arg_ty = cx.stash()[*arg_expr].ty;
                let arg_span = cx.stash()[*arg_expr].span;
                if let Err(e) = cx.require_eq(arg_ty, *param_ty, arg_span) {
                    cx.catch(e);
                }
            }
            ret
        }
        _ => {
            let _ = call_span;
            cx.fresh_ty_var()
        }
    }
}
