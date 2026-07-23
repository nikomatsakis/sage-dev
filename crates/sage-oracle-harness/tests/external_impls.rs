#![feature(rustc_private)]

use std::path::PathBuf;

use sage_ir::check::infer::egraph::VersionedEGraph;
use sage_ir::check::infer::version::{Universe, Version};
use sage_ir::check::solve::{
    Assumption, GoalOutput, GoalQuery, QueryResultData, SolverGoal, canonicalize_solver_goal,
};
use sage_ir::name::Name;
use sage_ir::scope::ScopeSymbol;
use sage_ir::symbol::{FnSymbol, StructSymbol, Symbol, SymbolData};
use sage_ir::ty::{
    AliasTy, BinderExt, Lifetime, ProjectionTy, SolverEligibility, TraitItemDef, TraitRef, Ty,
};
use sage_ir::tytree::{CallDispatch, TyExprData};
use sage_oracle_harness::{Fixture, combined};
use sage_stash::{Stash, StashCopy};

#[test]
fn into_iter_iterator_proof_reads_only_relevant_external_headers() {
    let fixture = Fixture::SingleFile(
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../test-fixtures/solver/external_iterator_impl.rs"),
    );

    let (diagnostics, cold, warm) = combined::with_proxy_database(&fixture, |database, files| {
        let refs: Vec<_> = files
            .iter()
            .map(|(path, source)| (path.as_str(), source.as_str()))
            .collect();
        sage_test_harness::with_test_crate_files_twice_using_db(database, &refs, |db, root| {
            let function = root
                .expanded_module_items(db)
                .iter()
                .find_map(|symbol| match symbol.data(db) {
                    SymbolData::FnSymbol(FnSymbol::Local(function))
                        if function.name(db).text(db) == "check" =>
                    {
                        Some(function)
                    }
                    SymbolData::FnSymbol(FnSymbol::Local(_))
                    | SymbolData::FnSymbol(FnSymbol::Ext(_))
                    | SymbolData::StructSymbol(_)
                    | SymbolData::EnumSymbol(_)
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
                    | SymbolData::MacroInvocationSymbol(_) => None,
                })
                .expect("check function");
            function
                .body(db)
                .diagnostics
                .iter()
                .map(|diagnostic| diagnostic.render(db))
                .collect::<Vec<_>>()
        })
    });

    assert!(
        diagnostics.is_empty(),
        "{diagnostics:#?}\ncold trace:\n{cold}\nwarm trace:\n{warm}"
    );
    let relevant: Vec<_> = cold
        .lines()
        .filter(|line| line.contains("tcx::relevant_trait_impls"))
        .collect();
    assert_eq!(relevant.len(), 2, "cold trace:\n{cold}");
    assert!(
        relevant.iter().all(|line| line.contains("Some(Adt(")),
        "both Iterator and nested Allocator lookup need rigid ADT heads:\n{cold}"
    );
    let headers: Vec<_> = cold
        .lines()
        .filter(|line| line.contains("tcx::impl_signature"))
        .collect();
    assert_eq!(headers.len(), 2, "cold trace:\n{cold}");
    assert!(
        !cold.contains("tcx::associated_items"),
        "truth proof must not read associated items or values:\n{cold}"
    );
    let local_bodies = cold
        .lines()
        .filter(|line| line.contains("LocalFnSym") && line.contains("::body_"))
        .count();
    assert_eq!(
        local_bodies, 1,
        "only the requested `check` body may be read; no callee body is relevant:\n{cold}"
    );
    assert!(
        !warm.contains("tcx::relevant_trait_impls") && !warm.contains("tcx::impl_signature"),
        "unchanged proof must reuse metadata queries:\n{warm}"
    );
}

#[test]
fn into_iter_item_normalization_reads_only_the_requested_associated_value() {
    let fixture = Fixture::SingleFile(
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../test-fixtures/solver/external_iterator_impl.rs"),
    );

    let (_output, cold, warm) = combined::with_proxy_database(&fixture, |database, files| {
        let refs: Vec<_> = files
            .iter()
            .map(|(path, source)| (path.as_str(), source.as_str()))
            .collect();
        sage_test_harness::with_test_crate_files_twice_using_db(database, &refs, |db, root| {
            let function = root
                .expanded_module_items(db)
                .iter()
                .find_map(|symbol| match symbol.data(db) {
                    SymbolData::FnSymbol(FnSymbol::Local(function))
                        if function.name(db).text(db) == "check" =>
                    {
                        Some(function)
                    }
                    SymbolData::FnSymbol(_)
                    | SymbolData::StructSymbol(_)
                    | SymbolData::EnumSymbol(_)
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
                    | SymbolData::MacroInvocationSymbol(_) => None,
                })
                .unwrap();
            let requires_iterator = root
                .expanded_module_items(db)
                .iter()
                .find_map(|symbol| match symbol.data(db) {
                    SymbolData::StructSymbol(StructSymbol::Local(item))
                        if item.name(db).text(db) == "RequiresIterator" =>
                    {
                        Some(item)
                    }
                    SymbolData::StructSymbol(_)
                    | SymbolData::FnSymbol(_)
                    | SymbolData::EnumSymbol(_)
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
                    | SymbolData::MacroInvocationSymbol(_) => None,
                })
                .unwrap();

            let function_sig = function.sig(db);
            let (function_stash, function_sig) = function_sig.open();
            let self_ty = function_stash[function_sig.value.params][0];
            let Ty::Adt(_, self_args) = function_stash[self_ty] else {
                panic!("expected IntoIter parameter")
            };
            let expected_item = function_stash[self_args][0];
            let Ty::Adt(expected_symbol, _) = function_stash[expected_item] else {
                panic!("expected Frame type argument")
            };

            let struct_sig = requires_iterator.sig(db);
            let (struct_stash, struct_sig) = struct_sig.open();
            let [iterator_predicate] = &struct_stash[struct_sig.value.parameter_env.where_clauses]
            else {
                panic!("expected Iterator bound")
            };
            let iterator = iterator_predicate.trait_ref.trait_sym;
            let items = iterator.items(db).unwrap();
            let (items_stash, items) = items.open();
            let associated_ty = items_stash[items.value]
                .iter()
                .find_map(|item| match item {
                    TraitItemDef::Type(item)
                        if sage_ir::symbol::Symbol::from(*item)
                            .name(db)
                            .is_some_and(|(name, _)| name.text(db) == "Item") =>
                    {
                        Some(*item)
                    }
                    TraitItemDef::Type(_) | TraitItemDef::Function(_) | TraitItemDef::Const(_) => {
                        None
                    }
                })
                .unwrap();
            let ScopeSymbol::Crate(krate) = function.scope(db) else {
                panic!("check should be crate scoped")
            };

            let mut stash = Stash::new();
            let self_ty = self_ty.stash_copy(function_stash, &mut stash);
            let projection = AliasTy::Associated(ProjectionTy {
                associated_ty,
                self_ty,
                trait_ref: TraitRef {
                    trait_sym: iterator,
                    args: stash.alloc_slice(&[]),
                },
                args: stash.alloc_slice(&[]),
            });
            let assumptions = stash.alloc_slice::<Assumption>(&[]);
            let egraph = VersionedEGraph::new();
            let canonical = canonicalize_solver_goal(
                db,
                &stash,
                &egraph,
                Version::ROOT,
                krate,
                Universe::ROOT,
                true,
                assumptions,
                SolverGoal::Normalize(projection),
            );
            let result = GoalQuery::new(db, canonical.data).solve(db);
            let (result_stash, result) = result.open();
            let QueryResultData::Yes {
                output: GoalOutput::Type(output),
                ..
            } = result.value
            else {
                return false;
            };
            let Ty::Adt(output_symbol, output_args) = result_stash[output] else {
                panic!("expected Frame output")
            };
            assert_eq!(output_symbol, expected_symbol);
            assert!(result_stash[output_args].is_empty());
            true
        })
    });

    assert!(_output, "Iterator::Item should normalize:\n{cold}");
    assert_eq!(
        cold.matches("tcx::associated_type_value").count(),
        1,
        "{cold}"
    );
    assert_eq!(
        cold.matches("tcx::relevant_trait_impls").count(),
        2,
        "normalization needs only Iterator and its Allocator condition:\n{cold}"
    );
    assert!(
        !cold.contains("LocalFnSym < 'db >::body_"),
        "the direct normalization query must not read any function body:\n{cold}"
    );
    assert!(
        !warm.contains("tcx::associated_type_value")
            && !warm.contains("tcx::relevant_trait_impls")
            && !warm.contains("tcx::impl_signature"),
        "the unchanged canonical query should reuse every semantic dependency:\n{warm}"
    );
}

#[test]
fn iterator_next_elaborates_mutable_receiver_and_normalizes_item() {
    let fixture = Fixture::SingleFile(
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../test-fixtures/solver/external_iterator_next.rs"),
    );

    let (diagnostics, cold, warm) = combined::with_proxy_database(&fixture, |database, files| {
        let refs: Vec<_> = files
            .iter()
            .map(|(path, source)| (path.as_str(), source.as_str()))
            .collect();
        sage_test_harness::with_test_crate_files_twice_using_db(database, &refs, |db, root| {
            let frame = root
                .expanded_module_items(db)
                .iter()
                .find_map(|symbol| match symbol.data(db) {
                    SymbolData::StructSymbol(StructSymbol::Local(frame))
                        if frame.name(db).text(db) == "Frame" =>
                    {
                        Some(frame)
                    }
                    SymbolData::StructSymbol(StructSymbol::Local(_))
                    | SymbolData::StructSymbol(StructSymbol::Ext(_))
                    | SymbolData::FnSymbol(_)
                    | SymbolData::EnumSymbol(_)
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
                    | SymbolData::MacroInvocationSymbol(_) => None,
                })
                .expect("Frame struct");
            let function = root
                .expanded_module_items(db)
                .iter()
                .find_map(|symbol| match symbol.data(db) {
                    SymbolData::FnSymbol(FnSymbol::Local(function))
                        if function.name(db).text(db) == "next_item" =>
                    {
                        Some(function)
                    }
                    SymbolData::FnSymbol(_)
                    | SymbolData::StructSymbol(_)
                    | SymbolData::EnumSymbol(_)
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
                    | SymbolData::MacroInvocationSymbol(_) => None,
                })
                .expect("next_item function");
            let signature = function.sig(db);
            let (signature_stash, signature) = signature.open();
            let Ty::Adt(expected_option, _) = signature_stash[signature.value.ret] else {
                panic!("next_item must declare an Option return type")
            };
            let checked = function.body(db);
            let diagnostics: Vec<_> = checked
                .diagnostics
                .iter()
                .map(|diagnostic| diagnostic.render(db))
                .collect();
            if !diagnostics.is_empty() {
                return diagnostics;
            }
            let (stash, body) = checked.body.open_deref();
            let TyExprData::Block(_, Some(call)) = stash[body.root].data else {
                panic!("expected a tail call")
            };
            let TyExprData::ResolvedCall(target, arguments) = stash[call].data else {
                panic!("method syntax must be fully elaborated")
            };
            assert_eq!(
                Symbol::from(target.function).name(db).unwrap().0.text(db),
                "next"
            );
            let CallDispatch::StaticTrait { self_ty, trait_ref } = target.dispatch else {
                panic!("Iterator::next must use static trait dispatch")
            };
            assert_eq!(
                Symbol::from(trait_ref.trait_sym)
                    .name(db)
                    .unwrap()
                    .0
                    .text(db),
                "Iterator"
            );
            assert!(matches!(stash[self_ty], Ty::Adt(_, _)));
            let [receiver] = &stash[arguments] else {
                panic!("Iterator::next takes only its receiver")
            };
            let TyExprData::Ref(_, sage_ir::cst::Mutability::Mut) = stash[*receiver].data else {
                panic!("&mut self must become an explicit mutable borrow")
            };
            assert!(matches!(
                stash[stash[*receiver].ty],
                Ty::Ref(_, sage_ir::cst::Mutability::Mut, Lifetime::Dummy)
            ));
            let Ty::Adt(option, item_args) = stash[stash[call].ty] else {
                panic!("Iterator::next must return Option<Frame>")
            };
            assert_eq!(option, expected_option);
            assert_eq!(Symbol::from(option).name(db).unwrap().0.text(db), "Option");
            let [item] = &stash[item_args] else {
                panic!("Option must retain its element type")
            };
            let Ty::Adt(item, item_args) = stash[*item] else {
                panic!("Iterator::Item must normalize to Frame")
            };
            assert_eq!(item, StructSymbol::Local(frame).into());
            assert!(stash[item_args].is_empty());
            diagnostics
        })
    });

    assert!(
        diagnostics.is_empty(),
        "{diagnostics:#?}\ncold trace:\n{cold}"
    );
    let inherent_lookups: Vec<_> = cold
        .lines()
        .filter(|line| line.contains("tcx::inherent_method_candidates"))
        .collect();
    assert_eq!(
        inherent_lookups.len(),
        1,
        "trait shadow auditing needs one name-keyed lookup:\n{cold}"
    );
    assert!(
        inherent_lookups[0].contains("\"next\""),
        "the lookup must be keyed by the requested method name:\n{cold}"
    );
    assert_eq!(
        cold.matches("tcx::associated_type_value").count(),
        1,
        "only Iterator::Item should be read:\n{cold}"
    );
    assert_eq!(
        cold.matches("tcx::fn_signature").count(),
        1,
        "only the selected Iterator::next signature should be read:\n{cold}"
    );
    assert_eq!(
        cold.matches("tcx::relevant_trait_impls").count(),
        2,
        "only Iterator and its represented Allocator condition need impl discovery:\n{cold}"
    );
    assert_eq!(
        cold.matches("tcx::impl_signature").count(),
        2,
        "only the surviving Iterator and Allocator impl headers should be read:\n{cold}"
    );
    assert!(
        !warm.contains("tcx::associated_type_value")
            && !warm.contains("tcx::fn_signature")
            && !warm.contains("tcx::relevant_trait_impls")
            && !warm.contains("tcx::inherent_method_candidates"),
        "the completed body and metadata should be reused:\n{warm}"
    );
}

#[test]
fn external_inherent_discovery_excludes_static_associated_functions() {
    let fixture = Fixture::SingleFile(
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../test-fixtures/solver/external_vec_inherent_methods.rs"),
    );

    let (_output, cold, warm) = combined::with_proxy_database(&fixture, |database, files| {
        let refs: Vec<_> = files
            .iter()
            .map(|(path, source)| (path.as_str(), source.as_str()))
            .collect();
        sage_test_harness::with_test_crate_files_twice_using_db(database, &refs, |db, root| {
            let function = root
                .expanded_module_items(db)
                .iter()
                .find_map(|symbol| match symbol.data(db) {
                    SymbolData::FnSymbol(FnSymbol::Local(function))
                        if function.name(db).text(db) == "takes_vec" =>
                    {
                        Some(function)
                    }
                    SymbolData::FnSymbol(_)
                    | SymbolData::StructSymbol(_)
                    | SymbolData::EnumSymbol(_)
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
                    | SymbolData::MacroInvocationSymbol(_) => None,
                })
                .expect("takes_vec function");
            let signature = function.sig(db);
            let (stash, signature) = signature.open();
            let [parameter] = &stash[signature.value.params] else {
                panic!("takes_vec must have one parameter")
            };
            let Ty::Adt(vector, _) = stash[*parameter] else {
                panic!("takes_vec parameter must be Vec<Frame>")
            };
            let SymbolData::StructSymbol(StructSymbol::Ext(vector)) = vector.data(db) else {
                panic!("Vec must be an external struct")
            };

            let new = sage_ir::external_syms::external_inherent_method_candidates(
                db,
                vector,
                Name::new(db, "new".to_owned()),
            );
            assert!(new.complete);
            assert!(
                new.candidates.is_empty(),
                "Vec::new has no receiver and cannot shadow a dot-call method"
            );

            let push = sage_ir::external_syms::external_inherent_method_candidates(
                db,
                vector,
                Name::new(db, "push".to_owned()),
            );
            assert!(push.complete);
            assert_eq!(push.candidates.len(), 1);
            assert!(push.candidates[0].externally_visible);
            assert_eq!(
                Symbol::from(push.candidates[0].function)
                    .name(db)
                    .unwrap()
                    .0
                    .text(db),
                "push"
            );
        })
    });

    assert_eq!(
        cold.matches("tcx::inherent_method_candidates").count(),
        2,
        "only the two requested Vec method names should be queried:\n{cold}"
    );
    assert!(!cold.contains("tcx::fn_signature"));
    assert!(!warm.contains("tcx::inherent_method_candidates"));
}

#[test]
fn option_ok_or_binds_owner_and_method_generics() {
    let fixture = Fixture::SingleFile(
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../test-fixtures/solver/external_option_ok_or.rs"),
    );

    let (diagnostics, cold, warm) = combined::with_proxy_database(&fixture, |database, files| {
        let refs: Vec<_> = files
            .iter()
            .map(|(path, source)| (path.as_str(), source.as_str()))
            .collect();
        sage_test_harness::with_test_crate_files_twice_using_db(database, &refs, |db, root| {
            let function = root
                .expanded_module_items(db)
                .iter()
                .find_map(|symbol| match symbol.data(db) {
                    SymbolData::FnSymbol(FnSymbol::Local(function))
                        if function.name(db).text(db) == "next_item" =>
                    {
                        Some(function)
                    }
                    SymbolData::FnSymbol(_)
                    | SymbolData::StructSymbol(_)
                    | SymbolData::EnumSymbol(_)
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
                    | SymbolData::MacroInvocationSymbol(_) => None,
                })
                .expect("next_item function");
            let declared = function.sig(db);
            let (declared_stash, declared) = declared.open();
            let Ty::Adt(expected_result, expected_result_args) = declared_stash[declared.value.ret]
            else {
                panic!("next_item must declare Result<Frame, ParseError>")
            };
            let [expected_frame, expected_error] = &declared_stash[expected_result_args] else {
                panic!("Result must have two type arguments")
            };
            let Ty::Adt(expected_frame, expected_frame_args) = declared_stash[*expected_frame]
            else {
                panic!("the success type must be Frame")
            };
            let Ty::Adt(expected_error, expected_error_args) = declared_stash[*expected_error]
            else {
                panic!("the error type must be ParseError")
            };
            assert!(declared_stash[expected_frame_args].is_empty());
            assert!(declared_stash[expected_error_args].is_empty());

            let checked = function.body(db);
            let diagnostics = checked
                .diagnostics
                .iter()
                .map(|diagnostic| diagnostic.render(db))
                .collect::<Vec<_>>();
            if !diagnostics.is_empty() {
                return diagnostics;
            }
            let (stash, body) = checked.body.open_deref();
            let TyExprData::Block(_, Some(ok_or)) = stash[body.root].data else {
                panic!("expected ok_or as the tail expression")
            };
            let TyExprData::ResolvedCall(ok_or_target, ok_or_arguments) = stash[ok_or].data else {
                panic!("ok_or must be a resolved call")
            };
            assert_eq!(
                Symbol::from(ok_or_target.function)
                    .name(db)
                    .unwrap()
                    .0
                    .text(db),
                "ok_or"
            );
            assert!(matches!(ok_or_target.dispatch, CallDispatch::Direct));
            let [next, error_argument] = &stash[ok_or_arguments] else {
                panic!("ok_or must contain its receiver and error argument")
            };
            assert!(matches!(stash[*error_argument].data, TyExprData::Path(_)));

            let Ty::Adt(result, result_args) = stash[stash[ok_or].ty] else {
                panic!("ok_or must return Result<Frame, ParseError>")
            };
            assert_eq!(result, expected_result);
            let [actual_frame, actual_error] = &stash[result_args] else {
                panic!("Result must retain both type arguments")
            };
            let Ty::Adt(actual_frame, actual_frame_args) = stash[*actual_frame] else {
                panic!("ok_or success type must be Frame")
            };
            let Ty::Adt(actual_error, actual_error_args) = stash[*actual_error] else {
                panic!("ok_or error type must be ParseError")
            };
            assert_eq!(actual_frame, expected_frame);
            assert_eq!(actual_error, expected_error);
            assert!(stash[actual_frame_args].is_empty());
            assert!(stash[actual_error_args].is_empty());

            let TyExprData::ResolvedCall(next_target, _) = stash[*next].data else {
                panic!("ok_or receiver must be the resolved Iterator::next call")
            };
            assert_eq!(
                Symbol::from(next_target.function)
                    .name(db)
                    .unwrap()
                    .0
                    .text(db),
                "next"
            );
            let Ty::Adt(option, option_args) = stash[stash[*next].ty] else {
                panic!("Iterator::next must produce Option<Frame>")
            };
            assert_eq!(Symbol::from(option).name(db).unwrap().0.text(db), "Option");
            let [option_frame] = &stash[option_args] else {
                panic!("Option must retain Frame")
            };
            let Ty::Adt(option_frame, option_frame_args) = stash[*option_frame] else {
                panic!("Option element must be Frame")
            };
            assert_eq!(option_frame, expected_frame);
            assert!(stash[option_frame_args].is_empty());

            let ok_or_signature = ok_or_target.function.sig(db).unwrap();
            assert_eq!(ok_or_signature.root().value.owner_generic_count, 1);
            assert_eq!(
                ok_or_signature.root().value.method_candidate_eligibility,
                SolverEligibility::Eligible
            );
            assert!(
                !ok_or_signature.root().value.const_call_complete,
                "rustc's Destruct condition is const-only"
            );
            let generic_names: Vec<_> = ok_or_signature
                .iter_symbols()
                .map(|generic| generic.name(db).unwrap().text(db).as_str())
                .collect();
            assert_eq!(generic_names, ["T", "E"]);
            diagnostics
        })
    });

    assert!(
        diagnostics.is_empty(),
        "{diagnostics:#?}\ncold trace:\n{cold}"
    );
    let inherent_lookups: Vec<_> = cold
        .lines()
        .filter(|line| line.contains("tcx::inherent_method_candidates"))
        .collect();
    assert_eq!(inherent_lookups.len(), 2, "one lookup per call:\n{cold}");
    assert!(
        inherent_lookups
            .iter()
            .any(|line| line.contains("\"next\""))
    );
    assert!(
        inherent_lookups
            .iter()
            .any(|line| line.contains("\"ok_or\""))
    );
    assert_eq!(
        cold.matches("tcx::fn_signature").count(),
        2,
        "only Iterator::next and Option::ok_or signatures should be read:\n{cold}"
    );
    assert_eq!(cold.matches("tcx::associated_type_value").count(), 1);
    assert_eq!(cold.matches("tcx::relevant_trait_impls").count(), 2);
    assert_eq!(cold.matches("tcx::impl_signature").count(), 2);
    assert!(
        !warm.contains("tcx::inherent_method_candidates")
            && !warm.contains("tcx::fn_signature")
            && !warm.contains("tcx::associated_type_value")
            && !warm.contains("tcx::relevant_trait_impls")
            && !warm.contains("tcx::impl_signature"),
        "the completed body and all selected metadata should be reused:\n{warm}"
    );
}
