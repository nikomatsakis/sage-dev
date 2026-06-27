use sage_macros_from_impls::boxed_async_fn;
use sage_stash::Ptr;

use crate::cst::Mutability;
use crate::cst::expr::*;
use crate::diagnostic::Diagnostic;
use crate::resolve::Namespace;
use crate::span::RelativeSpan;
use crate::ty::Ty;
use crate::tytree::*;

use super::infer_ctx::{CheckError, ErrorContext, InferCtx, RecordErr, Scope, resolve_path};

type CheckResult<'db> = Result<Ptr<TyExpr<'db>>, CheckError<'db>>;

impl<'db> ExprCst<'db> {
    /// Check an expression, catching fatal errors and substituting error nodes.
    pub(crate) async fn check_with(
        &self,
        cx: &InferCtx<'_, 'db>,
        scope: &Scope<'db>,
    ) -> Ptr<TyExpr<'db>> {
        match self.check_expr(cx, scope).await {
            Ok(expr) => expr,
            Err(err) => cx.error_expr(err, self.span),
        }
    }

    #[boxed_async_fn]
    async fn check_expr(&self, cx: &InferCtx<'_, 'db>, scope: &Scope<'db>) -> CheckResult<'db> {
        let span = self.span;
        let (kind, ty) = match &self.kind {
            ExprCstKind::Literal(lit) => {
                let ty = check_literal_ty(cx, *lit);
                (TyExprData::Literal(*lit), ty)
            }
            ExprCstKind::Path(path_ptr) => {
                let path = cx.source_stash[*path_ptr];
                let res = resolve_path(cx, scope, path, Namespace::Value, span);
                let ty = res_to_ty(cx, scope, res);
                (TyExprData::Path(res), ty)
            }
            ExprCstKind::Block(stmts, tail) => {
                let mut scope = scope.clone();
                scope.resolver.ribs.push_scope();
                let mut rstmts = Vec::new();
                for s in cx.source_stash[*stmts].iter() {
                    rstmts.push(s.check_stmt(cx, &mut scope).await);
                }
                let stmts_slice = cx.stash_mut().alloc_slice(&rstmts);
                let (tail_ptr, ty) = match tail {
                    Some(t) => {
                        let te = cx.source_stash[*t].check_with(cx, &scope).await;
                        let ty = cx.stash()[te].ty;
                        (Some(te), ty)
                    }
                    None => (None, cx.unit_ty()),
                };
                scope.resolver.ribs.pop_scope();
                (TyExprData::Block(stmts_slice, tail_ptr), ty)
            }
            ExprCstKind::Call(func, args) => {
                let rf = cx.source_stash[*func].check_with(cx, scope).await;
                let mut rargs = Vec::new();
                for a in cx.source_stash[*args].iter() {
                    rargs.push(a.check_with(cx, scope).await);
                }
                let args_slice = cx.stash_mut().alloc_slice(&rargs);
                let callee_ty = cx.find_mut(cx.stash()[rf].ty);
                let ty = check_call_ty(cx, callee_ty, args_slice, span);
                (TyExprData::Call(rf, args_slice), ty)
            }
            ExprCstKind::MethodCall(obj, name, args) => {
                let ro = cx.source_stash[*obj].check_with(cx, scope).await;
                let mut rargs = Vec::new();
                for a in cx.source_stash[*args].iter() {
                    rargs.push(a.check_with(cx, scope).await);
                }
                let args_slice = cx.stash_mut().alloc_slice(&rargs);
                let ty = cx.fresh_ty_var();
                (TyExprData::MethodCall(ro, *name, args_slice), ty)
            }
            ExprCstKind::Field(obj, name) => {
                let ro = cx.source_stash[*obj].check_with(cx, scope).await;
                let obj_ty = cx.find_mut(cx.stash()[ro].ty);
                let ty = lookup_field_ty(cx, obj_ty, *name);
                (TyExprData::Field(ro, *name), ty)
            }
            ExprCstKind::Binary(lhs, op, rhs) => {
                let rl = cx.source_stash[*lhs].check_with(cx, scope).await;
                let rr = cx.source_stash[*rhs].check_with(cx, scope).await;
                let lhs_ty = cx.stash()[rl].ty;
                let rhs_ty = cx.stash()[rr].ty;
                let ty = check_binary_op_ty(cx, *op, lhs_ty, rhs_ty, span);
                (TyExprData::Binary(rl, *op, rr), ty)
            }
            ExprCstKind::Unary(op, operand) => {
                let ro = cx.source_stash[*operand].check_with(cx, scope).await;
                let ty = cx.stash()[ro].ty;
                (TyExprData::Unary(*op, ro), ty)
            }
            ExprCstKind::Ref(inner, m) => {
                let ri = cx.source_stash[*inner].check_with(cx, scope).await;
                let inner_ty = cx.stash()[ri].ty;
                let ty = cx.alloc_ty(Ty::Ref(inner_ty, *m, crate::ty::Lifetime::Erased));
                (TyExprData::Ref(ri, *m), ty)
            }
            ExprCstKind::If(cond, then, else_) => {
                let rc = cx.source_stash[*cond].check_with(cx, scope).await;
                let cond_ty = cx.stash()[rc].ty;
                let bool_ty = cx.alloc_ty(Ty::Bool);
                let cond_span = cx.source_stash[*cond].span;
                cx.require_eq(cond_ty, bool_ty, cond_span).record_err(cx);

                let result_ty = cx.fresh_ty_var();
                let rt = cx.source_stash[*then].check_with(cx, scope).await;
                let then_ty = cx.stash()[rt].ty;
                let then_span = cx.source_stash[*then].span;
                cx.require_coerce(then_ty, result_ty, then_span)
                    .record_err(cx);

                let re = match else_ {
                    Some(e) => {
                        let re = cx.source_stash[*e].check_with(cx, scope).await;
                        let else_ty = cx.stash()[re].ty;
                        let else_span = cx.source_stash[*e].span;
                        cx.require_coerce(else_ty, result_ty, else_span)
                            .record_err(cx);
                        Some(re)
                    }
                    None => {
                        let unit = cx.unit_ty();
                        cx.require_eq(result_ty, unit, span).record_err(cx);
                        None
                    }
                };
                (TyExprData::If(rc, rt, re), result_ty)
            }
            ExprCstKind::IfLet(pat, scrutinee, then, else_) => {
                let rs = cx.source_stash[*scrutinee].check_with(cx, scope).await;
                let mut inner_scope = scope.clone();
                inner_scope.resolver.ribs.push_scope();
                let rp = cx.source_stash[*pat].check_pat(cx, &mut inner_scope);
                let rt = cx.source_stash[*then].check_with(cx, &inner_scope).await;
                inner_scope.resolver.ribs.pop_scope();

                let result_ty = cx.fresh_ty_var();
                let then_ty = cx.stash()[rt].ty;
                let then_span = cx.source_stash[*then].span;
                cx.require_coerce(then_ty, result_ty, then_span)
                    .record_err(cx);

                let re = match else_ {
                    Some(e) => {
                        let re = cx.source_stash[*e].check_with(cx, scope).await;
                        let else_ty = cx.stash()[re].ty;
                        let else_span = cx.source_stash[*e].span;
                        cx.require_coerce(else_ty, result_ty, else_span)
                            .record_err(cx);
                        Some(re)
                    }
                    None => {
                        let unit = cx.unit_ty();
                        cx.require_eq(result_ty, unit, span).record_err(cx);
                        None
                    }
                };
                (TyExprData::IfLet(rp, rs, rt, re), result_ty)
            }
            ExprCstKind::Match(scrutinee, arms) => {
                let rs = cx.source_stash[*scrutinee].check_with(cx, scope).await;
                let result_ty = cx.fresh_ty_var();
                let mut rarms = Vec::new();
                for arm in cx.source_stash[*arms].iter() {
                    rarms.push(arm.check_arm(cx, scope, result_ty).await);
                }
                let arms_slice = cx.stash_mut().alloc_slice(&rarms);
                (TyExprData::Match(rs, arms_slice), result_ty)
            }
            ExprCstKind::Loop(body) => {
                let rb = cx.source_stash[*body].check_with(cx, scope).await;
                let ty = cx.alloc_ty(Ty::Never);
                (TyExprData::Loop(rb), ty)
            }
            ExprCstKind::While(cond, body) => {
                let rc = cx.source_stash[*cond].check_with(cx, scope).await;
                let cond_ty = cx.stash()[rc].ty;
                let bool_ty = cx.alloc_ty(Ty::Bool);
                let cond_span = cx.source_stash[*cond].span;
                cx.require_eq(cond_ty, bool_ty, cond_span).record_err(cx);
                let rb = cx.source_stash[*body].check_with(cx, scope).await;
                let ty = cx.unit_ty();
                (TyExprData::While(rc, rb), ty)
            }
            ExprCstKind::WhileLet(pat, scrutinee, body) => {
                let rs = cx.source_stash[*scrutinee].check_with(cx, scope).await;
                let mut scope = scope.clone();
                scope.resolver.ribs.push_scope();
                let rp = cx.source_stash[*pat].check_pat(cx, &mut scope);
                let rb = cx.source_stash[*body].check_with(cx, &scope).await;
                scope.resolver.ribs.pop_scope();
                let ty = cx.unit_ty();
                (TyExprData::WhileLet(rp, rs, rb), ty)
            }
            ExprCstKind::For(pat, iter, body) => {
                let ri = cx.source_stash[*iter].check_with(cx, scope).await;
                let mut scope = scope.clone();
                scope.resolver.ribs.push_scope();
                let rp = cx.source_stash[*pat].check_pat(cx, &mut scope);
                let rb = cx.source_stash[*body].check_with(cx, &scope).await;
                scope.resolver.ribs.pop_scope();
                let ty = cx.unit_ty();
                (TyExprData::For(rp, ri, rb), ty)
            }
            ExprCstKind::Break(val) => {
                let rv = match val {
                    Some(v) => Some(cx.source_stash[*v].check_with(cx, scope).await),
                    None => None,
                };
                let ty = cx.alloc_ty(Ty::Never);
                (TyExprData::Break(rv), ty)
            }
            ExprCstKind::Continue => {
                let ty = cx.alloc_ty(Ty::Never);
                (TyExprData::Continue, ty)
            }
            ExprCstKind::Return(val) => {
                let rv = match val {
                    Some(v) => Some(cx.source_stash[*v].check_with(cx, scope).await),
                    None => None,
                };
                if let Some((ret_ty, ret_span)) = cx.ret_ty() {
                    let val_ty = match rv {
                        Some(expr) => cx.stash()[expr].ty,
                        None => cx.unit_ty(),
                    };
                    if let Err(e) = cx.require_coerce(val_ty, ret_ty, span) {
                        let e = if let Some(ret_span) = ret_span {
                            e.with_context(ErrorContext::ReturnType { ret_span })
                        } else {
                            e
                        };
                        cx.catch(e);
                    }
                }
                let ty = cx.alloc_ty(Ty::Never);
                (TyExprData::Return(rv), ty)
            }
            ExprCstKind::Assign(lhs, rhs) => {
                let rl = cx.source_stash[*lhs].check_with(cx, scope).await;
                let rr = cx.source_stash[*rhs].check_with(cx, scope).await;
                let lhs_ty = cx.stash()[rl].ty;
                let rhs_ty = cx.stash()[rr].ty;
                let rhs_span = cx.source_stash[*rhs].span;
                cx.require_coerce(rhs_ty, lhs_ty, rhs_span).record_err(cx);
                let ty = cx.unit_ty();
                (TyExprData::Assign(rl, rr), ty)
            }
            ExprCstKind::Await(inner) => {
                let ri = cx.source_stash[*inner].check_with(cx, scope).await;
                let ty = cx.fresh_ty_var();
                (TyExprData::Await(ri), ty)
            }
            ExprCstKind::Try(inner) => {
                let ri = cx.source_stash[*inner].check_with(cx, scope).await;
                let ty = cx.fresh_ty_var();
                (TyExprData::Try(ri), ty)
            }
            ExprCstKind::Closure(params, body) => {
                let mut scope = scope.clone();
                scope.resolver.ribs.push_scope();
                let rparams: Vec<_> = cx.source_stash[*params]
                    .iter()
                    .map(|p| {
                        let rp = cx.source_stash[p.pat].check_pat(cx, &mut scope);
                        let param_ty = cx.stash()[rp].ty;
                        TyClosureParam {
                            pat: rp,
                            ty: param_ty,
                            span: p.span,
                        }
                    })
                    .collect();
                let rb = cx.source_stash[*body].check_with(cx, &scope).await;
                scope.resolver.ribs.pop_scope();
                let params_slice = cx.stash_mut().alloc_slice(&rparams);
                let ty = cx.fresh_ty_var();
                (TyExprData::Closure(params_slice, rb), ty)
            }
            ExprCstKind::Tuple(elems) => {
                let mut relems = Vec::new();
                for e in cx.source_stash[*elems].iter() {
                    relems.push(e.check_with(cx, scope).await);
                }
                let elem_tys: Vec<Ptr<Ty<'db>>> =
                    relems.iter().map(|e| cx.stash()[*e].ty).collect();
                let elems_slice = cx.stash_mut().alloc_slice(&relems);
                let ty_elems = cx.stash_mut().alloc_slice(&elem_tys);
                let ty = cx.alloc_ty(Ty::Tuple(ty_elems));
                (TyExprData::Tuple(elems_slice), ty)
            }
            ExprCstKind::Array(elems) => {
                let result_ty = cx.fresh_ty_var();
                let mut relems = Vec::new();
                for e in cx.source_stash[*elems].iter() {
                    let re = e.check_with(cx, scope).await;
                    let elem_ty = cx.stash()[re].ty;
                    cx.require_coerce(elem_ty, result_ty, e.span).record_err(cx);
                    relems.push(re);
                }
                let elems_slice = cx.stash_mut().alloc_slice(&relems);
                let ty = cx.alloc_ty(Ty::Slice(result_ty));
                (TyExprData::Array(elems_slice), ty)
            }
            ExprCstKind::Index(obj, idx) => {
                let ro = cx.source_stash[*obj].check_with(cx, scope).await;
                let ri = cx.source_stash[*idx].check_with(cx, scope).await;
                let ty = cx.fresh_ty_var();
                (TyExprData::Index(ro, ri), ty)
            }
            ExprCstKind::Cast(expr, ty_cst) => {
                let re = cx.source_stash[*expr].check_with(cx, scope).await;
                let target_ty = cx.source_stash[*ty_cst].check_ty(cx, scope);
                let target = cx.stash_mut().alloc(target_ty);
                (TyExprData::Cast(re, target), target)
            }
            ExprCstKind::StructLit(path_ptr, fields) => {
                let path = cx.source_stash[*path_ptr];
                let res = resolve_path(cx, scope, path, Namespace::Type, span);
                let mut rfields = Vec::new();
                for fi in cx.source_stash[*fields].iter() {
                    rfields.push(TyFieldInit {
                        name: fi.name,
                        value: cx.source_stash[fi.value].check_with(cx, scope).await,
                        span: fi.span,
                    });
                }
                let fields_slice = cx.stash_mut().alloc_slice(&rfields);
                let result = struct_lit_ty(cx, res);
                if let Some(local) = result.local {
                    check_struct_lit_fields(cx, local, result.type_args, fields_slice);
                }
                (TyExprData::StructLit(res, fields_slice), result.ty)
            }
            ExprCstKind::Range(lo, hi) => {
                let rl = match lo {
                    Some(l) => Some(cx.source_stash[*l].check_with(cx, scope).await),
                    None => None,
                };
                let rh = match hi {
                    Some(h) => Some(cx.source_stash[*h].check_with(cx, scope).await),
                    None => None,
                };
                let ty = cx.fresh_ty_var();
                (TyExprData::Range(rl, rh), ty)
            }
            ExprCstKind::Missing => {
                let e = cx.record(Diagnostic::error(cx.span(span), "syntax error"));
                let ty = cx.alloc_ty(Ty::Error(e));
                (TyExprData::Missing, ty)
            }
        };
        Ok(cx.alloc_expr(kind, ty, span))
    }
}

// ---------------------------------------------------------------------------
// Statements (synchronous — they introduce bindings sequentially)
// ---------------------------------------------------------------------------

impl<'db> StmtCst<'db> {
    pub(crate) async fn check_stmt(
        &self,
        cx: &InferCtx<'_, 'db>,
        scope: &mut Scope<'db>,
    ) -> TyStmt<'db> {
        let span = self.span;
        let kind = match &self.kind {
            StmtCstKind::Let(pat, ty_ann, init) => {
                let rinit = match init {
                    Some(e) => Some(cx.source_stash[*e].check_with(cx, scope).await),
                    None => None,
                };
                let rpat = cx.source_stash[*pat].check_pat(cx, scope);
                let pat_ty = cx.stash()[rpat].ty;

                if let Some(init_ptr) = rinit {
                    let init_ty = cx.stash()[init_ptr].ty;
                    let init_span = cx.stash()[init_ptr].span;
                    cx.require_coerce(init_ty, pat_ty, init_span).record_err(cx);
                }

                let ty_ann_ptr = ty_ann.map(|t| {
                    let resolved = cx.source_stash[t].check_ty(cx, scope);
                    cx.stash_mut().alloc(resolved)
                });

                TyStmtKind::Let(rpat, ty_ann_ptr, rinit)
            }
            StmtCstKind::Expr(e) => {
                TyStmtKind::Expr(cx.source_stash[*e].check_with(cx, scope).await)
            }
        };
        TyStmt { kind, span }
    }
}

// ---------------------------------------------------------------------------
// Patterns (synchronous — they introduce bindings into scope)
// ---------------------------------------------------------------------------

impl<'db> PatCst<'db> {
    /// Check a pattern, mutating the scope to add bindings.
    pub(crate) fn check_pat(
        &self,
        cx: &InferCtx<'_, 'db>,
        scope: &mut Scope<'db>,
    ) -> Ptr<TyPat<'db>> {
        let span = self.span;
        let ty = cx.fresh_ty_var();
        let kind = match &self.kind {
            PatCstKind::Wildcard => TyPatKind::Wildcard,
            PatCstKind::Bind(name, mutability) => {
                let id = scope.add_binding(cx, *name, span);
                let local_ty = scope.local_type(id.0);
                cx.assume_eq(ty, local_ty);
                TyPatKind::Bind(id, *mutability)
            }
            PatCstKind::Path(path_ptr) => {
                let path = cx.source_stash[*path_ptr];
                let res = resolve_path(cx, scope, path, Namespace::Value, span);
                TyPatKind::Path(res)
            }
            PatCstKind::Tuple(pats) => {
                let rpats: Vec<_> = cx.source_stash[*pats]
                    .iter()
                    .map(|p| p.check_pat(cx, scope))
                    .collect();
                let pats_slice = cx.stash_mut().alloc_slice(&rpats);
                TyPatKind::Tuple(pats_slice)
            }
            PatCstKind::Struct(path_ptr, fields) => {
                let path = cx.source_stash[*path_ptr];
                let res = resolve_path(cx, scope, path, Namespace::Type, span);
                let rfields: Vec<_> = cx.source_stash[*fields]
                    .iter()
                    .map(|fp| TyFieldPat {
                        name: fp.name,
                        pat: cx.source_stash[fp.pat].check_pat(cx, scope),
                        span: fp.span,
                    })
                    .collect();
                let fields_slice = cx.stash_mut().alloc_slice(&rfields);
                TyPatKind::Struct(res, fields_slice)
            }
            PatCstKind::TupleStruct(path_ptr, pats) => {
                let path = cx.source_stash[*path_ptr];
                let res = resolve_path(cx, scope, path, Namespace::Value, span);
                let rpats: Vec<_> = cx.source_stash[*pats]
                    .iter()
                    .map(|p| p.check_pat(cx, scope))
                    .collect();
                let pats_slice = cx.stash_mut().alloc_slice(&rpats);
                TyPatKind::TupleStruct(res, pats_slice)
            }
            PatCstKind::Ref(inner, m) => {
                let ri = cx.source_stash[*inner].check_pat(cx, scope);
                TyPatKind::Ref(ri, *m)
            }
            PatCstKind::Literal(lit) => TyPatKind::Literal(*lit),
            PatCstKind::Or(pats) => {
                let rpats: Vec<_> = cx.source_stash[*pats]
                    .iter()
                    .map(|p| p.check_pat(cx, scope))
                    .collect();
                let pats_slice = cx.stash_mut().alloc_slice(&rpats);
                TyPatKind::Or(pats_slice)
            }
            PatCstKind::Rest => TyPatKind::Rest,
            PatCstKind::Missing => TyPatKind::Missing,
        };
        cx.stash_mut().alloc(TyPat { kind, ty, span })
    }

    /// Check a pattern without mutating scope (read-only, for non-binding patterns).
    pub(crate) fn check_pat_shared(
        &self,
        cx: &InferCtx<'_, 'db>,
        scope: &Scope<'db>,
    ) -> Ptr<TyPat<'db>> {
        let mut scope = scope.clone();
        self.check_pat(cx, &mut scope)
    }
}

// ---------------------------------------------------------------------------
// Match arms
// ---------------------------------------------------------------------------

impl<'db> MatchArmCst<'db> {
    #[boxed_async_fn]
    pub(crate) async fn check_arm(
        &self,
        cx: &InferCtx<'_, 'db>,
        scope: &Scope<'db>,
        result_ty: Ptr<Ty<'db>>,
    ) -> TyMatchArm<'db> {
        let mut scope = scope.clone();
        scope.resolver.ribs.push_scope();
        let rp = cx.source_stash[self.pat].check_pat(cx, &mut scope);
        let rg = match self.guard {
            Some(g) => Some(cx.source_stash[g].check_with(cx, &scope).await),
            None => None,
        };
        let rb = cx.source_stash[self.body].check_with(cx, &scope).await;
        let body_ty = cx.stash()[rb].ty;
        let body_span = cx.source_stash[self.body].span;
        cx.require_coerce(body_ty, result_ty, body_span)
            .record_err(cx);
        scope.resolver.ribs.pop_scope();
        TyMatchArm {
            pat: rp,
            guard: rg,
            body: rb,
            span: self.span,
        }
    }
}

// ---------------------------------------------------------------------------
// TypeCst checking (synchronous)
// ---------------------------------------------------------------------------

use crate::cst::ty::{TypeCst, TypeCstKind};
use crate::symbol::intrinsic::Intrinsic;
use crate::ty::Lifetime;

impl<'db> TypeCst<'db> {
    pub(crate) fn check_ty(&self, cx: &InferCtx<'_, 'db>, scope: &Scope<'db>) -> Ty<'db> {
        use crate::resolve::Resolution;
        use crate::symbol::SymbolData;

        match self.kind {
            TypeCstKind::Path(path_ptr) => {
                let path = cx.source_stash[path_ptr];
                let mut resolver = scope.resolver.clone();
                let results = resolver.resolve_path(cx.source_stash, path, Namespace::Type);
                match results.into_iter().next() {
                    Some(Resolution::Sym(sym)) => match sym.data(cx.db) {
                        SymbolData::IntrinsicTypeSymbol(s) => intrinsic_to_ty(s.intrinsic(cx.db)),
                        _ => {
                            let type_args = cx.stash_mut().alloc_slice(&[]);
                            Ty::Adt(sym, type_args)
                        }
                    },
                    Some(Resolution::Param(param)) => Ty::Param(param),
                    Some(Resolution::SelfTy(ty)) => ty,
                    Some(Resolution::Local(_)) | None => {
                        let e = cx.record(Diagnostic::error(cx.span(self.span), "unresolved type"));
                        Ty::Error(e)
                    }
                }
            }
            TypeCstKind::Reference(inner, m) => {
                let inner_ty = cx.source_stash[inner].check_ty(cx, scope);
                let inner_ptr = cx.stash_mut().alloc(inner_ty);
                Ty::Ref(inner_ptr, m, Lifetime::Erased)
            }
            TypeCstKind::Tuple(elems) => {
                let tys: Vec<_> = cx.source_stash[elems]
                    .iter()
                    .map(|e| e.check_ty(cx, scope))
                    .collect();
                let ptrs: Vec<_> = tys.into_iter().map(|t| cx.stash_mut().alloc(t)).collect();
                let elems_slice = cx.stash_mut().alloc_slice(&ptrs);
                Ty::Tuple(elems_slice)
            }
            TypeCstKind::Slice(inner) => {
                let inner_ty = cx.source_stash[inner].check_ty(cx, scope);
                let inner_ptr = cx.stash_mut().alloc(inner_ty);
                Ty::Slice(inner_ptr)
            }
            TypeCstKind::Array(inner) => {
                let inner_ty = cx.source_stash[inner].check_ty(cx, scope);
                let inner_ptr = cx.stash_mut().alloc(inner_ty);
                Ty::Array(inner_ptr, crate::ty::Const::Literal(0))
            }
            TypeCstKind::Fn(params, ret) => {
                let param_tys: Vec<_> = cx.source_stash[params]
                    .iter()
                    .map(|p| p.check_ty(cx, scope))
                    .collect();
                let param_ptrs: Vec<_> = param_tys
                    .into_iter()
                    .map(|t| cx.stash_mut().alloc(t))
                    .collect();
                let param_slice = cx.stash_mut().alloc_slice(&param_ptrs);
                let ret_ty = match ret {
                    Some(r) => cx.source_stash[r].check_ty(cx, scope),
                    None => {
                        let unit = cx.stash_mut().alloc_slice(&[]);
                        Ty::Tuple(unit)
                    }
                };
                let ret_ptr = cx.stash_mut().alloc(ret_ty);
                Ty::FnPtr(param_slice, ret_ptr)
            }
            TypeCstKind::Never => Ty::Never,
            TypeCstKind::Infer | TypeCstKind::Error => {
                let e = cx.record(Diagnostic::error(
                    cx.span(self.span),
                    "syntax error in type",
                ));
                Ty::Error(e)
            }
        }
    }
}

fn intrinsic_to_ty(intrinsic: Intrinsic) -> Ty<'static> {
    match intrinsic {
        Intrinsic::Bool => Ty::Bool,
        Intrinsic::Char => Ty::Char,
        Intrinsic::Str => Ty::Str,
        Intrinsic::Int(i) => Ty::Int(i),
        Intrinsic::Uint(u) => Ty::Uint(u),
        Intrinsic::Float(f) => Ty::Float(f),
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn check_literal_ty<'db>(cx: &InferCtx<'_, 'db>, lit: Literal<'db>) -> Ptr<Ty<'db>> {
    match lit {
        Literal::Bool(_) => cx.alloc_ty(Ty::Bool),
        Literal::Int(_) => cx.fresh_ty_var(),
        Literal::Float(_) => cx.fresh_ty_var(),
        Literal::String(_) => {
            let str_ty = cx.alloc_ty(Ty::Str);
            cx.alloc_ty(Ty::Ref(
                str_ty,
                Mutability::Shared,
                crate::ty::Lifetime::Static,
            ))
        }
        Literal::Char(_) => cx.alloc_ty(Ty::Char),
    }
}

fn res_to_ty<'db>(cx: &InferCtx<'_, 'db>, scope: &Scope<'db>, res: Res<'db>) -> Ptr<Ty<'db>> {
    match res {
        Res::Local(LocalId(id)) => scope.local_type(id),
        Res::Def(sym) => def_to_ty(cx, sym),
        Res::Error(e) => cx.alloc_ty(Ty::Error(e)),
    }
}

fn def_to_ty<'db>(cx: &InferCtx<'_, 'db>, sym: crate::symbol::Symbol<'db>) -> Ptr<Ty<'db>> {
    use crate::symbol::SymbolData;
    use crate::ty::BinderExt;

    match sym.data(cx.db) {
        SymbolData::FnSymbol(crate::symbol::FnSymbol::Local(local)) => {
            let sig = local.sig(cx.db);
            let sig_stash = sig.stash();

            let type_arg_ptrs: Vec<_> = sig.iter_symbols().map(|_| cx.fresh_ty_var()).collect();
            let type_args: Vec<_> = type_arg_ptrs.iter().map(|&ptr| cx.stash()[ptr]).collect();
            let instantiated = crate::ty_fold::instantiate_fn_sig(
                sig_stash,
                &mut *cx.stash_mut(),
                &sig.root(),
                type_args,
            );
            cx.alloc_ty(Ty::FnPtr(instantiated.params, instantiated.ret))
        }
        _ => cx.fresh_ty_var(),
    }
}

fn check_binary_op_ty<'db>(
    cx: &InferCtx<'_, 'db>,
    op: BinaryOp,
    lhs_ty: Ptr<Ty<'db>>,
    rhs_ty: Ptr<Ty<'db>>,
    span: RelativeSpan,
) -> Ptr<Ty<'db>> {
    cx.require_eq(rhs_ty, lhs_ty, span).record_err(cx);
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

use sage_stash::Slice;

struct StructLitResult<'db> {
    ty: Ptr<Ty<'db>>,
    local: Option<crate::local_syms::structs::LocalStructSym<'db>>,
    type_args: Slice<Ptr<Ty<'db>>>,
}

fn struct_lit_ty<'db>(cx: &InferCtx<'_, 'db>, res: Res<'db>) -> StructLitResult<'db> {
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
            let e = cx.record(Diagnostic::error(
                cx.span(RelativeSpan { start: 0, end: 0 }),
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
        SymbolData::VariantSymbol(crate::symbol::VariantSymbol::Local(local_variant)) => {
            let parent_enum = local_variant.parent_enum(cx.db);
            let enum_sym: crate::symbol::Symbol<'db> = parent_enum.into();
            let type_args_slice = cx.stash_mut().alloc_slice(&[]);
            StructLitResult {
                ty: cx.alloc_ty(Ty::Adt(enum_sym, type_args_slice)),
                local: None,
                type_args: type_args_slice,
            }
        }
        SymbolData::VariantSymbol(crate::symbol::VariantSymbol::Ext(_)) => {
            let type_args_slice = cx.stash_mut().alloc_slice(&[]);
            StructLitResult {
                ty: cx.alloc_ty(Ty::Adt(sym, type_args_slice)),
                local: None,
                type_args: type_args_slice,
            }
        }
        _ => {
            let e = cx.record(Diagnostic::error(
                cx.span(RelativeSpan { start: 0, end: 0 }),
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
    cx: &InferCtx<'_, 'db>,
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
                    let mut stash = cx.stash_mut();
                    field_sig.ty.stash_copy(fields_stash, &mut *stash)
                } else {
                    let mut stash = cx.stash_mut();
                    let mut folder = Substitute::new(fields_stash, &mut *stash, subst.clone());
                    let ty_data = folder.fold_ty(fields_stash[field_sig.ty]);
                    drop(folder);
                    stash.alloc(ty_data)
                };
                let init_ty = cx.stash()[field_init.value].ty;
                if let Err(e) = cx.require_coerce(init_ty, declared_ty, field_init.span) {
                    let e = e.with_context(ErrorContext::FieldInit {
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
    cx: &InferCtx<'_, 'db>,
    obj_ty_ptr: Ptr<Ty<'db>>,
    field_name: crate::name::Name<'db>,
) -> Ptr<Ty<'db>> {
    use crate::symbol::SymbolData;
    use crate::ty::BinderExt;
    use crate::ty_fold::{SubstTarget, Substitute, TyFolder};
    use rustc_hash::FxHashMap;

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
                    let mut stash = cx.stash_mut();
                    let mut folder = Substitute::new(fields_stash, &mut *stash, subst);
                    let field_ty_data = folder.fold_ty(fields_stash[field_sig.ty]);
                    drop(folder);
                    return stash.alloc(field_ty_data);
                }
            }
            cx.fresh_ty_var()
        }
        _ => cx.fresh_ty_var(),
    }
}

fn check_call_ty<'db>(
    cx: &InferCtx<'_, 'db>,
    callee_ty_ptr: Ptr<Ty<'db>>,
    arg_exprs: Slice<Ptr<TyExpr<'db>>>,
    call_span: RelativeSpan,
) -> Ptr<Ty<'db>> {
    let callee_ty = cx.stash()[callee_ty_ptr];

    match callee_ty {
        Ty::FnPtr(params, ret) => {
            let param_tys: Vec<_> = cx.stash()[params].to_vec();
            let arg_ptrs: Vec<_> = cx.stash()[arg_exprs].to_vec();
            for (param_ty, arg_expr) in param_tys.iter().zip(arg_ptrs.iter()) {
                let arg_ty = cx.stash()[*arg_expr].ty;
                let arg_span = cx.stash()[*arg_expr].span;
                cx.require_eq(arg_ty, *param_ty, arg_span).record_err(cx);
            }
            ret
        }
        _ => {
            let _ = call_span;
            cx.fresh_ty_var()
        }
    }
}
