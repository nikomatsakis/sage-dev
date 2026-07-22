use sage_stash::StashDirect;

use crate::cst::impls::ImplCst;
use crate::scope::ScopeSymbol;
use crate::span::AbsoluteSpan;

#[salsa::tracked(debug)]
pub struct LocalImplSym<'db> {
    pub scope: ScopeSymbol<'db>,

    #[returns(ref)]
    pub cst: ImplCst<'db>,

    #[tracked]
    pub span: AbsoluteSpan<'db>,
}

impl StashDirect for LocalImplSym<'_> {}

impl<'db> LocalImplSym<'db> {
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
impl<'db> LocalImplSym<'db> {
    #[salsa::tracked]
    pub fn sig(self, db: &'db dyn crate::Db) -> sage_stash::Stashed<crate::ty::ImplSignature<'db>> {
        use crate::check::Check;
        use crate::cst::generics::CheckGenerics;
        use crate::resolve::Resolver;
        use crate::symbol::Symbol;
        use crate::ty::{Binder, ImplSignatureData, SolverEligibility};

        let (source, cst) = self.cst(db).open_deref();
        let mut cx = Check::new(db, source, Resolver::new(db, self.scope(db)));
        cx.current_sym = Some(crate::local_syms::LocalModItemSym::Impl(self));
        let parent: Symbol<'db> = self.into();
        let generics = cst.generics.check(db, &mut cx, parent);
        let self_ty_value = cx.source_stash[cst.self_ty].check(&mut cx);
        let self_ty = cx.target_stash.alloc(self_ty_value);

        let (where_clauses, mut solver_eligibility) = crate::check::trait_env::lower_predicates(
            &mut cx,
            cst.generics,
            generics,
            cst.where_clauses,
        );
        let trait_ref = cst.trait_path.and_then(|path| {
            match crate::check::trait_env::lower_trait_ref(&mut cx, source[path]) {
                Ok(trait_ref) => Some(trait_ref),
                Err(()) => {
                    solver_eligibility = SolverEligibility::Unsupported;
                    None
                }
            }
        });
        if let Some(trait_ref) = trait_ref {
            solver_eligibility = solver_eligibility.and(
                crate::check::trait_env::trait_ref_eligibility(db, trait_ref),
            );
        }
        if cst.is_negative || cst.is_const || cst.is_default {
            solver_eligibility = SolverEligibility::Unsupported;
        }

        cx.finish(Binder::new(
            ImplSignatureData {
                trait_ref,
                self_ty,
                where_clauses,
                solver_eligibility,
            },
            generics,
        ))
    }

    #[salsa::tracked]
    pub fn items(self, db: &'db dyn crate::Db) -> sage_stash::Stashed<crate::ty::ImplItems<'db>> {
        use crate::ty::BinderExt;

        let (source, cst) = self.cst(db).open_deref();
        crate::local_syms::associated::lower_items(
            db,
            crate::local_syms::LocalAssociatedOwner::Impl(self),
            self.scope(db),
            self.span(db),
            source,
            cst.items,
            self.sig(db).iter_symbols(),
        )
    }
}

#[salsa::tracked(returns(ref))]
pub fn local_impls<'db>(
    db: &'db dyn crate::Db,
    krate: crate::scope::LocalCrateSymbol<'db>,
) -> Vec<LocalImplSym<'db>> {
    use crate::symbol::SymbolData;

    fn visit<'db>(
        db: &'db dyn crate::Db,
        module: crate::local_syms::mods::LocalModSym<'db>,
        output: &mut Vec<LocalImplSym<'db>>,
    ) {
        for symbol in crate::local_syms::mods::local_expanded_module_items(db, module) {
            match symbol.data(db) {
                SymbolData::ImplSymbol(crate::symbol::ImplSymbol::Local(local)) => {
                    output.push(local)
                }
                SymbolData::ModSymbol(crate::symbol::ModSymbol::Local(child)) => {
                    visit(db, child, output)
                }
                SymbolData::ImplSymbol(crate::symbol::ImplSymbol::Ext(_))
                | SymbolData::ModSymbol(crate::symbol::ModSymbol::Ext(_))
                | SymbolData::FnSymbol(_)
                | SymbolData::StructSymbol(_)
                | SymbolData::EnumSymbol(_)
                | SymbolData::VariantSymbol(_)
                | SymbolData::VariantCtorSymbol(_)
                | SymbolData::TraitSymbol(_)
                | SymbolData::TypeAliasSymbol(_)
                | SymbolData::ConstSymbol(_)
                | SymbolData::StaticSymbol(_)
                | SymbolData::MacroDefSymbol(_)
                | SymbolData::IntrinsicTypeSymbol(_)
                | SymbolData::MacroInvocationSymbol(_)
                | SymbolData::UseSymbol(_) => {}
            }
        }
    }

    let mut impls = Vec::new();
    visit(db, krate.root_mod(db), &mut impls);
    impls
}
