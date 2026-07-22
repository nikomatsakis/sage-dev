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

/// Lower one requested associated type value without reading function bodies
/// or the values of sibling associated items. GATs and attributed values are
/// outside the current normalization slice and remain unavailable.
#[salsa::tracked]
pub fn local_impl_associated_type_value<'db>(
    db: &'db dyn crate::Db,
    impl_sym: LocalImplSym<'db>,
    associated_ty: crate::symbol::TypeAliasSymbol<'db>,
) -> Option<sage_stash::Stashed<crate::ty::Binder<'db, sage_stash::Ptr<crate::ty::Ty<'db>>>>> {
    use crate::check::Check;
    use crate::cst::traits::TraitItemCst;
    use crate::resolve::Resolver;
    use crate::ty::{Binder, BinderExt, Ty};

    let expected_name = crate::symbol::Symbol::from(associated_ty).name(db)?.0;
    let (source, impl_cst) = impl_sym.cst(db).open_deref();
    let item = source[impl_cst.items].iter().find_map(|item| match item {
        TraitItemCst::Type(item) if source[*item].name == expected_name => Some(source[*item]),
        TraitItemCst::Type(_) | TraitItemCst::Fn(_) | TraitItemCst::Const(_) => None,
    })?;
    if !source[item.generics].is_empty()
        || !source[item.where_clauses].is_empty()
        || !source[item.attrs].is_empty()
    {
        return None;
    }
    let value = item.ty?;
    let signature = impl_sym.sig(db);
    let generics: Vec<_> = signature.iter_symbols().collect();
    let mut resolver = Resolver::new(db, impl_sym.scope(db));
    resolver
        .ribs
        .add_generic_params(db, generics.iter().copied());
    let mut cx = Check::new(db, source, resolver);
    cx.current_sym = Some(crate::local_syms::LocalModItemSym::Impl(impl_sym));
    let value = cx.source_stash[value].check(&mut cx);
    if matches!(value, Ty::Error(_)) || !cx.diagnostics.is_empty() {
        return None;
    }
    let value = cx.target_stash.alloc(value);
    let generics = cx.target_stash.alloc_slice(&generics);
    Some(cx.finish(Binder::new(value, generics)))
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
        let parameter_env = cx.complete_parameter_env(where_clauses, solver_eligibility);

        cx.finish(Binder::new(
            ImplSignatureData {
                trait_ref,
                self_ty,
                where_clauses: parameter_env.where_clauses,
                solver_eligibility: parameter_env.solver_eligibility,
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

#[derive(Clone, Debug, PartialEq, Eq, salsa::Update)]
pub struct LocalImplCandidates<'db> {
    pub impls: Vec<LocalImplSym<'db>>,
    pub complete: bool,
}

/// Local impls for one fixed trait, together with conservative expansion
/// completeness. This is the trait-keyed solver boundary; the current source
/// index still scans module impls internally.
#[salsa::tracked(returns(ref))]
pub fn local_impl_candidates<'db>(
    db: &'db dyn crate::Db,
    krate: crate::scope::LocalCrateSymbol<'db>,
    target_trait: crate::symbol::TraitSymbol<'db>,
) -> LocalImplCandidates<'db> {
    use crate::symbol::SymbolData;

    fn visit<'db>(
        db: &'db dyn crate::Db,
        module: crate::local_syms::mods::LocalModSym<'db>,
        target_trait: crate::symbol::TraitSymbol<'db>,
        output: &mut Vec<LocalImplSym<'db>>,
        complete: &mut bool,
    ) {
        *complete &=
            crate::local_syms::mods::module_expansion_complete_for_trait(db, module, target_trait);
        for symbol in crate::local_syms::mods::local_expanded_module_items(db, module) {
            match symbol.data(db) {
                SymbolData::ImplSymbol(crate::symbol::ImplSymbol::Local(local)) => {
                    if crate::local_syms::mods::item_has_unexpanded_active_attribute(
                        db,
                        crate::local_syms::LocalModItemSym::Impl(local),
                    ) {
                        *complete = false;
                        continue;
                    }

                    let (_, cst) = local.cst(db).open_deref();
                    let signature = local.sig(db);
                    match signature.root().value.trait_ref {
                        Some(trait_ref) if trait_ref.trait_sym == target_trait => {
                            output.push(local)
                        }
                        Some(_) => {}
                        None if cst.trait_path.is_none() => {}
                        None => *complete = false,
                    }
                }
                SymbolData::ModSymbol(crate::symbol::ModSymbol::Local(child)) => {
                    if crate::local_syms::mods::item_has_unexpanded_active_attribute(
                        db,
                        crate::local_syms::LocalModItemSym::Mod(child),
                    ) {
                        *complete = false;
                    } else {
                        visit(db, child, target_trait, output, complete)
                    }
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
    let mut complete = true;
    visit(
        db,
        krate.root_mod(db),
        target_trait,
        &mut impls,
        &mut complete,
    );
    LocalImplCandidates { impls, complete }
}
