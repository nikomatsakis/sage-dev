use sage_stash::{Ptr, Stash, Stashed};

use crate::Db;
use crate::body::*;
use crate::item::FnAst;
use crate::module::ModSymbol;
use crate::name::Name;
use crate::resolve::{Namespace, Resolver, SourceRoot};
use crate::resolved::*;
use crate::ribs::{RibEntry, Ribs};
use crate::sig_ast::{PathAst, TypeRefAst, TypeRefAstKind};
use crate::span::RelativeSpan;

struct BodyResolver<'db> {
    resolver: Resolver<'db>,
    module: ModSymbol<'db>,
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
                        match self.resolver.resolve_segments(self.module, &names, ns) {
                            Ok(sym) => Res::Def(sym),
                            Err(_) => Res::Err,
                        }
                    }
                }
            };
        }

        // No rib hit — resolve via module-level resolution.
        let names: Vec<_> = segments.iter().map(|s| s.name).collect();
        match self.resolver.resolve_segments(self.module, &names, ns) {
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

    fn resolve_expr(&mut self, expr: &Expr<'db>) -> Ptr<RExpr<'db>> {
        let kind = match &expr.kind {
            ExprKind::Literal(lit) => RExprKind::Literal(*lit),
            ExprKind::Path(path) => RExprKind::Path(self.resolve_value_path(*path)),

            ExprKind::Block(stmts, tail) => {
                self.push_scope();
                let rstmts: Vec<_> = self.src[*stmts]
                    .iter()
                    .map(|s| self.resolve_stmt(s))
                    .collect();
                let rtail = tail.map(|t| self.resolve_expr(&self.src[t]));
                self.pop_scope();
                RExprKind::Block(self.out.alloc_slice(&rstmts), rtail)
            }
            ExprKind::Call(func, args) => {
                let rf = self.resolve_expr(&self.src[*func]);
                let rargs: Vec<_> = self.src[*args]
                    .iter()
                    .map(|a| self.resolve_expr_val(a))
                    .collect();
                RExprKind::Call(rf, self.out.alloc_slice(&rargs))
            }
            ExprKind::MethodCall(obj, name, args) => {
                let ro = self.resolve_expr(&self.src[*obj]);
                let rargs: Vec<_> = self.src[*args]
                    .iter()
                    .map(|a| self.resolve_expr_val(a))
                    .collect();
                RExprKind::MethodCall(ro, *name, self.out.alloc_slice(&rargs))
            }
            ExprKind::Field(obj, name) => {
                RExprKind::Field(self.resolve_expr(&self.src[*obj]), *name)
            }
            ExprKind::Binary(lhs, op, rhs) => {
                let rl = self.resolve_expr(&self.src[*lhs]);
                let rr = self.resolve_expr(&self.src[*rhs]);
                RExprKind::Binary(rl, *op, rr)
            }
            ExprKind::Unary(op, operand) => {
                RExprKind::Unary(*op, self.resolve_expr(&self.src[*operand]))
            }
            ExprKind::Ref(inner, m) => RExprKind::Ref(self.resolve_expr(&self.src[*inner]), *m),
            ExprKind::If(cond, then, else_) => {
                let rc = self.resolve_expr(&self.src[*cond]);
                let rt = self.resolve_expr(&self.src[*then]);
                let re = else_.map(|e| self.resolve_expr(&self.src[e]));
                RExprKind::If(rc, rt, re)
            }
            ExprKind::IfLet(pat, scrutinee, then, else_) => {
                let rs = self.resolve_expr(&self.src[*scrutinee]);
                self.push_scope();
                let rp = self.resolve_pat(&self.src[*pat]);
                let rt = self.resolve_expr(&self.src[*then]);
                self.pop_scope();
                let re = else_.map(|e| self.resolve_expr(&self.src[e]));
                RExprKind::IfLet(rp, rs, rt, re)
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
                        RMatchArm {
                            pat: rp,
                            guard: rg,
                            body: rb,
                            span: arm.span,
                        }
                    })
                    .collect();
                RExprKind::Match(rs, self.out.alloc_slice(&rarms))
            }
            ExprKind::Loop(body) => RExprKind::Loop(self.resolve_expr(&self.src[*body])),
            ExprKind::While(cond, body) => {
                let rc = self.resolve_expr(&self.src[*cond]);
                let rb = self.resolve_expr(&self.src[*body]);
                RExprKind::While(rc, rb)
            }
            ExprKind::WhileLet(pat, scrutinee, body) => {
                let rs = self.resolve_expr(&self.src[*scrutinee]);
                self.push_scope();
                let rp = self.resolve_pat(&self.src[*pat]);
                let rb = self.resolve_expr(&self.src[*body]);
                self.pop_scope();
                RExprKind::WhileLet(rp, rs, rb)
            }
            ExprKind::For(pat, iter, body) => {
                let ri = self.resolve_expr(&self.src[*iter]);
                self.push_scope();
                let rp = self.resolve_pat(&self.src[*pat]);
                let rb = self.resolve_expr(&self.src[*body]);
                self.pop_scope();
                RExprKind::For(rp, ri, rb)
            }
            ExprKind::Break(val) => RExprKind::Break(val.map(|v| self.resolve_expr(&self.src[v]))),
            ExprKind::Continue => RExprKind::Continue,
            ExprKind::Return(val) => {
                RExprKind::Return(val.map(|v| self.resolve_expr(&self.src[v])))
            }
            ExprKind::Assign(lhs, rhs) => {
                let rl = self.resolve_expr(&self.src[*lhs]);
                let rr = self.resolve_expr(&self.src[*rhs]);
                RExprKind::Assign(rl, rr)
            }
            ExprKind::Await(inner) => RExprKind::Await(self.resolve_expr(&self.src[*inner])),
            ExprKind::Try(inner) => RExprKind::Try(self.resolve_expr(&self.src[*inner])),
            ExprKind::Closure(params, body) => {
                self.push_scope();
                let rparams: Vec<_> = self.src[*params]
                    .iter()
                    .map(|p| {
                        let rp = self.resolve_pat(&self.src[p.pat]);
                        let rty = p.ty.map(|t| self.copy_type_ref(t));
                        RClosureParam {
                            pat: rp,
                            ty: rty,
                            span: p.span,
                        }
                    })
                    .collect();
                let rb = self.resolve_expr(&self.src[*body]);
                self.pop_scope();
                RExprKind::Closure(self.out.alloc_slice(&rparams), rb)
            }
            ExprKind::Tuple(elems) => {
                let relems: Vec<_> = self.src[*elems]
                    .iter()
                    .map(|e| self.resolve_expr_val(e))
                    .collect();
                RExprKind::Tuple(self.out.alloc_slice(&relems))
            }
            ExprKind::Array(elems) => {
                let relems: Vec<_> = self.src[*elems]
                    .iter()
                    .map(|e| self.resolve_expr_val(e))
                    .collect();
                RExprKind::Array(self.out.alloc_slice(&relems))
            }
            ExprKind::Index(obj, idx) => {
                let ro = self.resolve_expr(&self.src[*obj]);
                let ri = self.resolve_expr(&self.src[*idx]);
                RExprKind::Index(ro, ri)
            }
            ExprKind::Cast(expr, ty) => {
                let rty = self.copy_type_ref(*ty);
                RExprKind::Cast(self.resolve_expr(&self.src[*expr]), rty)
            }
            ExprKind::StructLit(path, fields) => {
                let res = self.resolve_type_path(*path);
                let rfields: Vec<_> = self.src[*fields]
                    .iter()
                    .map(|fi| RFieldInit {
                        name: fi.name,
                        value: self.resolve_expr(&self.src[fi.value]),
                        span: fi.span,
                    })
                    .collect();
                RExprKind::StructLit(res, self.out.alloc_slice(&rfields))
            }
            ExprKind::Range(lo, hi) => {
                let rl = lo.map(|l| self.resolve_expr(&self.src[l]));
                let rh = hi.map(|h| self.resolve_expr(&self.src[h]));
                RExprKind::Range(rl, rh)
            }
            ExprKind::MacroCall(path, tt) => {
                let res = self.resolve_macro_path(*path);
                RExprKind::MacroCall(res, *tt)
            }
            ExprKind::Missing => RExprKind::Missing,
        };
        self.out.alloc(RExpr {
            kind,
            span: expr.span,
        })
    }

    /// Resolve an expression value (not behind a Ptr — used for slice elements).
    fn resolve_expr_val(&mut self, expr: &Expr<'db>) -> RExpr<'db> {
        let ptr = self.resolve_expr(expr);
        self.out[ptr]
    }

    // -- statement resolution --

    fn resolve_stmt(&mut self, stmt: &Stmt<'db>) -> RStmt<'db> {
        match &stmt.kind {
            StmtKind::Let(pat, ty, init) => {
                let rinit = init.map(|e| self.resolve_expr(&self.src[e]));
                let rpat = self.resolve_pat(&self.src[*pat]);
                let rty = ty.map(|t| self.copy_type_ref(t));
                RStmt {
                    kind: RStmtKind::Let(rpat, rty, rinit),
                    span: stmt.span,
                }
            }
            StmtKind::Expr(e) => RStmt {
                kind: RStmtKind::Expr(self.resolve_expr(&self.src[*e])),
                span: stmt.span,
            },
        }
    }

    // -- pattern resolution --

    fn resolve_pat(&mut self, pat: &Pat<'db>) -> Ptr<RPat<'db>> {
        let kind = match &pat.kind {
            PatKind::Wildcard => RPatKind::Wildcard,
            PatKind::Bind(name, mutability) => {
                let id = self.add_binding(*name, pat.span);
                RPatKind::Bind(id, *mutability)
            }
            PatKind::Path(path) => RPatKind::Path(self.resolve_value_path(*path)),
            PatKind::Tuple(pats) => {
                let rpats: Vec<_> = self.src[*pats]
                    .iter()
                    .map(|p| self.resolve_pat_val(p))
                    .collect();
                RPatKind::Tuple(self.out.alloc_slice(&rpats))
            }
            PatKind::Struct(path, fields) => {
                let res = self.resolve_type_path(*path);
                let rfields: Vec<_> = self.src[*fields]
                    .iter()
                    .map(|fp| RFieldPat {
                        name: fp.name,
                        pat: self.resolve_pat(&self.src[fp.pat]),
                        span: fp.span,
                    })
                    .collect();
                RPatKind::Struct(res, self.out.alloc_slice(&rfields))
            }
            PatKind::TupleStruct(path, pats) => {
                let res = self.resolve_value_path(*path);
                let rpats: Vec<_> = self.src[*pats]
                    .iter()
                    .map(|p| self.resolve_pat_val(p))
                    .collect();
                RPatKind::TupleStruct(res, self.out.alloc_slice(&rpats))
            }
            PatKind::Ref(inner, m) => RPatKind::Ref(self.resolve_pat(&self.src[*inner]), *m),
            PatKind::Literal(lit) => RPatKind::Literal(*lit),
            PatKind::Or(pats) => {
                let rpats: Vec<_> = self.src[*pats]
                    .iter()
                    .map(|p| self.resolve_pat_val(p))
                    .collect();
                RPatKind::Or(self.out.alloc_slice(&rpats))
            }
            PatKind::Rest => RPatKind::Rest,
            PatKind::Missing => RPatKind::Missing,
        };
        self.out.alloc(RPat {
            kind,
            span: pat.span,
        })
    }

    fn resolve_pat_val(&mut self, pat: &Pat<'db>) -> RPat<'db> {
        let ptr = self.resolve_pat(pat);
        self.out[ptr]
    }
}

/// Produce a resolved body for a function.
#[salsa::tracked(returns(ref))]
pub fn resolve_body<'db>(
    db: &'db dyn Db,
    function: FnAst<'db>,
    module: ModSymbol<'db>,
    source_root: SourceRoot,
) -> ResolvedBody<'db> {
    let body = function.body(db);
    let src_stash = body.stash();
    let body_data = &src_stash[*body.root()];
    let root_expr = &src_stash[body_data.root];

    let mut resolver = BodyResolver {
        resolver: Resolver::new(db, source_root),
        module,
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
    let rbody = resolver.out.alloc(RBody {
        root: resolved_root,
        locals,
        span: body_data.span,
    });

    resolver.pop_scope();
    Stashed::new(resolver.out, rbody)
}
