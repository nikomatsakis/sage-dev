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
impl<'db> LocalStructSym<'db> {
    /// Computes the "signature" of a struct: its generics and where-clauses.
    ///
    /// Reads the CST's generic parameters, mints `GenericParam` symbols for
    /// each, and returns the checked parameter environment under their binder.
    // ANCHOR: example_struct_sig
    #[salsa::tracked]
    pub fn sig(self, db: &'db dyn crate::Db) -> Stashed<Binder<'db, StructSig<'db>>> {
        use crate::check::Check;
        use crate::cst::generics::CheckGenerics;
        use crate::resolve::Resolver;
        use crate::symbol::Symbol;

        let (src, cst) = self.cst(db).open_deref();
        let mut cx = Check::new(db, src, Resolver::new(db, self.scope(db)));
        cx.current_sym = Some(crate::local_syms::LocalModItemSym::Struct(self));

        let parent: Symbol<'db> = self.into();
        let generics = cst.generics.check(db, &mut cx, parent);

        let (where_clauses, solver_eligibility) = crate::check::trait_env::lower_predicates(
            &mut cx,
            cst.generics,
            generics,
            cst.where_clauses,
        );
        let parameter_env = cx.complete_parameter_env(where_clauses, solver_eligibility);
        let struct_sig = StructSig { parameter_env };
        let binder = Binder::new(struct_sig, generics);
        cx.finish(binder)
    }
    // ANCHOR_END: example_struct_sig

    /// Computes the fields of a struct.
    ///
    /// Calls `sig()` to get the generic parameter symbols, then resolves
    /// each field's type from the CST with those params in scope.
    // ANCHOR: example_struct_fields
    #[salsa::tracked]
    pub fn fields(self, db: &'db dyn crate::Db) -> Stashed<StructFields<'db>> {
        use crate::check::Check;
        use crate::resolve::Resolver;
        use crate::ty::FieldSig;

        let (src, cst) = self.cst(db).open_deref();

        let mut cx = Check::new(db, src, Resolver::new(db, self.scope(db)));
        cx.current_sym = Some(crate::local_syms::LocalModItemSym::Struct(self));
        cx.resolver
            .ribs
            .add_generic_params(db, self.sig(db).iter_symbols());

        let field_sigs: Vec<_> = src[cst.fields]
            .iter()
            .map(|f| {
                let ty_val = cx.source_stash[f.ty].check(&mut cx);
                let ty = cx.target_stash.alloc(ty_val);
                FieldSig { name: f.name, ty }
            })
            .collect();
        let fields = cx.target_stash.alloc_slice(&field_sigs);
        let no_declared_predicates = cx.target_stash.alloc_slice(&[]);
        let parameter_env = cx.complete_parameter_env(
            no_declared_predicates,
            crate::ty::SolverEligibility::Eligible,
        );

        cx.finish(StructFields {
            fields,
            parameter_env,
        })
    }
    // ANCHOR_END: example_struct_fields
}
