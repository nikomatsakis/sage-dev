use sage_stash::{StashDirect, Stashed};

use crate::cst::structs::StructCst;
use crate::name::Name;
use crate::scope::ScopeSymbol;
use crate::span::AbsoluteSpan;
use crate::ty::BinderExt;
use crate::ty::{Binder, StructFields, StructSig};

#[salsa::tracked(debug)]
pub struct LocalStructSym<'db> {
    pub name: Name<'db>,
    pub scope: ScopeSymbol<'db>,

    #[returns(ref)]
    pub cst: StructCst<'db>,

    #[tracked]
    pub span: AbsoluteSpan<'db>,
}

impl StashDirect for LocalStructSym<'_> {}

impl<'db> LocalStructSym<'db> {
    pub fn attrs(self, db: &'db dyn crate::Db) -> (&'db sage_stash::Stash, &'db [crate::cst::attrs::AttrCst<'db>]) {
        let (stash, data) = self.cst(db).open_deref();
        (stash, &stash[data.attrs])
    }
}

#[salsa::tracked]
impl<'db> LocalStructSym<'db> {
    /// Computes the "signature" of a struct: its generics and where-clauses.
    ///
    /// Reads the CST's generic parameters, mints `GenericParam` symbols for
    /// each, and returns a `Binder` wrapping a (currently empty) `StructSig`.
    #[salsa::tracked]
    pub fn sig(self, db: &'db dyn crate::Db) -> Stashed<Binder<'db, StructSig<'db>>> {
        use crate::check::Check;
        use crate::cst::generics::CheckGenerics;
        use crate::resolve::Resolver;
        use crate::symbol::Symbol;

        let (src, cst) = self.cst(db).open_deref();
        let mut cx = Check::new(db, src, Resolver::new(db, self.scope(db)));

        let parent: Symbol<'db> = self.into();
        let generics = cst.generics.check(db, &mut cx, parent);

        // TODO: lower where-clauses into trait bounds on generics

        let struct_sig = StructSig {
            dummy: std::marker::PhantomData,
        };
        let binder = Binder::new(struct_sig, generics);
        cx.finish(binder)
    }

    /// Computes the fields of a struct.
    ///
    /// Calls `sig()` to get the generic parameter symbols, then resolves
    /// each field's type from the CST with those params in scope.
    #[salsa::tracked]
    pub fn fields(self, db: &'db dyn crate::Db) -> Stashed<StructFields<'db>> {
        use crate::check::Check;
        use crate::resolve::Resolver;
        use crate::ty::FieldSig;

        let (src, cst) = self.cst(db).open_deref();

        let mut cx = Check::new(db, src, Resolver::new(db, self.scope(db)));
        cx.resolver
            .ribs
            .add_generic_params(db, self.sig(db).iter_symbols());

        let field_sigs: Vec<_> = src[cst.fields]
            .iter()
            .map(|f| {
                let ty_val = cx.src[f.ty].check(&mut cx);
                let ty = cx.target_stash.alloc(ty_val);
                FieldSig { name: f.name, ty }
            })
            .collect();
        let fields = cx.target_stash.alloc_slice(&field_sigs);

        cx.finish(StructFields { fields })
    }
}
