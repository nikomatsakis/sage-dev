use sage_stash::StashDirect;

use crate::cst::traits::TraitCst;
use crate::name::Name;
use crate::scope::ScopeSymbol;
use crate::span::AbsoluteSpan;

#[salsa::tracked(debug)]
pub struct LocalTraitSym<'db> {
    pub name: Name<'db>,
    pub scope: ScopeSymbol<'db>,

    #[returns(ref)]
    pub cst: TraitCst<'db>,

    #[tracked]
    pub span: AbsoluteSpan<'db>,
}

impl StashDirect for LocalTraitSym<'_> {}

impl<'db> LocalTraitSym<'db> {
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
impl<'db> LocalTraitSym<'db> {
    #[salsa::tracked]
    pub fn sig(
        self,
        db: &'db dyn crate::Db,
    ) -> sage_stash::Stashed<crate::ty::TraitSignature<'db>> {
        use crate::check::Check;
        use crate::cst::generics::CheckGenerics;
        use crate::generic_param::{AstGenericParam, GenericParam, GenericParamKind};
        use crate::resolve::{Namespace, Resolution, Resolver};
        use crate::symbol::Symbol;
        use crate::ty::{Binder, TraitSignatureData, Ty};

        let (source, cst) = self.cst(db).open_deref();
        let mut cx = Check::new(db, source, Resolver::new(db, self.scope(db)));
        cx.current_sym = Some(crate::local_syms::LocalModItemSym::Trait(self));
        let parent: Symbol<'db> = self.into();
        let explicit_generics = cst.generics.check(db, &mut cx, parent);

        let self_param = GenericParam::Ast(AstGenericParam::new(
            db,
            GenericParamKind::Type,
            Some(Name::new(db, "Self".to_owned())),
            cst.span,
            parent,
            u32::MAX,
        ));
        cx.resolver.ribs.add(
            Name::new(db, "Self".to_owned()),
            Namespace::Type,
            Resolution::SelfTy(Ty::Param(self_param)),
        );

        let (where_clauses, mut solver_eligibility) = crate::check::trait_env::lower_predicates(
            &mut cx,
            cst.generics,
            explicit_generics,
            cst.where_clauses,
        );
        if cst.is_auto || !source[cst.supertraits].is_empty() {
            solver_eligibility = crate::ty::SolverEligibility::Unsupported;
        }
        let mut all_generics = vec![self_param];
        all_generics.extend_from_slice(&cx.target_stash[explicit_generics]);
        let generics = cx.target_stash.alloc_slice(&all_generics);
        cx.finish(Binder::new(
            TraitSignatureData {
                self_param,
                where_clauses,
                solver_eligibility,
            },
            generics,
        ))
    }

    #[salsa::tracked]
    pub fn items(self, db: &'db dyn crate::Db) -> sage_stash::Stashed<crate::ty::TraitItems<'db>> {
        use crate::ty::BinderExt;

        let (source, cst) = self.cst(db).open_deref();
        crate::local_syms::associated::lower_items(
            db,
            crate::local_syms::LocalAssociatedOwner::Trait(self),
            self.scope(db),
            self.span(db),
            source,
            cst.items,
            self.sig(db).iter_symbols(),
        )
    }
}
