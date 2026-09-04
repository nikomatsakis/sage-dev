#![feature(rustc_private)]

use std::path::PathBuf;

use rust_ref::{Expr, Item, Type};
use sage_ir::cst::Mutability;
use sage_ir::name::Name;
use sage_ir::symbol::{Symbol, SymbolData};
use sage_ir::ty::{FloatTy, IntTy, Lifetime, Ty, UintTy};
use sage_ir::tytree::{CallDispatch, TyExprData};
use sage_oracle_harness::{Fixture, combined};
use sage_test_harness::local_function_named;

fn fixture() -> Fixture {
    Fixture::SingleFile(
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../test-fixtures/solver/binary_operators.rs"),
    )
}

#[test]
fn oracle_projects_operators_to_the_same_typed_tree_as_sage() {
    let fixture = fixture();
    let (oracle, sage) = combined::run_combined(&fixture);
    let oracle = oracle.expect("oracle analysis failed");
    for name in [
        "primitive_add",
        "add_literal",
        "primitive_shift",
        "equal",
        "less",
        "lazy",
        "custom",
    ] {
        let oracle_function = oracle
            .root
            .items
            .iter()
            .find_map(|item| match item {
                Item::Fn(function) => (function.name == name).then_some(function),
                Item::Struct(_) | Item::Enum(_) | Item::Mod(_) => None,
            })
            .unwrap_or_else(|| panic!("oracle omitted operator function `{name}`"));
        let sage_function = sage
            .root
            .items
            .iter()
            .find_map(|item| match item {
                Item::Fn(function) => (function.name == name).then_some(function),
                Item::Struct(_) | Item::Enum(_) | Item::Mod(_) => None,
            })
            .unwrap_or_else(|| panic!("Sage omitted operator function `{name}`"));
        assert_eq!(
            oracle_function, sage_function,
            "operator function `{name}` diverged between oracle and Sage"
        );
    }
}

#[test]
fn oracle_projects_comparison_with_never_to_any_adjustment() {
    let oracle = fixture()
        .oracle_output()
        .expect("oracle analysis of binary-operator fixture failed");
    let function = oracle
        .root
        .items
        .iter()
        .find_map(|item| match item {
            Item::Fn(function) => (function.name == "never_to_any_comparison").then_some(function),
            Item::Struct(_) | Item::Enum(_) | Item::Mod(_) => None,
        })
        .expect("oracle omitted the NeverToAny comparison function");
    let Some(Expr::Block {
        tail: Some(tail), ..
    }) = function.body.as_ref()
    else {
        panic!("expected a tail expression")
    };
    let Expr::Call {
        owner_type_args,
        args,
        ..
    } = tail.as_ref()
    else {
        panic!("comparison must be projected as a resolved call")
    };
    let [Type::Primitive(left_owner), Type::Primitive(right_owner)] = owner_type_args.as_slice()
    else {
        panic!("comparison must retain concrete Self and Rhs owner arguments")
    };
    assert_eq!(left_owner, "u32");
    assert_eq!(right_owner, "u32");
    let [
        Expr::Ref { mutable: false, .. },
        Expr::Ref {
            mutable: false,
            expr: right,
            ..
        },
    ] = args.as_slice()
    else {
        panic!("comparison must pass two shared-borrowed operands")
    };
    assert!(
        matches!(right.as_ref(), Expr::NeverToAny { .. }),
        "the diverging right operand must retain its NeverToAny adjustment"
    );
}

#[test]
fn primitive_operators_use_core_impls_and_resolved_calls() {
    let (diagnostics, cold, warm) = combined::with_proxy_database(&fixture(), |database, files| {
        let refs: Vec<_> = files
            .iter()
            .map(|(path, source)| (path.as_str(), source.as_str()))
            .collect();
        sage_test_harness::with_test_crate_files_twice_using_db(database, &refs, |db, root| {
            assert_value_operator(
                db,
                root,
                "primitive_add",
                "Add",
                "add",
                Ty::Uint(UintTy::U32),
                Ty::Uint(UintTy::U32),
                Ty::Uint(UintTy::U32),
            );
            assert_value_operator(
                db,
                root,
                "primitive_shift",
                "Shl",
                "shl",
                Ty::Uint(UintTy::U8),
                Ty::Int(IntTy::I16),
                Ty::Uint(UintTy::U8),
            );
            ["primitive_add", "primitive_shift"]
                .into_iter()
                .flat_map(|name| {
                    local_function_named(db, root, name)
                        .body(db)
                        .diagnostics
                        .clone()
                })
                .map(|diagnostic| diagnostic.render(db))
                .collect::<Vec<_>>()
        })
    });

    assert!(
        diagnostics.is_empty(),
        "{diagnostics:#?}\ncold trace:\n{cold}\nwarm trace:\n{warm}"
    );
    assert!(
        cold.contains("tcx::relevant_trait_impls")
            && cold.contains("tcx::impl_signature")
            && cold.contains("tcx::associated_type_value")
            && cold.contains("tcx::fn_signature"),
        "operator checking must read ordinary core impl metadata:\n{cold}"
    );
    assert!(
        !warm.contains("tcx::relevant_trait_impls")
            && !warm.contains("tcx::impl_signature")
            && !warm.contains("tcx::associated_type_value")
            && !warm.contains("tcx::fn_signature"),
        "the completed bodies must reuse their semantic dependencies:\n{warm}"
    );
}

#[test]
fn represented_float_operator_impls_resolve() {
    const SOURCE: &str = "
        fn float32_add(left: f32, right: f32) -> f32 { left + right }
        fn float64_add(left: f64, right: f64) -> f64 { left + right }
    ";
    let ((), cold, warm) = combined::with_proxy_database(&fixture(), |database, _| {
        sage_test_harness::with_test_crate_files_twice_using_db(
            database,
            &[("lib.rs", SOURCE)],
            |db, root| {
                assert_value_operator(
                    db,
                    root,
                    "float32_add",
                    "Add",
                    "add",
                    Ty::Float(FloatTy::F32),
                    Ty::Float(FloatTy::F32),
                    Ty::Float(FloatTy::F32),
                );
                assert_value_operator(
                    db,
                    root,
                    "float64_add",
                    "Add",
                    "add",
                    Ty::Float(FloatTy::F64),
                    Ty::Float(FloatTy::F64),
                    Ty::Float(FloatTy::F64),
                );
            },
        )
    });

    assert!(
        cold.contains("tcx::relevant_trait_impls")
            && cold.contains("Some(Float32)")
            && cold.contains("Some(Float64)"),
        "represented float operator lookups must use width-specific partitions:\n{cold}"
    );
    assert!(
        !warm.contains("tcx::relevant_trait_impls"),
        "the completed float operator body must reuse its semantic dependencies:\n{warm}"
    );
}

#[test]
fn selected_operator_impl_narrows_unsuffixed_integer_literal() {
    let (diagnostics, cold, warm) = combined::with_proxy_database(&fixture(), |database, files| {
        let refs: Vec<_> = files
            .iter()
            .map(|(path, source)| (path.as_str(), source.as_str()))
            .collect();
        sage_test_harness::with_test_crate_files_twice_using_db(database, &refs, |db, root| {
            let function = local_function_named(db, root, "add_literal");
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
                panic!("expected a tail expression")
            };
            let TyExprData::ResolvedCall(target, arguments) = stash[call].data else {
                panic!("addition must be a resolved call")
            };
            assert_symbol_name(db, target.function.into(), "add");
            let CallDispatch::StaticTrait { trait_ref, .. } = target.dispatch else {
                panic!("addition must use static trait dispatch")
            };
            assert_symbol_name(db, trait_ref.trait_sym.into(), "Add");

            let [_, right] = stash[arguments] else {
                panic!("addition must pass two operands")
            };
            assert!(
                matches!(stash[right].data, TyExprData::Literal(_)),
                "the right operand must remain the source literal"
            );
            assert_eq!(
                stash[stash[right].ty],
                Ty::Uint(UintTy::U32),
                "the selected u32 Add impl must narrow the unsuffixed literal"
            );
            assert_eq!(stash[stash[call].ty], Ty::Uint(UintTy::U32));
            diagnostics
        })
    });

    assert!(
        diagnostics.is_empty(),
        "{diagnostics:#?}\ncold trace:\n{cold}\nwarm trace:\n{warm}"
    );
}

#[test]
fn comparisons_materialize_signature_required_shared_borrows() {
    let (diagnostics, _cold, _warm) =
        combined::with_proxy_database(&fixture(), |database, files| {
            let refs: Vec<_> = files
                .iter()
                .map(|(path, source)| (path.as_str(), source.as_str()))
                .collect();
            sage_test_harness::with_test_crate_files_twice_using_db(database, &refs, |db, root| {
                for (function_name, trait_name, method_name) in
                    [("equal", "PartialEq", "eq"), ("less", "PartialOrd", "lt")]
                {
                    let function = local_function_named(db, root, function_name);
                    let checked = function.body(db);
                    assert!(checked.diagnostics.is_empty(), "{:?}", checked.diagnostics);
                    let (stash, body) = checked.body.open_deref();
                    let TyExprData::Block(_, Some(call)) = stash[body.root].data else {
                        panic!("expected a tail expression")
                    };
                    let TyExprData::ResolvedCall(target, arguments) = stash[call].data else {
                        panic!("comparison must be a resolved call")
                    };
                    assert_symbol_name(db, target.function.into(), method_name);
                    let CallDispatch::StaticTrait { trait_ref, .. } = target.dispatch else {
                        panic!("comparison must use static trait dispatch")
                    };
                    assert_symbol_name(db, trait_ref.trait_sym.into(), trait_name);
                    assert!(matches!(stash[stash[call].ty], Ty::Bool));
                    let [left, right] = stash[arguments] else {
                        panic!("comparison must pass two operands")
                    };
                    for operand in [left, right] {
                        assert!(matches!(
                            stash[operand].data,
                            TyExprData::Ref(_, Mutability::Shared)
                        ));
                        assert!(matches!(
                            stash[stash[operand].ty],
                            Ty::Ref(_, Mutability::Shared, Lifetime::Dummy)
                        ));
                    }
                }
                Vec::<String>::new()
            })
        });
    assert!(diagnostics.is_empty());
}

#[test]
fn local_operator_impl_can_choose_distinct_rhs_and_output() {
    let (diagnostics, cold, _warm) =
        combined::with_proxy_database(&fixture(), |database, files| {
            let refs: Vec<_> = files
                .iter()
                .map(|(path, source)| (path.as_str(), source.as_str()))
                .collect();
            sage_test_harness::with_test_crate_files_twice_using_db(database, &refs, |db, root| {
                let function = local_function_named(db, root, "custom");
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
                    panic!("expected a tail expression")
                };
                let TyExprData::ResolvedCall(target, _) = stash[call].data else {
                    panic!("custom addition must be a resolved call")
                };
                assert_symbol_name(db, target.function.into(), "add");
                let [left, right] = stash[target.owner_type_args] else {
                    panic!("Add must retain Self and Rhs")
                };
                let Ty::Adt(left, _) = stash[left] else {
                    panic!("Self must be Left")
                };
                let Ty::Adt(right, _) = stash[right] else {
                    panic!("Rhs must be Right")
                };
                let Ty::Adt(output, _) = stash[stash[call].ty] else {
                    panic!("Output must be Sum")
                };
                assert_symbol_name(db, left, "Left");
                assert_symbol_name(db, right, "Right");
                assert_symbol_name(db, output, "Sum");
                diagnostics
            })
        });
    assert!(diagnostics.is_empty(), "{diagnostics:#?}\n{cold}");
    assert!(
        cold.contains("Some(LocalAdt)"),
        "local nominal types need a completeness-preserving external impl partition:\n{cold}"
    );
}

#[test]
fn failed_operator_proof_becomes_one_diagnostic_and_an_error_node() {
    const SOURCE: &str = "fn bad(left: bool, right: bool) { let _ = left + right; }";
    let (result, _cold, _warm) = combined::with_proxy_database(&fixture(), |database, _| {
        sage_test_harness::with_test_crate_files_twice_using_db(
            database,
            &[("lib.rs", SOURCE)],
            |db, root| {
                let checked = local_function_named(db, root, "bad").body(db);
                let rendered: Vec<_> = checked
                    .diagnostics
                    .iter()
                    .map(|diagnostic| diagnostic.render(db))
                    .collect();
                let (stash, body) = checked.body.open_deref();
                let TyExprData::Block(statements, None) = stash[body.root].data else {
                    panic!("expected a statement-only body")
                };
                let expression = match stash[statements][0].kind {
                    sage_ir::tytree::TyStmtKind::Let(_, _, Some(expression)) => expression,
                    _ => panic!("expected a let initializer"),
                };
                (
                    rendered,
                    matches!(stash[expression].data, TyExprData::Error(_)),
                )
            },
        )
    });
    assert_eq!(result.0.len(), 1, "{:#?}", result.0);
    assert!(result.0[0].contains("associated type relation is not satisfied"));
    assert!(
        result.1,
        "failed proof must not leave a successful-looking call"
    );
}

#[test]
fn rigid_mixed_rhs_operator_reports_one_diagnostic_and_recovers_to_error() {
    const SOURCE: &str = "fn bad(left: u32, right: bool) -> u32 { left + right }";
    let (result, _cold, _warm) = combined::with_proxy_database(&fixture(), |database, _| {
        sage_test_harness::with_test_crate_files_twice_using_db(
            database,
            &[("lib.rs", SOURCE)],
            |db, root| {
                let checked = local_function_named(db, root, "bad").body(db);
                let rendered: Vec<_> = checked
                    .diagnostics
                    .iter()
                    .map(|diagnostic| diagnostic.render(db))
                    .collect();
                let (stash, body) = checked.body.open_deref();
                let TyExprData::Block(_, Some(tail)) = stash[body.root].data else {
                    panic!("expected a tail expression")
                };
                (rendered, matches!(stash[tail].data, TyExprData::Error(_)))
            },
        )
    });

    assert_eq!(result.0.len(), 1, "{:#?}", result.0);
    assert!(
        result.0[0].contains("associated type relation is not satisfied"),
        "{}",
        result.0[0]
    );
    assert!(
        result.1,
        "failed mixed-Rhs proof must recover the operator call to Error"
    );
}

#[test]
fn rigid_return_comparison_recovers_from_ambiguous_trait_obligation() {
    let (result, _cold, _warm) = combined::with_proxy_database(&fixture(), |database, files| {
        let mut source = files
            .into_iter()
            .find_map(|(path, source)| (path == "lib.rs").then_some(source))
            .expect("fixture must contain lib.rs");
        source.push_str(
            "\nfn invalid_comparison(left: Left, right: Right) -> bool { left < right }\n",
        );
        sage_test_harness::with_test_crate_files_twice_using_db(
            database,
            &[("lib.rs", source.as_str())],
            |db, root| {
                let checked = local_function_named(db, root, "invalid_comparison").body(db);
                let rendered: Vec<_> = checked
                    .diagnostics
                    .iter()
                    .map(|diagnostic| diagnostic.render(db))
                    .collect();
                let (stash, body) = checked.body.open_deref();
                let TyExprData::Block(_, Some(tail)) = stash[body.root].data else {
                    panic!("expected a tail expression")
                };
                (rendered, matches!(stash[tail].data, TyExprData::Error(_)))
            },
        )
    });

    assert_eq!(result.0.len(), 1, "{:#?}", result.0);
    assert!(
        result.0[0].contains("trait obligation could not be resolved"),
        "{}",
        result.0[0]
    );
    assert!(
        result.1,
        "ambiguous comparison proof must recover the operator call to Error"
    );
}

#[test]
fn one_failed_operator_result_does_not_poison_an_equivalent_call() {
    const SOURCE: &str =
        "fn split(left: u32, right: u32) -> (bool, u32) { (left + right, left + right) }";
    let (result, _cold, _warm) = combined::with_proxy_database(&fixture(), |database, _| {
        sage_test_harness::with_test_crate_files_twice_using_db(
            database,
            &[("lib.rs", SOURCE)],
            |db, root| {
                let checked = local_function_named(db, root, "split").body(db);
                let rendered: Vec<_> = checked
                    .diagnostics
                    .iter()
                    .map(|diagnostic| diagnostic.render(db))
                    .collect();
                let (stash, body) = checked.body.open_deref();
                let TyExprData::Block(_, Some(tuple)) = stash[body.root].data else {
                    panic!("expected a tail tuple")
                };
                let TyExprData::Tuple(elements) = stash[tuple].data else {
                    panic!("expected a tuple expression")
                };
                let [failed, successful] = stash[elements] else {
                    panic!("expected two tuple elements")
                };
                (
                    rendered,
                    matches!(stash[failed].data, TyExprData::Error(_)),
                    matches!(stash[successful].data, TyExprData::ResolvedCall(_, _)),
                    matches!(stash[stash[successful].ty], Ty::Uint(UintTy::U32)),
                )
            },
        )
    });

    assert_eq!(result.0.len(), 1, "{:#?}", result.0);
    assert!(
        result.1,
        "the incompatible first result must recover to Error"
    );
    assert!(
        result.2,
        "failure recovery must remain scoped to its source expression"
    );
    assert!(result.3, "the successful call must retain its u32 result");
}

#[test]
fn failed_call_does_not_poison_a_call_sharing_a_method_requirement() {
    const SOURCE: &str =
        "fn split(left: bool, right: u32) -> u32 { let _ = left + right; right + right }";
    let (result, _cold, _warm) = combined::with_proxy_database(&fixture(), |database, _| {
        sage_test_harness::with_test_crate_files_twice_using_db(
            database,
            &[("lib.rs", SOURCE)],
            |db, root| {
                let checked = local_function_named(db, root, "split").body(db);
                let rendered: Vec<_> = checked
                    .diagnostics
                    .iter()
                    .map(|diagnostic| diagnostic.render(db))
                    .collect();
                let (stash, body) = checked.body.open_deref();
                let TyExprData::Block(statements, Some(successful)) = stash[body.root].data else {
                    panic!("expected a let followed by a tail call")
                };
                let failed = match stash[statements][0].kind {
                    sage_ir::tytree::TyStmtKind::Let(_, _, Some(expression)) => expression,
                    _ => panic!("expected a let initializer"),
                };
                (
                    rendered,
                    matches!(stash[failed].data, TyExprData::Error(_)),
                    matches!(stash[successful].data, TyExprData::ResolvedCall(_, _)),
                )
            },
        )
    });

    assert_eq!(result.0.len(), 1, "{:#?}", result.0);
    assert!(result.1, "the invalid call must recover to Error");
    assert!(
        result.2,
        "deduplicated method requirements must not share recovery scopes"
    );
}

#[test]
fn unknown_operator_self_reports_current_missing_candidate_hint_once() {
    // Known deviation: Sage does not yet use the available impl set to narrow
    // an unknown operator Self, so the integer variable remains unresolved.
    const SOURCE: &str = "fn bad() { let _ = 1 + true; }";
    let (result, _cold, _warm) = combined::with_proxy_database(&fixture(), |database, _| {
        sage_test_harness::with_test_crate_files_twice_using_db(
            database,
            &[("lib.rs", SOURCE)],
            |db, root| {
                let checked = local_function_named(db, root, "bad").body(db);
                let rendered: Vec<_> = checked
                    .diagnostics
                    .iter()
                    .map(|diagnostic| diagnostic.render(db))
                    .collect();
                let (stash, body) = checked.body.open_deref();
                let TyExprData::Block(statements, None) = stash[body.root].data else {
                    panic!("expected a statement-only body")
                };
                let expression = match stash[statements][0].kind {
                    sage_ir::tytree::TyStmtKind::Let(_, _, Some(expression)) => expression,
                    _ => panic!("expected a let initializer"),
                };
                (
                    rendered,
                    matches!(stash[expression].data, TyExprData::Error(_)),
                )
            },
        )
    });

    assert_eq!(result.0.len(), 1, "{:#?}", result.0);
    assert!(
        result.0[0].contains("could not infer an integer type"),
        "without candidate-set narrowing, the unknown Self remains an integer variable: {}",
        result.0[0]
    );
    assert!(
        result.1,
        "terminal ambiguity must recover the provisional operator call"
    );
}

#[test]
fn lazy_boolean_operators_remain_control_flow_nodes() {
    assert_lazy_boolean_operator("fn lazy(a: bool, b: bool) -> bool { a && b }");
}

#[test]
fn lazy_boolean_or_remains_control_flow_without_trait_lookup() {
    assert_lazy_boolean_operator("fn lazy(a: bool, b: bool) -> bool { a || b }");
}

fn assert_lazy_boolean_operator(source: &str) {
    let (result, cold, warm) = combined::with_proxy_database(&fixture(), |database, _| {
        sage_test_harness::with_test_crate_files_twice_using_db(
            database,
            &[("lib.rs", source)],
            |db, root| {
                let checked = local_function_named(db, root, "lazy").body(db);
                assert!(checked.diagnostics.is_empty());
                let (stash, body) = checked.body.open_deref();
                let TyExprData::Block(_, Some(tail)) = stash[body.root].data else {
                    panic!("expected a tail expression")
                };
                matches!(stash[tail].data, TyExprData::Binary(_, _, _))
            },
        )
    });
    assert!(result);
    for trace in [cold, warm] {
        assert!(!trace.contains("tcx::relevant_trait_impls"), "{trace}");
        assert!(!trace.contains("tcx::associated_items"), "{trace}");
    }
}

#[test]
fn lazy_boolean_operators_reject_non_boolean_operands_without_trait_lookup() {
    const SOURCE: &str = "fn lazy(left: u32, right: u32) -> bool { left && right }";
    let (result, cold, warm) = combined::with_proxy_database(&fixture(), |database, _| {
        sage_test_harness::with_test_crate_files_twice_using_db(
            database,
            &[("lib.rs", SOURCE)],
            |db, root| {
                let checked = local_function_named(db, root, "lazy").body(db);
                let rendered: Vec<_> = checked
                    .diagnostics
                    .iter()
                    .map(|diagnostic| diagnostic.render(db))
                    .collect();
                let (stash, body) = checked.body.open_deref();
                let TyExprData::Block(_, Some(tail)) = stash[body.root].data else {
                    panic!("expected a tail expression")
                };
                (rendered, matches!(stash[tail].data, TyExprData::Error(_)))
            },
        )
    });

    assert_eq!(result.0.len(), 2, "{:#?}", result.0);
    assert!(
        result
            .0
            .iter()
            .all(|diagnostic| diagnostic.contains("expected `bool`, found `u32`")),
        "{:#?}",
        result.0
    );
    assert!(result.1, "invalid lazy operation must recover to Error");
    for trace in [cold, warm] {
        assert!(!trace.contains("tcx::relevant_trait_impls"), "{trace}");
        assert!(!trace.contains("tcx::associated_items"), "{trace}");
    }
}

fn assert_value_operator<'db>(
    db: &'db dyn sage_ir::Db,
    root: sage_ir::symbol::ModSymbol<'db>,
    function_name: &str,
    trait_name: &str,
    method_name: &str,
    left_ty: Ty<'db>,
    right_ty: Ty<'db>,
    output_ty: Ty<'db>,
) {
    let function = local_function_named(db, root, function_name);
    let checked = function.body(db);
    assert!(checked.diagnostics.is_empty(), "{:?}", checked.diagnostics);
    let (stash, body) = checked.body.open_deref();
    let TyExprData::Block(_, Some(call)) = stash[body.root].data else {
        panic!("expected a tail expression")
    };
    let TyExprData::ResolvedCall(target, arguments) = stash[call].data else {
        panic!("operator must be a resolved call")
    };
    assert_symbol_name(db, target.function.into(), method_name);
    let CallDispatch::StaticTrait { trait_ref, .. } = target.dispatch else {
        panic!("operator must use static trait dispatch")
    };
    assert_symbol_name(db, trait_ref.trait_sym.into(), trait_name);
    let [left, right] = stash[target.owner_type_args] else {
        panic!("operator must retain Self and Rhs")
    };
    assert_eq!(stash[left], left_ty);
    assert_eq!(stash[right], right_ty);
    assert_eq!(stash[stash[call].ty], output_ty);
    assert_eq!(stash[arguments].len(), 2);
}

fn assert_symbol_name<'db>(db: &'db dyn sage_ir::Db, symbol: Symbol<'db>, expected: &str) {
    let (name, _) = symbol.name(db).expect("symbol must have a name");
    assert_eq!(name, Name::new(db, expected.to_owned()));
    assert!(!matches!(symbol.data(db), SymbolData::UseSymbol(_)));
}
