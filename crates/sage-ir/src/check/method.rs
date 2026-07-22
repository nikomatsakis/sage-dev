use sage_stash::Ptr;

use crate::diagnostic::{Diagnostic, ErrorReported};
use crate::generic_param::GenericParamKind;
use crate::local_syms::impls::local_impls;
use crate::name::Name;
use crate::span::RelativeSpan;
use crate::symbol::{FnSymbol, StructSymbol, Symbol, SymbolData};
use crate::ty::{BinderExt, FnSig, SolverEligibility, TraitRef, Ty};
use crate::tytree::{CallDispatch, ResolvedCallTarget};

use super::infer_ctx::{InferCtx, Scope, TraitGoalCertainty};

pub(crate) struct ResolvedMethod<'db> {
    pub target: ResolvedCallTarget<'db>,
    pub signature: FnSig<'db>,
}

/// Select a trait method for the first completed-IR vertical slice.
///
/// Discovery is name-based and the solver is asked only fixed-trait goals.
/// The represented subset currently admits methods whose only type parameter
/// is the owning trait's `Self`; broader generic method inference remains an
/// explicit unknown rather than being guessed.
pub(crate) fn resolve_trait_method<'db>(
    cx: &InferCtx<'_, 'db>,
    scope: &Scope<'db>,
    receiver_ty: Ptr<Ty<'db>>,
    method_name: Name<'db>,
    span: RelativeSpan,
) -> Result<ResolvedMethod<'db>, ErrorReported> {
    let mut resolver = scope.resolver.clone();
    let (traits, scope_complete) = resolver.traits_in_method_scope();
    let mut definite = Vec::new();
    let mut unknown = !scope_complete
        || cx.has_unhandled_method_bound_providers()
        || unhandled_inherent_provider(cx, scope, receiver_ty);

    for (trait_sym, definitely_in_scope) in traits {
        let Some(items) = trait_sym.items(cx.db) else {
            unknown = true;
            continue;
        };
        // ANCHOR: example_discover_trait_methods
        let matching: Vec<FnSymbol<'db>> = items.stash()[items.root().value]
            .iter()
            .filter_map(|item| match *item {
                crate::ty::TraitItemDef::Function(function)
                    if Symbol::from(function)
                        .name(cx.db)
                        .is_some_and(|(name, _)| name == method_name) =>
                {
                    Some(function)
                }
                crate::ty::TraitItemDef::Function(_)
                | crate::ty::TraitItemDef::Type(_)
                | crate::ty::TraitItemDef::Const(_) => None,
            })
            .collect();
        // ANCHOR_END: example_discover_trait_methods

        for function in matching {
            // ANCHOR: example_classify_trait_candidate
            let Some(trait_signature) = trait_sym.sig(cx.db) else {
                unknown = true;
                continue;
            };
            let trait_binder = trait_signature.root();
            if trait_binder.value.solver_eligibility != SolverEligibility::Eligible {
                unknown = true;
                continue;
            }
            let type_generics: Vec<_> = trait_signature
                .iter_symbols()
                .filter(|generic| generic.kind(cx.db) == GenericParamKind::Type)
                .collect();
            if type_generics.as_slice() != [trait_binder.value.self_param] {
                unknown = true;
                continue;
            }

            let trait_args = cx.stash_mut().alloc_slice(&[]);
            let trait_ref = TraitRef {
                trait_sym,
                args: trait_args,
            };
            match cx.classify_trait_goal(receiver_ty, trait_ref) {
                TraitGoalCertainty::No => continue,
                TraitGoalCertainty::Maybe => {
                    unknown = true;
                    continue;
                }
                TraitGoalCertainty::Yes => {}
            }
            if !definitely_in_scope {
                // The crate edition is not represented yet. A trait exported
                // by only some standard preludes is therefore a possible, not
                // definite, provider.
                unknown = true;
                continue;
            }
            // ANCHOR_END: example_classify_trait_candidate

            // ANCHOR: example_instantiate_trait_method
            let Some(function_signature) = function.sig(cx.db) else {
                unknown = true;
                continue;
            };
            if function_signature.root().value.method_candidate_eligibility
                != SolverEligibility::Eligible
            {
                unknown = true;
                continue;
            }
            let function_type_generics: Vec<_> = function_signature
                .iter_symbols()
                .filter(|generic| generic.kind(cx.db) == GenericParamKind::Type)
                .collect();
            if function_type_generics.len() != 1 {
                unknown = true;
                continue;
            }

            let receiver_ty_value = cx.stash()[receiver_ty];
            let signature = crate::ty_fold::instantiate_fn_sig(
                cx.db,
                function_signature.stash(),
                &mut *cx.stash_mut(),
                &function_signature.root(),
                vec![receiver_ty_value],
            );
            if signature.receiver.is_none() {
                continue;
            }
            definite.push(ResolvedMethod {
                target: ResolvedCallTarget {
                    function,
                    dispatch: CallDispatch::StaticTrait {
                        self_ty: receiver_ty,
                        trait_ref,
                    },
                },
                signature,
            });
            // ANCHOR_END: example_instantiate_trait_method
        }
    }

    // ANCHOR: example_select_trait_method
    if definite.len() == 1 && !unknown {
        return Ok(definite.pop().unwrap());
    }

    let message = if definite.len() > 1 {
        "method call is ambiguous"
    } else if unknown {
        "method lookup requires unsupported or incomplete candidate information"
    } else {
        "no applicable method found"
    };
    Err(cx.record(Diagnostic::error(cx.span(span), message)))
    // ANCHOR_END: example_select_trait_method
}

fn unhandled_inherent_provider<'db>(
    cx: &InferCtx<'_, 'db>,
    scope: &Scope<'db>,
    receiver_ty: Ptr<Ty<'db>>,
) -> bool {
    let Ty::Adt(receiver_symbol, _) = cx.stash()[receiver_ty] else {
        // External, primitive, reference/autoderef, and inference-dependent
        // inherent providers are not enumerated by this vertical slice.
        return true;
    };
    if !matches!(
        receiver_symbol.data(cx.db),
        SymbolData::StructSymbol(StructSymbol::Local(_))
            | SymbolData::EnumSymbol(crate::symbol::EnumSymbol::Local(_))
    ) {
        return true;
    }

    let local_crate = scope.resolver.local_crate();
    if !crate::local_syms::mods::module_expansion_complete_for_method_providers(
        cx.db,
        local_crate.root_mod(cx.db),
    ) {
        return true;
    }

    local_impls(cx.db, local_crate).iter().any(|local_impl| {
        let signature = local_impl.sig(cx.db);
        let data = signature.root().value;
        if data.trait_ref.is_some() {
            return false;
        }
        matches!(
            signature.stash()[data.self_ty],
            Ty::Adt(impl_symbol, _) if impl_symbol == receiver_symbol
        )
    })
}
