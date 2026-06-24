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
        use crate::check::BodyCheck;
        use crate::resolve::Resolver;
        use crate::ty::BinderExt;

        let sig = self.sig(db);
        let (src, cst) = self.cst(db).open_deref();

        let mut bx = BodyCheck::new(db, src, Resolver::new(db, self.scope(db)));

        // Bring generics into scope.
        bx.resolver.ribs.add_generic_params(db, sig.iter_symbols());

        // Import the signature's param/return types into the body stash.
        let imported = bx.import_fn_sig(&sig);

        // Bind function parameters as locals with their declared types.
        bx.bind_params(imported.params, cst.params);

        // Walk the body CST: resolve names + infer types → TyExpr.
        let body_expr = match cst.body {
            Some(body_ptr) => src[body_ptr].check(&mut bx),
            None => {
                let ty = bx.alloc_ty(crate::ty::Ty::Error);
                bx.alloc_expr(crate::tytree::TyExprData::Missing, ty, cst.span)
            }
        };

        // Constrain body type against declared return type.
        let body_ty = bx.stash()[body_expr].ty;
        bx.require_coerce(body_ty, imported.ret);

        // Resolve remaining inference variables.
        bx.finalize();
        bx.resolve_types();

        bx.finish(body_expr, cst.span)
    }
}
