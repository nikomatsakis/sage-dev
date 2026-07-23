use sage_stash::Ptr;

use crate::diagnostic::{Diagnostic, ErrorReported};
use crate::generic_param::GenericParamKind;
use crate::local_syms::impls::local_impls;
use crate::name::Name;
use crate::span::RelativeSpan;
use crate::symbol::{EnumSymbol, FnSymbol, StructSymbol, Symbol, SymbolData};
use crate::ty::{BinderExt, FnSig, SolverEligibility, TraitRef, Ty};
use crate::tytree::{CallDispatch, ResolvedCallTarget};

use super::infer_ctx::{InferCtx, Scope, TraitGoalCertainty};

pub(crate) struct ResolvedMethod<'db> {
    pub target: ResolvedCallTarget<'db>,
    pub signature: FnSig<'db>,
    pub parameter_env_published: bool,
}

pub(crate) fn resolve_method<'db>(
    cx: &InferCtx<'_, 'db>,
    scope: &Scope<'db>,
    receiver_ty: Ptr<Ty<'db>>,
    method_name: Name<'db>,
    arguments: &[(Ptr<Ty<'db>>, RelativeSpan)],
    span: RelativeSpan,
) -> Result<ResolvedMethod<'db>, ErrorReported> {
    if let Some(result) =
        resolve_external_inherent_method(cx, receiver_ty, method_name, arguments, span)
    {
        return result;
    }
    resolve_trait_method(cx, scope, receiver_ty, method_name, span)
}

/// Select a trait method for the first completed-IR vertical slice.
///
/// Discovery is name-based and the solver is asked only fixed-trait goals.
/// The represented subset currently admits methods whose only type parameter
/// is the owning trait's `Self`; broader generic method inference remains an
/// explicit unknown rather than being guessed.
fn resolve_trait_method<'db>(
    cx: &InferCtx<'_, 'db>,
    scope: &Scope<'db>,
    receiver_ty: Ptr<Ty<'db>>,
    method_name: Name<'db>,
    span: RelativeSpan,
) -> Result<ResolvedMethod<'db>, ErrorReported> {
    let mut resolver = scope.resolver.clone();
    let (traits, scope_complete) = resolver.traits_in_method_scope();
    let mut definite = Vec::new();
    let bound_provider_unknown = cx.has_unhandled_method_bound_providers(receiver_ty);
    let inherent_provider_unknown =
        unhandled_inherent_provider(cx, scope, receiver_ty, method_name);
    let mut unknown = !scope_complete || bound_provider_unknown || inherent_provider_unknown;

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
                parameter_env_published: false,
            });
            // ANCHOR_END: example_instantiate_trait_method
        }
    }

    // ANCHOR: example_select_trait_method
    if definite.len() == 1 && !unknown {
        let mut method = definite.pop().unwrap();
        method.signature = cx.normalize_call_signature(method.signature, span);
        return Ok(method);
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

fn resolve_external_inherent_method<'db>(
    cx: &InferCtx<'_, 'db>,
    receiver_ty: Ptr<Ty<'db>>,
    method_name: Name<'db>,
    arguments: &[(Ptr<Ty<'db>>, RelativeSpan)],
    span: RelativeSpan,
) -> Option<Result<ResolvedMethod<'db>, ErrorReported>> {
    let Ty::Adt(receiver_symbol, _) = cx.stash()[receiver_ty] else {
        return None;
    };
    let external = match receiver_symbol.data(cx.db) {
        SymbolData::StructSymbol(StructSymbol::Ext(external))
        | SymbolData::EnumSymbol(EnumSymbol::Ext(external)) => external,
        SymbolData::StructSymbol(StructSymbol::Local(_))
        | SymbolData::EnumSymbol(EnumSymbol::Local(_))
        | SymbolData::FnSymbol(_)
        | SymbolData::VariantSymbol(_)
        | SymbolData::VariantCtorSymbol(_)
        | SymbolData::TraitSymbol(_)
        | SymbolData::TypeAliasSymbol(_)
        | SymbolData::ConstSymbol(_)
        | SymbolData::StaticSymbol(_)
        | SymbolData::ImplSymbol(_)
        | SymbolData::ModSymbol(_)
        | SymbolData::MacroDefSymbol(_)
        | SymbolData::UseSymbol(_)
        | SymbolData::IntrinsicTypeSymbol(_)
        | SymbolData::MacroInvocationSymbol(_) => return None,
    };

    // ANCHOR: example_select_external_inherent_method
    let candidates =
        crate::external_syms::external_inherent_method_candidates(cx.db, external, method_name);
    if !candidates.complete {
        return Some(Err(cx.record(Diagnostic::error(
            cx.span(span),
            "external inherent method lookup is incomplete",
        ))));
    }
    if candidates.candidates.is_empty() {
        return None;
    }
    let visible: Vec<_> = candidates
        .candidates
        .iter()
        .filter(|candidate| candidate.externally_visible)
        .collect();
    if visible.is_empty() {
        return Some(Err(cx.record(Diagnostic::error(
            cx.span(span),
            "inherent method is not visible from this crate",
        ))));
    }
    let [candidate] = visible.as_slice() else {
        return Some(Err(cx.record(Diagnostic::error(
            cx.span(span),
            "inherent method call is ambiguous",
        ))));
    };
    let candidate = **candidate;
    // ANCHOR_END: example_select_external_inherent_method

    let Some(signature) = candidate.function.sig(cx.db) else {
        return Some(Err(cx.record(Diagnostic::error(
            cx.span(span),
            "external inherent method signature is unavailable",
        ))));
    };
    let binder = signature.root();
    let source = signature.stash();
    if binder.value.method_candidate_eligibility != SolverEligibility::Eligible
        || binder.value.receiver.is_none()
        || binder.value.owner_self_ty.is_none()
        || binder.value.owner_generic_count as usize > source[binder.generics].len()
    {
        return Some(Err(cx.record(Diagnostic::error(
            cx.span(span),
            "external inherent method signature is outside the represented subset",
        ))));
    }

    let parameter_count = source[binder.value.params].len();
    if parameter_count != arguments.len() {
        return Some(Err(cx.record(Diagnostic::error(
            cx.span(span),
            "method argument count does not match its signature",
        ))));
    }

    // ANCHOR: example_instantiate_external_inherent_method
    let instantiated =
        instantiate_external_inherent_signature(cx, &signature, receiver_ty, arguments, span);
    let Ok(signature) = instantiated else {
        return Some(Err(cx.record(Diagnostic::error(
            cx.span(span),
            "selected inherent method signature does not match this call",
        ))));
    };
    // ANCHOR_END: example_instantiate_external_inherent_method
    Some(Ok(ResolvedMethod {
        target: ResolvedCallTarget {
            function: candidate.function,
            dispatch: CallDispatch::Direct,
        },
        signature,
        parameter_env_published: true,
    }))
}

fn instantiate_external_inherent_signature<'db>(
    cx: &InferCtx<'_, 'db>,
    signature: &sage_stash::Stashed<crate::ty::Binder<'db, FnSig<'db>>>,
    receiver_ty: Ptr<Ty<'db>>,
    arguments: &[(Ptr<Ty<'db>>, RelativeSpan)],
    span: RelativeSpan,
) -> Result<FnSig<'db>, ()> {
    use super::infer::obligations::{ObligationReason, StagedObligationBatch};
    use super::infer::version::{Universe, Version};

    let binder = signature.root();
    let source = signature.stash();
    let transaction = cx.branch_from(Version::ROOT);
    let type_argument_ptrs: Vec<_> = source[binder.generics]
        .iter()
        .filter(|generic| generic.kind(cx.db) == GenericParamKind::Type)
        .map(|_| cx.fresh_ty_var_in(transaction, Universe(1)))
        .collect();
    let type_arguments: Vec<_> = type_argument_ptrs
        .iter()
        .map(|argument| cx.stash()[*argument])
        .collect();
    let instantiated = crate::ty_fold::instantiate_fn_sig(
        cx.db,
        source,
        &mut *cx.stash_mut(),
        &binder,
        type_arguments,
    );
    let receiver_matches = instantiated
        .owner_self_ty
        .is_some_and(|owner_self_ty| cx.try_eq_in(transaction, receiver_ty, owner_self_ty));
    let parameters = cx.stash()[instantiated.params].to_vec();
    let arguments_match = receiver_matches
        && parameters
            .into_iter()
            .zip(arguments.iter().map(|(ty, _)| *ty))
            .all(|(parameter, argument)| cx.try_eq_in(transaction, parameter, argument));
    if !arguments_match {
        cx.discard_branch(transaction);
        return Err(());
    }

    let mut obligations = StagedObligationBatch::new();
    obligations.push_parameter_env(
        instantiated.parameter_env,
        span,
        ObligationReason::FunctionCall,
    );
    cx.commit_branch(transaction);
    cx.publish_obligation_batch(obligations);
    let signature = cx.normalize_call_signature(instantiated, span);
    Ok(signature)
}

fn unhandled_inherent_provider<'db>(
    cx: &InferCtx<'_, 'db>,
    scope: &Scope<'db>,
    receiver_ty: Ptr<Ty<'db>>,
    method_name: Name<'db>,
) -> bool {
    let Ty::Adt(receiver_symbol, _) = cx.stash()[receiver_ty] else {
        // Primitive, reference/autoderef, and inference-dependent inherent
        // providers are not enumerated by this vertical slice.
        return true;
    };
    match receiver_symbol.data(cx.db) {
        SymbolData::StructSymbol(StructSymbol::Ext(adt))
        | SymbolData::EnumSymbol(EnumSymbol::Ext(adt)) => {
            let candidates =
                crate::external_syms::external_inherent_method_candidates(cx.db, adt, method_name);
            return !candidates.complete || !candidates.candidates.is_empty();
        }
        SymbolData::StructSymbol(StructSymbol::Local(_))
        | SymbolData::EnumSymbol(EnumSymbol::Local(_)) => {}
        SymbolData::FnSymbol(_)
        | SymbolData::VariantSymbol(_)
        | SymbolData::VariantCtorSymbol(_)
        | SymbolData::TraitSymbol(_)
        | SymbolData::TypeAliasSymbol(_)
        | SymbolData::ConstSymbol(_)
        | SymbolData::StaticSymbol(_)
        | SymbolData::ImplSymbol(_)
        | SymbolData::ModSymbol(_)
        | SymbolData::MacroDefSymbol(_)
        | SymbolData::UseSymbol(_)
        | SymbolData::IntrinsicTypeSymbol(_)
        | SymbolData::MacroInvocationSymbol(_) => return true,
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::check::infer::bound::Bound;
    use crate::db::Database;
    use crate::symbol::{CrateNum, DefIndex, SymExt, SymExtKind};
    use crate::tcx::{
        ExternalDefPath, RawChild, RawDefId, RawFnSignature, RawGenericParam, RawGenericParamKind,
        RawReceiver, RawTy, TcxDb,
    };
    use sage_stash::Stash;
    use salsa::Database as _;

    #[derive(Clone)]
    struct SignatureTcx {
        ordinary_complete: bool,
    }

    impl TcxDb for SignatureTcx {
        fn extern_crate(&self, _name: &str) -> Option<CrateNum> {
            None
        }

        fn module_children(&self, _crate_num: CrateNum, _def_index: DefIndex) -> Vec<RawChild> {
            Vec::new()
        }

        fn item_name(&self, _crate_num: CrateNum, _def_index: DefIndex) -> Option<String> {
            None
        }

        fn is_module(&self, _crate_num: CrateNum, _def_index: DefIndex) -> bool {
            false
        }

        fn is_builtin_derive(&self, _crate_num: CrateNum, _def_index: DefIndex) -> bool {
            false
        }

        fn def_path(&self, _crate_num: CrateNum, _def_index: DefIndex) -> Option<String> {
            None
        }

        fn structured_def_path(
            &self,
            _crate_num: CrateNum,
            _def_index: DefIndex,
        ) -> Option<ExternalDefPath> {
            None
        }

        fn fn_signature(
            &self,
            _crate_num: CrateNum,
            _def_index: DefIndex,
        ) -> Option<RawFnSignature> {
            Some(RawFnSignature {
                owner_generics: vec![RawGenericParam {
                    index: 0,
                    name: Some("T".to_owned()),
                    kind: RawGenericParamKind::Type,
                }],
                method_generics: vec![RawGenericParam {
                    index: 1,
                    name: Some("E".to_owned()),
                    kind: RawGenericParamKind::Type,
                }],
                owner_self_ty: Some(RawTy::Adt(
                    RawDefId {
                        crate_num: CrateNum(1),
                        def_index: DefIndex(10),
                        kind: SymExtKind::Struct,
                    },
                    vec![RawTy::Param(0)],
                )),
                owner_trait: None,
                receiver: Some(RawReceiver::Value),
                params: vec![RawTy::Param(1), RawTy::Bool],
                ret: RawTy::Bool,
                predicates: Vec::new(),
                ordinary_complete: self.ordinary_complete,
                const_call_complete: true,
            })
        }

        fn expand_proc_macro_derive(
            &self,
            _crate_num: CrateNum,
            _def_index: DefIndex,
            _item_source: &str,
        ) -> Option<String> {
            None
        }

        fn expand_proc_macro_bang(
            &self,
            _crate_num: CrateNum,
            _def_index: DefIndex,
            _input_tokens: &str,
        ) -> Option<String> {
            None
        }

        fn expand_proc_macro_attr(
            &self,
            _crate_num: CrateNum,
            _def_index: DefIndex,
            _attr_args: &str,
            _item_source: &str,
        ) -> Option<String> {
            None
        }
    }

    #[test]
    fn external_method_mismatch_discards_partial_generic_bindings() {
        let database = Database::new(SignatureTcx {
            ordinary_complete: true,
        });
        database.attach(|db| {
            let source_stash = Stash::new();
            let cx = InferCtx::new(db, &source_stash, None);
            let function =
                FnSymbol::Ext(SymExt::new(db, CrateNum(1), DefIndex(11), SymExtKind::Fn));
            let signature = function.sig(db).expect("synthetic external signature");

            let bool_ty = cx.alloc_ty(Ty::Bool);
            let char_ty = cx.alloc_ty(Ty::Char);
            let caller_var = cx.fresh_ty_var();
            let owner = StructSymbol::Ext(SymExt::new(
                db,
                CrateNum(1),
                DefIndex(10),
                SymExtKind::Struct,
            ));
            let owner_args = cx.stash_mut().alloc_slice(&[bool_ty]);
            let receiver_ty = cx.alloc_ty(Ty::Adt(owner.into(), owner_args));
            let span = RelativeSpan { start: 0, end: 0 };
            let root_revision = cx.root_semantic_revision();

            assert!(
                instantiate_external_inherent_signature(
                    &cx,
                    &signature,
                    receiver_ty,
                    &[(caller_var, span), (char_ty, span)],
                    span,
                )
                .is_err(),
                "the second argument must reject the selected signature"
            );
            assert_eq!(cx.get_bound(caller_var), Bound::None);
            assert_eq!(
                cx.root_semantic_revision(),
                root_revision,
                "the first argument's method-generic binding must roll back"
            );
        });
    }

    #[test]
    fn incomplete_ordinary_external_method_contract_is_ineligible() {
        let database = Database::new(SignatureTcx {
            ordinary_complete: false,
        });
        database.attach(|db| {
            let function =
                FnSymbol::Ext(SymExt::new(db, CrateNum(1), DefIndex(11), SymExtKind::Fn));
            let signature = function.sig(db).expect("synthetic external signature");

            assert_eq!(
                signature.root().value.method_candidate_eligibility,
                SolverEligibility::Unsupported,
                "an unknown ordinary predicate must block method selection"
            );
        });
    }
}
