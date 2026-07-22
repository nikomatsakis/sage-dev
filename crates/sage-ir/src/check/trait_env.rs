use sage_stash::{Ptr, Slice};

use crate::check::Check;
use crate::cst::generics::{GenericParamCst, TypeBoundCst};
use crate::cst::paths::Path;
use crate::cst::where_clause::WhereClauseCst;
use crate::diagnostic::Diagnostic;
use crate::generic_param::{GenericParam, GenericParamKind};
use crate::resolve::{Namespace, Resolution};
use crate::symbol::{SymbolData, TraitSymbol};
use crate::ty::{SolverEligibility, TraitRef, Ty, WherePredicate};

pub(crate) fn type_only_generics<'db>(
    db: &'db dyn crate::Db,
    generics: &[GenericParam<'db>],
) -> SolverEligibility {
    if generics
        .iter()
        .all(|generic| generic.kind(db) == GenericParamKind::Type)
    {
        SolverEligibility::Eligible
    } else {
        SolverEligibility::Unsupported
    }
}

pub(crate) fn source_generics_supported(
    stash: &sage_stash::Stash,
    generics: Slice<GenericParamCst<'_>>,
) -> SolverEligibility {
    if stash[generics].iter().all(|generic| {
        matches!(
            generic,
            GenericParamCst::Type {
                name: _,
                bounds: _,
                default: None,
                span: _,
            }
        )
    }) {
        SolverEligibility::Eligible
    } else {
        SolverEligibility::Unsupported
    }
}

pub(crate) fn lower_predicates<'db>(
    cx: &mut Check<'_, 'db>,
    source_generics: Slice<GenericParamCst<'db>>,
    checked_generics: Slice<GenericParam<'db>>,
    where_clauses: Slice<WhereClauseCst<'db>>,
) -> (Slice<WherePredicate<'db>>, SolverEligibility) {
    let mut predicates = Vec::new();
    let mut eligibility = type_only_generics(cx.db, &cx.target_stash[checked_generics])
        .and(source_generics_supported(cx.source_stash, source_generics));

    let source_params = cx.source_stash[source_generics].to_vec();
    let checked_params = cx.target_stash[checked_generics].to_vec();
    for (source, checked) in source_params.into_iter().zip(checked_params) {
        let GenericParamCst::Type { bounds, .. } = source else {
            continue;
        };
        let self_ty = cx.target_stash.alloc(Ty::Param(checked));
        lower_bounds(cx, self_ty, bounds, &mut predicates, &mut eligibility);
    }

    for clause in cx.source_stash[where_clauses].to_vec() {
        let self_ty_value = cx.source_stash[clause.subject].check(cx);
        let self_ty = cx.target_stash.alloc(self_ty_value);
        lower_bounds(
            cx,
            self_ty,
            clause.bounds,
            &mut predicates,
            &mut eligibility,
        );
    }

    (cx.target_stash.alloc_slice(&predicates), eligibility)
}

fn lower_bounds<'db>(
    cx: &mut Check<'_, 'db>,
    self_ty: Ptr<Ty<'db>>,
    bounds: Slice<TypeBoundCst<'db>>,
    predicates: &mut Vec<WherePredicate<'db>>,
    eligibility: &mut SolverEligibility,
) {
    for bound in cx.source_stash[bounds].to_vec() {
        match bound {
            TypeBoundCst::Trait(path) => match lower_trait_ref(cx, cx.source_stash[path]) {
                Ok(trait_ref) => {
                    *eligibility = eligibility.and(trait_ref_eligibility(cx.db, trait_ref));
                    predicates.push(WherePredicate { self_ty, trait_ref });
                }
                Err(()) => *eligibility = SolverEligibility::Unsupported,
            },
            TypeBoundCst::Lifetime(_) => {
                *eligibility = SolverEligibility::Unsupported;
            }
        }
    }
}

pub(crate) fn trait_ref_eligibility(
    db: &dyn crate::Db,
    trait_ref: TraitRef<'_>,
) -> SolverEligibility {
    match trait_ref.trait_sym {
        TraitSymbol::Local(local) => {
            let (stash, cst) = local.cst(db).open_deref();
            if source_generics_supported(stash, cst.generics).is_eligible()
                && stash[cst.supertraits].is_empty()
                && !cst.is_auto
            {
                SolverEligibility::Eligible
            } else {
                SolverEligibility::Unsupported
            }
        }
        TraitSymbol::Ext(_) => SolverEligibility::Unsupported,
    }
}

pub(crate) fn lower_trait_ref<'db>(
    cx: &mut Check<'_, 'db>,
    path: Path<'db>,
) -> Result<TraitRef<'db>, ()> {
    let segment = path.final_segment(cx);
    let args = segment.check_type_args(cx);
    let Some(Resolution::Sym(symbol)) = path.resolve(cx, Namespace::Type) else {
        cx.report(Diagnostic::error(
            cx.span(segment.span),
            "unresolved trait in type predicate",
        ));
        return Err(());
    };
    let trait_sym = match symbol.data(cx.db) {
        SymbolData::TraitSymbol(trait_sym) => trait_sym,
        SymbolData::FnSymbol(_)
        | SymbolData::StructSymbol(_)
        | SymbolData::EnumSymbol(_)
        | SymbolData::VariantSymbol(_)
        | SymbolData::VariantCtorSymbol(_)
        | SymbolData::TypeAliasSymbol(_)
        | SymbolData::ConstSymbol(_)
        | SymbolData::StaticSymbol(_)
        | SymbolData::ImplSymbol(_)
        | SymbolData::ModSymbol(_)
        | SymbolData::MacroDefSymbol(_)
        | SymbolData::IntrinsicTypeSymbol(_)
        | SymbolData::MacroInvocationSymbol(_)
        | SymbolData::UseSymbol(_) => {
            cx.report(Diagnostic::error(
                cx.span(segment.span),
                "expected a trait in type predicate",
            ));
            return Err(());
        }
    };

    match trait_sym {
        TraitSymbol::Local(local) => {
            let (trait_stash, trait_cst) = local.cst(cx.db).open_deref();
            let expected = trait_stash[trait_cst.generics]
                .iter()
                .filter(|generic| matches!(generic, GenericParamCst::Type { .. }))
                .count();
            let actual = cx.target_stash[args].len();
            if actual != expected {
                cx.report(Diagnostic::error(
                    cx.span(segment.span),
                    format!("trait expects {expected} type arguments, found {actual}"),
                ));
                return Err(());
            }
        }
        TraitSymbol::Ext(_) => {}
    }

    Ok(TraitRef { trait_sym, args })
}

pub(crate) fn contains_erased_lifetime(stash: &sage_stash::Stash, ty: Ptr<Ty<'_>>) -> bool {
    match stash[ty] {
        Ty::Ref(_, _, crate::ty::Lifetime::Erased) => true,
        Ty::Bool
        | Ty::Char
        | Ty::Int(_)
        | Ty::Uint(_)
        | Ty::Float(_)
        | Ty::Str
        | Ty::Adt(_, _)
        | Ty::Ref(_, _, crate::ty::Lifetime::Param(_) | crate::ty::Lifetime::Static)
        | Ty::Tuple(_)
        | Ty::Slice(_)
        | Ty::Array(_, _)
        | Ty::FnPtr(_, _)
        | Ty::Param(_)
        | Ty::InferVar(_)
        | Ty::Never
        | Ty::Error(_) => crate::check::infer::skeleton::decompose(stash, ty)
            .children
            .into_iter()
            .any(|child| contains_erased_lifetime(stash, child)),
    }
}
