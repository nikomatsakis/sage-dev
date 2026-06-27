use sage_stash::{StashDirect, Stashed};

use crate::cst::fns::FnCst;
use crate::name::Name;
use crate::scope::ScopeSymbol;
use crate::span::AbsoluteSpan;
use crate::ty::{Binder, FnSig};
use crate::tytree::CheckedBody;

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

impl StashDirect for LocalFnSym<'_> {}

impl<'db> LocalFnSym<'db> {
    pub fn attrs(
        self,
        db: &'db dyn crate::Db,
    ) -> (
        &'db sage_stash::Stash,
        &'db [crate::cst::attrs::AttrCst<'db>],
    ) {
        let (stash, data) = self.cst(db).open_deref();
        (stash, &stash[data.attrs])
    }
}

#[salsa::tracked]
impl<'db> LocalFnSym<'db> {
    /// Computes the signature: generics, parameter types, return type.
    #[salsa::tracked]
    pub fn sig(self, db: &'db dyn crate::Db) -> Stashed<Binder<'db, FnSig<'db>>> {
        use crate::check::Check;
        use crate::cst::generics::CheckGenerics;
        use crate::resolve::Resolver;
        use crate::symbol::Symbol;

        let (src, cst) = self.cst(db).open_deref();
        let mut cx = Check::new(db, src, Resolver::new(db, self.scope(db)));
        cx.current_sym = Some(crate::local_syms::LocalModItemSym::Function(self));

        let parent: Symbol<'db> = self.into();
        let generics = cst.generics.check(db, &mut cx, parent);

        let param_tys: Vec<_> = cx.source_stash[cst.params]
            .iter()
            .map(|p| {
                let ty = cx.source_stash[p.ty].check(&mut cx);
                cx.target_stash.alloc(ty)
            })
            .collect();
        let params = cx.target_stash.alloc_slice(&param_tys);

        let ret_ty = match cst.ret {
            Some(ret_ptr) => cx.source_stash[ret_ptr].check(&mut cx),
            None => {
                let unit = cx.target_stash.alloc_slice(&[]);
                crate::ty::Ty::Tuple(unit)
            }
        };
        let ret = cx.target_stash.alloc(ret_ty);

        let fn_sig = FnSig { params, ret };
        let binder = Binder::new(fn_sig, generics);
        cx.finish(binder)
    }

    /// Resolves and type-checks the function body in a single walk.
    #[salsa::tracked(returns(ref))]
    pub fn body(self, db: &'db dyn crate::Db) -> CheckedBody<'db> {
        use crate::check::infer_ctx::{ErrorContext, InferCtx, Scope};
        use crate::local_syms::LocalModItemSym;
        use crate::resolve::Resolver;
        use crate::ty::BinderExt;

        let sig = self.sig(db);
        let (src, cst) = self.cst(db).open_deref();

        let current_sym = LocalModItemSym::Function(self);
        let mut cx = InferCtx::new(db, src, Some(current_sym));
        let mut scope = Scope::new(Resolver::new(db, self.scope(db)));

        // Bring generics into scope.
        scope
            .resolver
            .ribs
            .add_generic_params(db, sig.iter_symbols());

        // Import the signature's param/return types into the body stash.
        let imported = cx.import_fn_sig(&sig);
        let ret_span = cst.ret.map(|r| src[r].span);
        cx.set_ret_ty(imported.ret, ret_span);

        // Bind function parameters as locals with their declared types.
        scope.bind_params(&cx, imported.params, cst.params);

        // Walk the body CST: resolve names + infer types → TyExpr.
        let body_expr = cx.block_on(async {
            let expr = match cst.body {
                Some(body_ptr) => src[body_ptr].check_with(&cx, &scope).await,
                None => {
                    let ty = cx.alloc_ty(crate::ty::Ty::Never);
                    cx.alloc_expr(crate::tytree::TyExprData::Missing, ty, cst.span)
                }
            };

            // Constrain body type against declared return type.
            let body_ty = cx.stash()[expr].ty;
            let body_span = cx.stash()[expr].span;
            if let Err(e) = cx.require_coerce(body_ty, imported.ret, body_span) {
                let e = if let Some(ret_ptr) = cst.ret {
                    let ret_span = src[ret_ptr].span;
                    e.with_context(ErrorContext::ReturnType { ret_span })
                } else {
                    e
                };
                cx.catch(e);
            }
            expr
        });

        // Resolve remaining inference variables.
        cx.finalize();
        cx.resolve_types();

        cx.finish(body_expr, cst.span)
    }
}
