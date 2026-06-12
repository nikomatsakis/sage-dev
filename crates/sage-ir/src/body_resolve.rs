use sage_stash::{Ptr, Stash, Stashed};

use crate::Db;
use crate::body::*;
use crate::item::FnAst;
use crate::name::Name;
use crate::resolve::{Namespace, Resolver};
use crate::resolved::*;
use crate::ribs::{RibEntry, Ribs};
use crate::scope::ScopeSymbol;
use crate::sig_ast::{PathAst, TypeRefAst, TypeRefAstKind};
use crate::span::RelativeSpan;

struct BodyResolver<'db> {
    resolver: Resolver<'db>,
    src: &'db Stash,
    out: Stash,
    locals: Vec<LocalVar<'db>>,
    ribs: Ribs<'db>,
}

impl<'db> BodyResolver<'db> {
    // -- scope operations --

    fn push_scope(&mut self) {
        self.ribs.push_scope();
    }

    fn pop_scope(&mut self) {
        self.ribs.pop_scope();
    }

    fn add_binding(&mut self, name: Name<'db>, span: RelativeSpan) -> LocalId {
        let id = LocalId(self.locals.len() as u32);
        self.locals.push(LocalVar { name, span });
        self.ribs.add(name, Namespace::Value, RibEntry::Local(id));
        id
    }

    // -- path resolution --

    fn resolve_value_path(&mut self, path_ptr: Ptr<PathAst<'db>>) -> Res<'db> {
        self.resolve_path_ast(path_ptr, Namespace::Value)
    }

    fn resolve_type_path(&mut self, path_ptr: Ptr<PathAst<'db>>) -> Res<'db> {
        self.resolve_path_ast(path_ptr, Namespace::Type)
    }

    fn resolve_macro_path(&mut self, path_ptr: Ptr<PathAst<'db>>) -> Res<'db> {
        self.resolve_path_ast(path_ptr, Namespace::Macro(crate::resolve::MacroKind::Bang))
    }

    fn resolve_path_ast(&mut self, path_ptr: Ptr<PathAst<'db>>, ns: Namespace) -> Res<'db> {
        let path = &self.src[path_ptr];
        let segments = &self.src[path.segments];
        if segments.is_empty() {
            return Res::Err;
        }

        let first = segments[0].name;
        let rest = &segments[1..];

        // Check ribs for the first segment.
        if let Some(entry) = self.ribs.lookup(first, ns) {
            return match entry {
                RibEntry::Local(id) => {
                    if rest.is_empty() {
                        Res::Local(id)
                    } else {
                        Res::Err
                    }
                }
                RibEntry::Param(_) | RibEntry::SelfTy(_) => {
                    // TODO: generic params / Self in body expressions
                    Res::Err
                }
                RibEntry::Sym(sym) => {
                    if rest.is_empty() {
                        Res::Def(sym)
                    } else {
                        let names: Vec<_> = segments.iter().map(|s| s.name).collect();
                        match self.resolver.resolve_segments(&names, ns) {
                            Ok(sym) => Res::Def(sym),
                            Err(_) => Res::Err,
                        }
                    }
                }
            };
        }

        // No rib hit — resolve via module-level resolution.
        let names: Vec<_> = segments.iter().map(|s| s.name).collect();
        match self.resolver.resolve_segments(&names, ns) {
            Ok(sym) => Res::Def(sym),
            Err(_) => Res::Err,
        }
    }

    // -- type ref deep copy (src stash → out stash) --

    fn copy_type_ref(&mut self, ty: Ptr<TypeRefAst<'db>>) -> Ptr<TypeRefAst<'db>> {
        let ty_data = self.src[ty];
        let kind = match ty_data.kind {
            TypeRefAstKind::Path(path) => TypeRefAstKind::Path(self.copy_path(path)),
            TypeRefAstKind::Reference(inner, m) => {
                TypeRefAstKind::Reference(self.copy_type_ref(inner), m)
            }
            TypeRefAstKind::Slice(inner) => TypeRefAstKind::Slice(self.copy_type_ref(inner)),
            TypeRefAstKind::Array(inner) => TypeRefAstKind::Array(self.copy_type_ref(inner)),
            TypeRefAstKind::Tuple(elems) => {
                let copied: Vec<_> = self.src[elems]
                    .iter()
                    .map(|e| {
                        let ptr = self.copy_type_ref_val(*e);
                        self.out[ptr]
                    })
                    .collect();
                TypeRefAstKind::Tuple(self.out.alloc_slice(&copied))
            }
            TypeRefAstKind::Never => TypeRefAstKind::Never,
            TypeRefAstKind::Infer => TypeRefAstKind::Infer,
            TypeRefAstKind::Error => TypeRefAstKind::Error,
        };
        self.out.alloc(TypeRefAst {
            kind,
            span: ty_data.span,
        })
    }

    fn copy_type_ref_val(&mut self, ty: TypeRefAst<'db>) -> Ptr<TypeRefAst<'db>> {
        let kind = match ty.kind {
            TypeRefAstKind::Path(path) => TypeRefAstKind::Path(self.copy_path(path)),
            TypeRefAstKind::Reference(inner, m) => {
                TypeRefAstKind::Reference(self.copy_type_ref(inner), m)
            }
            TypeRefAstKind::Slice(inner) => TypeRefAstKind::Slice(self.copy_type_ref(inner)),
            TypeRefAstKind::Array(inner) => TypeRefAstKind::Array(self.copy_type_ref(inner)),
            TypeRefAstKind::Tuple(elems) => {
                let copied: Vec<_> = self.src[elems]
                    .iter()
                    .map(|e| {
                        let ptr = self.copy_type_ref_val(*e);
                        self.out[ptr]
                    })
                    .collect();
                TypeRefAstKind::Tuple(self.out.alloc_slice(&copied))
            }
            TypeRefAstKind::Never => TypeRefAstKind::Never,
            TypeRefAstKind::Infer => TypeRefAstKind::Infer,
            TypeRefAstKind::Error => TypeRefAstKind::Error,
        };
        self.out.alloc(TypeRefAst {
            kind,
            span: ty.span,
        })
    }

    fn copy_path(&mut self, path: Ptr<PathAst<'db>>) -> Ptr<PathAst<'db>> {
        let path_data = self.src[path];
        let segs: Vec<_> = self.src[path_data.segments]
            .iter()
            .map(|seg| {
                let type_args: Vec<_> = self.src[seg.type_args]
                    .iter()
                    .map(|a| {
                        let ptr = self.copy_type_ref_val(*a);
                        self.out[ptr]
                    })
                    .collect();
                crate::sig_ast::PathSegmentAst {
                    name: seg.name,
                    type_args: self.out.alloc_slice(&type_args),
                    span: seg.span,
                }
            })
            .collect();
        let segs = self.out.alloc_slice(&segs);
        self.out.alloc(PathAst {
            segments: segs,
            span: path_data.span,
        })
    }

    // -- expression resolution --

    fn resolve_expr(&mut self, expr: &Expr<'db>) -> Ptr<CheckedExpr<'db>> {
        let kind = match &expr.kind {
            ExprKind::Literal(lit) => CheckedExprKind::Literal(*lit),
            ExprKind::Path(path) => CheckedExprKind::Path(self.resolve_value_path(*path)),

            ExprKind::Block(stmts, tail) => {
                self.push_scope();
                let rstmts: Vec<_> = self.src[*stmts]
                    .iter()
                    .map(|s| self.resolve_stmt(s))
                    .collect();
                let rtail = tail.map(|t| self.resolve_expr(&self.src[t]));
                self.pop_scope();
                CheckedExprKind::Block(self.out.alloc_slice(&rstmts), rtail)
            }
            ExprKind::Call(func, args) => {
                let rf = self.resolve_expr(&self.src[*func]);
                let rargs: Vec<_> = self.src[*args]
                    .iter()
                    .map(|a| self.resolve_expr_val(a))
                    .collect();
                CheckedExprKind::Call(rf, self.out.alloc_slice(&rargs))
            }
            ExprKind::MethodCall(obj, name, args) => {
                let ro = self.resolve_expr(&self.src[*obj]);
                let rargs: Vec<_> = self.src[*args]
                    .iter()
                    .map(|a| self.resolve_expr_val(a))
                    .collect();
                CheckedExprKind::MethodCall(ro, *name, self.out.alloc_slice(&rargs))
            }
            ExprKind::Field(obj, name) => {
                CheckedExprKind::Field(self.resolve_expr(&self.src[*obj]), *name)
            }
            ExprKind::Binary(lhs, op, rhs) => {
                let rl = self.resolve_expr(&self.src[*lhs]);
                let rr = self.resolve_expr(&self.src[*rhs]);
                CheckedExprKind::Binary(rl, *op, rr)
            }
            ExprKind::Unary(op, operand) => {
                CheckedExprKind::Unary(*op, self.resolve_expr(&self.src[*operand]))
            }
            ExprKind::Ref(inner, m) => {
                CheckedExprKind::Ref(self.resolve_expr(&self.src[*inner]), *m)
            }
            ExprKind::If(cond, then, else_) => {
                let rc = self.resolve_expr(&self.src[*cond]);
                let rt = self.resolve_expr(&self.src[*then]);
                let re = else_.map(|e| self.resolve_expr(&self.src[e]));
                CheckedExprKind::If(rc, rt, re)
            }
            ExprKind::IfLet(pat, scrutinee, then, else_) => {
                let rs = self.resolve_expr(&self.src[*scrutinee]);
                self.push_scope();
                let rp = self.resolve_pat(&self.src[*pat]);
                let rt = self.resolve_expr(&self.src[*then]);
                self.pop_scope();
                let re = else_.map(|e| self.resolve_expr(&self.src[e]));
                CheckedExprKind::IfLet(rp, rs, rt, re)
            }
            ExprKind::Match(scrutinee, arms) => {
                let rs = self.resolve_expr(&self.src[*scrutinee]);
                let rarms: Vec<_> = self.src[*arms]
                    .iter()
                    .map(|arm| {
                        self.push_scope();
                        let rp = self.resolve_pat(&self.src[arm.pat]);
                        let rg = arm.guard.map(|g| self.resolve_expr(&self.src[g]));
                        let rb = self.resolve_expr(&self.src[arm.body]);
                        self.pop_scope();
                        CheckedMatchArm {
                            pat: rp,
                            guard: rg,
                            body: rb,
                            span: arm.span,
                        }
                    })
                    .collect();
                CheckedExprKind::Match(rs, self.out.alloc_slice(&rarms))
            }
            ExprKind::Loop(body) => CheckedExprKind::Loop(self.resolve_expr(&self.src[*body])),
            ExprKind::While(cond, body) => {
                let rc = self.resolve_expr(&self.src[*cond]);
                let rb = self.resolve_expr(&self.src[*body]);
                CheckedExprKind::While(rc, rb)
            }
            ExprKind::WhileLet(pat, scrutinee, body) => {
                let rs = self.resolve_expr(&self.src[*scrutinee]);
                self.push_scope();
                let rp = self.resolve_pat(&self.src[*pat]);
                let rb = self.resolve_expr(&self.src[*body]);
                self.pop_scope();
                CheckedExprKind::WhileLet(rp, rs, rb)
            }
            ExprKind::For(pat, iter, body) => {
                let ri = self.resolve_expr(&self.src[*iter]);
                self.push_scope();
                let rp = self.resolve_pat(&self.src[*pat]);
                let rb = self.resolve_expr(&self.src[*body]);
                self.pop_scope();
                CheckedExprKind::For(rp, ri, rb)
            }
            ExprKind::Break(val) => {
                CheckedExprKind::Break(val.map(|v| self.resolve_expr(&self.src[v])))
            }
            ExprKind::Continue => CheckedExprKind::Continue,
            ExprKind::Return(val) => {
                CheckedExprKind::Return(val.map(|v| self.resolve_expr(&self.src[v])))
            }
            ExprKind::Assign(lhs, rhs) => {
                let rl = self.resolve_expr(&self.src[*lhs]);
                let rr = self.resolve_expr(&self.src[*rhs]);
                CheckedExprKind::Assign(rl, rr)
            }
            ExprKind::Await(inner) => CheckedExprKind::Await(self.resolve_expr(&self.src[*inner])),
            ExprKind::Try(inner) => CheckedExprKind::Try(self.resolve_expr(&self.src[*inner])),
            ExprKind::Closure(params, body) => {
                self.push_scope();
                let rparams: Vec<_> = self.src[*params]
                    .iter()
                    .map(|p| {
                        let rp = self.resolve_pat(&self.src[p.pat]);
                        let rty = p.ty.map(|t| self.copy_type_ref(t));
                        CheckedClosureParam {
                            pat: rp,
                            ty: rty,
                            span: p.span,
                        }
                    })
                    .collect();
                let rb = self.resolve_expr(&self.src[*body]);
                self.pop_scope();
                CheckedExprKind::Closure(self.out.alloc_slice(&rparams), rb)
            }
            ExprKind::Tuple(elems) => {
                let relems: Vec<_> = self.src[*elems]
                    .iter()
                    .map(|e| self.resolve_expr_val(e))
                    .collect();
                CheckedExprKind::Tuple(self.out.alloc_slice(&relems))
            }
            ExprKind::Array(elems) => {
                let relems: Vec<_> = self.src[*elems]
                    .iter()
                    .map(|e| self.resolve_expr_val(e))
                    .collect();
                CheckedExprKind::Array(self.out.alloc_slice(&relems))
            }
            ExprKind::Index(obj, idx) => {
                let ro = self.resolve_expr(&self.src[*obj]);
                let ri = self.resolve_expr(&self.src[*idx]);
                CheckedExprKind::Index(ro, ri)
            }
            ExprKind::Cast(expr, ty) => {
                let rty = self.copy_type_ref(*ty);
                CheckedExprKind::Cast(self.resolve_expr(&self.src[*expr]), rty)
            }
            ExprKind::StructLit(path, fields) => {
                let res = self.resolve_type_path(*path);
                let rfields: Vec<_> = self.src[*fields]
                    .iter()
                    .map(|fi| CheckedFieldInit {
                        name: fi.name,
                        value: self.resolve_expr(&self.src[fi.value]),
                        span: fi.span,
                    })
                    .collect();
                CheckedExprKind::StructLit(res, self.out.alloc_slice(&rfields))
            }
            ExprKind::Range(lo, hi) => {
                let rl = lo.map(|l| self.resolve_expr(&self.src[l]));
                let rh = hi.map(|h| self.resolve_expr(&self.src[h]));
                CheckedExprKind::Range(rl, rh)
            }
            ExprKind::MacroCall(path, tt) => {
                let res = self.resolve_macro_path(*path);
                CheckedExprKind::MacroCall(res, *tt)
            }
            ExprKind::Missing => CheckedExprKind::Missing,
        };
        self.out.alloc(CheckedExpr {
            kind,
            span: expr.span,
        })
    }

    /// Resolve an expression value (not behind a Ptr — used for slice elements).
    fn resolve_expr_val(&mut self, expr: &Expr<'db>) -> CheckedExpr<'db> {
        let ptr = self.resolve_expr(expr);
        self.out[ptr]
    }

    // -- statement resolution --

    fn resolve_stmt(&mut self, stmt: &Stmt<'db>) -> CheckedStmt<'db> {
        match &stmt.kind {
            StmtKind::Let(pat, ty, init) => {
                let rinit = init.map(|e| self.resolve_expr(&self.src[e]));
                let rpat = self.resolve_pat(&self.src[*pat]);
                let rty = ty.map(|t| self.copy_type_ref(t));
                CheckedStmt {
                    kind: CheckedStmtKind::Let(rpat, rty, rinit),
                    span: stmt.span,
                }
            }
            StmtKind::Expr(e) => CheckedStmt {
                kind: CheckedStmtKind::Expr(self.resolve_expr(&self.src[*e])),
                span: stmt.span,
            },
        }
    }

    // -- pattern resolution --

    fn resolve_pat(&mut self, pat: &Pat<'db>) -> Ptr<CheckedPat<'db>> {
        let kind = match &pat.kind {
            PatKind::Wildcard => CheckedPatKind::Wildcard,
            PatKind::Bind(name, mutability) => {
                let id = self.add_binding(*name, pat.span);
                CheckedPatKind::Bind(id, *mutability)
            }
            PatKind::Path(path) => CheckedPatKind::Path(self.resolve_value_path(*path)),
            PatKind::Tuple(pats) => {
                let rpats: Vec<_> = self.src[*pats]
                    .iter()
                    .map(|p| self.resolve_pat_val(p))
                    .collect();
                CheckedPatKind::Tuple(self.out.alloc_slice(&rpats))
            }
            PatKind::Struct(path, fields) => {
                let res = self.resolve_type_path(*path);
                let rfields: Vec<_> = self.src[*fields]
                    .iter()
                    .map(|fp| CheckedFieldPat {
                        name: fp.name,
                        pat: self.resolve_pat(&self.src[fp.pat]),
                        span: fp.span,
                    })
                    .collect();
                CheckedPatKind::Struct(res, self.out.alloc_slice(&rfields))
            }
            PatKind::TupleStruct(path, pats) => {
                let res = self.resolve_value_path(*path);
                let rpats: Vec<_> = self.src[*pats]
                    .iter()
                    .map(|p| self.resolve_pat_val(p))
                    .collect();
                CheckedPatKind::TupleStruct(res, self.out.alloc_slice(&rpats))
            }
            PatKind::Ref(inner, m) => CheckedPatKind::Ref(self.resolve_pat(&self.src[*inner]), *m),
            PatKind::Literal(lit) => CheckedPatKind::Literal(*lit),
            PatKind::Or(pats) => {
                let rpats: Vec<_> = self.src[*pats]
                    .iter()
                    .map(|p| self.resolve_pat_val(p))
                    .collect();
                CheckedPatKind::Or(self.out.alloc_slice(&rpats))
            }
            PatKind::Rest => CheckedPatKind::Rest,
            PatKind::Missing => CheckedPatKind::Missing,
        };
        self.out.alloc(CheckedPat {
            kind,
            span: pat.span,
        })
    }

    fn resolve_pat_val(&mut self, pat: &Pat<'db>) -> CheckedPat<'db> {
        let ptr = self.resolve_pat(pat);
        self.out[ptr]
    }
}

/// Produce a resolved body for a function.
pub fn resolve_body<'db>(
    db: &'db dyn Db,
    function: FnAst<'db>,
    scope: ScopeSymbol<'db>,
) -> ResolvedBody<'db> {
    let body = function.body(db);
    let src_stash = body.stash();
    let body_data = &src_stash[*body.root()];
    let root_expr = &src_stash[body_data.root];

    let mut resolver = BodyResolver {
        resolver: Resolver::new(db, scope),
        src: src_stash,
        out: Stash::new(),
        locals: Vec::new(),
        ribs: Ribs::new(),
    };

    // Push function params as the outermost scope.
    resolver.push_scope();
    let sig = function.signature(db);
    let sig_stash = sig.stash();
    let sig_data = &sig_stash[*sig.root()];
    for param in &sig_stash[sig_data.params] {
        if let Some(name) = param.name {
            resolver.add_binding(name, param.span);
        }
    }

    let resolved_root = resolver.resolve_expr(root_expr);
    let locals = resolver.out.alloc_slice(&resolver.locals);
    let rbody = resolver.out.alloc(CheckedBody {
        root: resolved_root,
        locals,
        span: body_data.span,
    });

    resolver.pop_scope();
    Stashed::new(resolver.out, rbody)
}
