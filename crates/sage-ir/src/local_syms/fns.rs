use sage_stash::Stashed;

use crate::cst::fns::FnCst;
use crate::name::Name;
use crate::scope::ScopeSymbol;
use crate::span::AbsoluteSpan;
use crate::ty::{Binder, FnSig};
use crate::typed_body::TypedBody;

#[salsa::tracked(debug)]
pub struct LocalFnSym<'db> {
    pub name: Name<'db>,
    pub scope: ScopeSymbol<'db>,

    #[tracked]
    #[returns(ref)]
    pub cst: FnCst<'db>,

    #[tracked]
    pub span: AbsoluteSpan<'db>,
}

#[salsa::tracked]
impl<'db> LocalFnSym<'db> {
    /// Computes the signature: generics, parameter types, return type.
    #[salsa::tracked]
    pub fn sig(self, db: &'db dyn crate::Db) -> Stashed<Binder<'db, FnSig<'db>>> {
        use crate::cst::generics::CheckGenerics;
        use crate::resolve::Resolver;
        use crate::sig_lower::CstLowerCtx;
        use crate::symbol::Symbol;

        let (src, cst) = self.cst(db).open_deref();
        let mut cx = CstLowerCtx::new(src, Resolver::new(db, self.scope(db)));

        let parent: Symbol<'db> = self.into();
        let generics = cst.generics.check(db, &mut cx, parent);

        let param_tys: Vec<_> = cx.src[cst.params]
            .iter()
            .map(|p| {
                let ty = cx.src[p.ty].check(&mut cx);
                cx.dst.alloc(ty)
            })
            .collect();
        let params = cx.dst.alloc_slice(&param_tys);

        let ret_ty = match cst.ret {
            Some(ret_ptr) => cx.src[ret_ptr].check(&mut cx),
            None => {
                let unit = cx.dst.alloc_slice(&[]);
                crate::ty::Ty {
                    data: crate::ty::TyData::Tuple(unit),
                }
            }
        };
        let ret = cx.dst.alloc(ret_ty);

        let fn_sig = FnSig { params, ret };
        let binder = Binder::new(fn_sig, generics);
        cx.finish(binder)
    }

    /// Resolves and type-checks the function body.
    #[salsa::tracked(returns(ref))]
    pub fn body(self, db: &'db dyn crate::Db) -> TypedBody<'db> {
        use crate::cst::check::BodyCheckCtx;
        use crate::infer::check::type_check_body;
        use crate::resolve::Resolver;
        use crate::ty::BinderExt;

        let (src, cst) = self.cst(db).open_deref();

        let mut bx = BodyCheckCtx::new(src, Resolver::new(db, self.scope(db)));

        // Bring generics into scope.
        bx.ribs.add_generic_params(db, self.sig(db).iter_symbols());

        // Bind function parameters as locals.
        for param in &src[cst.params] {
            if let Some(name) = param.name {
                bx.add_binding(name, param.span);
            }
        }

        // Resolve the body expression.
        let body_expr = match cst.body {
            Some(body_ptr) => bx.check_expr(&src[body_ptr]),
            None => {
                let missing = crate::resolved::CheckedExpr {
                    kind: crate::resolved::CheckedExprKind::Missing,
                    span: Default::default(),
                };
                bx.out.alloc(missing)
            }
        };

        let span = cst.span;
        let resolved = bx.finish(body_expr, span);

        // Run type inference.
        let sig = self.sig(db);
        let result = type_check_body(db, &resolved, &sig, self.scope(db));
        let errors = result.render_errors(db);

        TypedBody {
            body: resolved,
            errors,
        }
    }
}
