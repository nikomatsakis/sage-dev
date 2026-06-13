use sage_stash::{AllocStashData, Ptr, Slice};

use crate::body::{BinaryOp, Literal, UnaryOp};
use crate::cst::paths::PathCst;
use crate::cst::ty::TypeCst;
use crate::name::Name;
use crate::span::RelativeSpan;
use crate::types::Mutability;

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
// Body checking: ExprCst/StmtCst/PatCst → Checked*
// ---------------------------------------------------------------------------

use crate::cst::check::BodyCheckCtx;
use crate::resolve::Namespace;
use crate::resolved::*;

impl<'db> ExprCst<'db> {
    pub(crate) fn check(&self, cx: &mut BodyCheckCtx<'_, 'db>) -> Ptr<CheckedExpr<'db>> {
        let kind = match &self.kind {
            ExprCstKind::Literal(lit) => CheckedExprKind::Literal(*lit),
            ExprCstKind::Path(path_ptr) => {
                let path = cx.src[*path_ptr];
                CheckedExprKind::Path(cx.resolve_path(path, Namespace::Value))
            }
            ExprCstKind::Block(stmts, tail) => {
                cx.ribs.push_scope();
                let rstmts: Vec<_> = cx.src[*stmts]
                    .iter()
                    .map(|s| s.check(cx))
                    .collect();
                let rtail = tail.map(|t| cx.src[t].check(cx));
                cx.ribs.pop_scope();
                CheckedExprKind::Block(cx.out.alloc_slice(&rstmts), rtail)
            }
            ExprCstKind::Call(func, args) => {
                let rf = cx.src[*func].check(cx);
                let rargs: Vec<_> = cx.src[*args]
                    .iter()
                    .map(|a| a.check_val(cx))
                    .collect();
                CheckedExprKind::Call(rf, cx.out.alloc_slice(&rargs))
            }
            ExprCstKind::MethodCall(obj, name, args) => {
                let ro = cx.src[*obj].check(cx);
                let rargs: Vec<_> = cx.src[*args]
                    .iter()
                    .map(|a| a.check_val(cx))
                    .collect();
                CheckedExprKind::MethodCall(ro, *name, cx.out.alloc_slice(&rargs))
            }
            ExprCstKind::Field(obj, name) => {
                CheckedExprKind::Field(cx.src[*obj].check(cx), *name)
            }
            ExprCstKind::Binary(lhs, op, rhs) => {
                let rl = cx.src[*lhs].check(cx);
                let rr = cx.src[*rhs].check(cx);
                CheckedExprKind::Binary(rl, *op, rr)
            }
            ExprCstKind::Unary(op, operand) => {
                CheckedExprKind::Unary(*op, cx.src[*operand].check(cx))
            }
            ExprCstKind::Ref(inner, m) => {
                CheckedExprKind::Ref(cx.src[*inner].check(cx), *m)
            }
            ExprCstKind::If(cond, then, else_) => {
                let rc = cx.src[*cond].check(cx);
                let rt = cx.src[*then].check(cx);
                let re = else_.map(|e| cx.src[e].check(cx));
                CheckedExprKind::If(rc, rt, re)
            }
            ExprCstKind::IfLet(pat, scrutinee, then, else_) => {
                let rs = cx.src[*scrutinee].check(cx);
                cx.ribs.push_scope();
                let rp = cx.src[*pat].check(cx);
                let rt = cx.src[*then].check(cx);
                cx.ribs.pop_scope();
                let re = else_.map(|e| cx.src[e].check(cx));
                CheckedExprKind::IfLet(rp, rs, rt, re)
            }
            ExprCstKind::Match(scrutinee, arms) => {
                let rs = cx.src[*scrutinee].check(cx);
                let rarms: Vec<_> = cx.src[*arms]
                    .iter()
                    .map(|arm| arm.check(cx))
                    .collect();
                CheckedExprKind::Match(rs, cx.out.alloc_slice(&rarms))
            }
            ExprCstKind::Loop(body) => {
                CheckedExprKind::Loop(cx.src[*body].check(cx))
            }
            ExprCstKind::While(cond, body) => {
                let rc = cx.src[*cond].check(cx);
                let rb = cx.src[*body].check(cx);
                CheckedExprKind::While(rc, rb)
            }
            ExprCstKind::WhileLet(pat, scrutinee, body) => {
                let rs = cx.src[*scrutinee].check(cx);
                cx.ribs.push_scope();
                let rp = cx.src[*pat].check(cx);
                let rb = cx.src[*body].check(cx);
                cx.ribs.pop_scope();
                CheckedExprKind::WhileLet(rp, rs, rb)
            }
            ExprCstKind::For(pat, iter, body) => {
                let ri = cx.src[*iter].check(cx);
                cx.ribs.push_scope();
                let rp = cx.src[*pat].check(cx);
                let rb = cx.src[*body].check(cx);
                cx.ribs.pop_scope();
                CheckedExprKind::For(rp, ri, rb)
            }
            ExprCstKind::Break(val) => {
                CheckedExprKind::Break(val.map(|v| cx.src[v].check(cx)))
            }
            ExprCstKind::Continue => CheckedExprKind::Continue,
            ExprCstKind::Return(val) => {
                CheckedExprKind::Return(val.map(|v| cx.src[v].check(cx)))
            }
            ExprCstKind::Assign(lhs, rhs) => {
                let rl = cx.src[*lhs].check(cx);
                let rr = cx.src[*rhs].check(cx);
                CheckedExprKind::Assign(rl, rr)
            }
            ExprCstKind::Await(inner) => CheckedExprKind::Await(cx.src[*inner].check(cx)),
            ExprCstKind::Try(inner) => CheckedExprKind::Try(cx.src[*inner].check(cx)),
            ExprCstKind::Closure(params, body) => {
                cx.ribs.push_scope();
                let rparams: Vec<_> = cx.src[*params]
                    .iter()
                    .map(|p| {
                        let rp = cx.src[p.pat].check(cx);
                        CheckedClosureParam {
                            pat: rp,
                            ty: None, // TODO: lower closure param type annotations
                            span: p.span,
                        }
                    })
                    .collect();
                let rb = cx.src[*body].check(cx);
                cx.ribs.pop_scope();
                CheckedExprKind::Closure(cx.out.alloc_slice(&rparams), rb)
            }
            ExprCstKind::Tuple(elems) => {
                let relems: Vec<_> = cx.src[*elems]
                    .iter()
                    .map(|e| e.check_val(cx))
                    .collect();
                CheckedExprKind::Tuple(cx.out.alloc_slice(&relems))
            }
            ExprCstKind::Array(elems) => {
                let relems: Vec<_> = cx.src[*elems]
                    .iter()
                    .map(|e| e.check_val(cx))
                    .collect();
                CheckedExprKind::Array(cx.out.alloc_slice(&relems))
            }
            ExprCstKind::Index(obj, idx) => {
                let ro = cx.src[*obj].check(cx);
                let ri = cx.src[*idx].check(cx);
                CheckedExprKind::Index(ro, ri)
            }
            ExprCstKind::Cast(expr, _ty) => {
                let re = cx.src[*expr].check(cx);
                // TODO: lower cast type annotation
                CheckedExprKind::Cast(re, todo!("cast type lowering"))
            }
            ExprCstKind::StructLit(path_ptr, fields) => {
                let path = cx.src[*path_ptr];
                let res = cx.resolve_path(path, Namespace::Type);
                let rfields: Vec<_> = cx.src[*fields]
                    .iter()
                    .map(|fi| CheckedFieldInit {
                        name: fi.name,
                        value: cx.src[fi.value].check(cx),
                        span: fi.span,
                    })
                    .collect();
                CheckedExprKind::StructLit(res, cx.out.alloc_slice(&rfields))
            }
            ExprCstKind::Range(lo, hi) => {
                let rl = lo.map(|l| cx.src[l].check(cx));
                let rh = hi.map(|h| cx.src[h].check(cx));
                CheckedExprKind::Range(rl, rh)
            }
            ExprCstKind::Missing => CheckedExprKind::Missing,
        };
        cx.out.alloc(CheckedExpr {
            kind,
            span: self.span,
        })
    }

    pub(crate) fn check_val(&self, cx: &mut BodyCheckCtx<'_, 'db>) -> CheckedExpr<'db> {
        let ptr = self.check(cx);
        cx.out[ptr]
    }
}

impl<'db> StmtCst<'db> {
    pub(crate) fn check(&self, cx: &mut BodyCheckCtx<'_, 'db>) -> CheckedStmt<'db> {
        match &self.kind {
            StmtCstKind::Let(pat, _ty, init) => {
                let rinit = init.map(|e| cx.src[e].check(cx));
                let rpat = cx.src[*pat].check(cx);
                // TODO: lower let type annotation
                CheckedStmt {
                    kind: CheckedStmtKind::Let(rpat, None, rinit),
                    span: self.span,
                }
            }
            StmtCstKind::Expr(e) => CheckedStmt {
                kind: CheckedStmtKind::Expr(cx.src[*e].check(cx)),
                span: self.span,
            },
        }
    }
}

impl<'db> PatCst<'db> {
    pub(crate) fn check(&self, cx: &mut BodyCheckCtx<'_, 'db>) -> Ptr<CheckedPat<'db>> {
        let kind = match &self.kind {
            PatCstKind::Wildcard => CheckedPatKind::Wildcard,
            PatCstKind::Bind(name, mutability) => {
                let id = cx.add_binding(*name, self.span);
                CheckedPatKind::Bind(id, *mutability)
            }
            PatCstKind::Path(path_ptr) => {
                let path = cx.src[*path_ptr];
                CheckedPatKind::Path(cx.resolve_path(path, Namespace::Value))
            }
            PatCstKind::Tuple(pats) => {
                let rpats: Vec<_> = cx.src[*pats]
                    .iter()
                    .map(|p| p.check_val(cx))
                    .collect();
                CheckedPatKind::Tuple(cx.out.alloc_slice(&rpats))
            }
            PatCstKind::Struct(path_ptr, fields) => {
                let path = cx.src[*path_ptr];
                let res = cx.resolve_path(path, Namespace::Type);
                let rfields: Vec<_> = cx.src[*fields]
                    .iter()
                    .map(|fp| CheckedFieldPat {
                        name: fp.name,
                        pat: cx.src[fp.pat].check(cx),
                        span: fp.span,
                    })
                    .collect();
                CheckedPatKind::Struct(res, cx.out.alloc_slice(&rfields))
            }
            PatCstKind::TupleStruct(path_ptr, pats) => {
                let path = cx.src[*path_ptr];
                let res = cx.resolve_path(path, Namespace::Value);
                let rpats: Vec<_> = cx.src[*pats]
                    .iter()
                    .map(|p| p.check_val(cx))
                    .collect();
                CheckedPatKind::TupleStruct(res, cx.out.alloc_slice(&rpats))
            }
            PatCstKind::Ref(inner, m) => {
                CheckedPatKind::Ref(cx.src[*inner].check(cx), *m)
            }
            PatCstKind::Literal(lit) => CheckedPatKind::Literal(*lit),
            PatCstKind::Or(pats) => {
                let rpats: Vec<_> = cx.src[*pats]
                    .iter()
                    .map(|p| p.check_val(cx))
                    .collect();
                CheckedPatKind::Or(cx.out.alloc_slice(&rpats))
            }
            PatCstKind::Rest => CheckedPatKind::Rest,
            PatCstKind::Missing => CheckedPatKind::Missing,
        };
        cx.out.alloc(CheckedPat {
            kind,
            span: self.span,
        })
    }

    pub(crate) fn check_val(&self, cx: &mut BodyCheckCtx<'_, 'db>) -> CheckedPat<'db> {
        let ptr = self.check(cx);
        cx.out[ptr]
    }
}

impl<'db> MatchArmCst<'db> {
    pub(crate) fn check(&self, cx: &mut BodyCheckCtx<'_, 'db>) -> CheckedMatchArm<'db> {
        cx.ribs.push_scope();
        let rp = cx.src[self.pat].check(cx);
        let rg = self.guard.map(|g| cx.src[g].check(cx));
        let rb = cx.src[self.body].check(cx);
        cx.ribs.pop_scope();
        CheckedMatchArm {
            pat: rp,
            guard: rg,
            body: rb,
            span: self.span,
        }
    }
}
