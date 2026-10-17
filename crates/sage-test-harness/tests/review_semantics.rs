#![feature(rustc_private)]
#![feature(register_tool)]
#![register_tool(sage)]
use sage_test_harness::compile_test;

#[test]
#[sage::test(crates.sage_test_harness.compile_tests.diagnostic_counts)]
#[sage::test(crates.sage_test_harness.compile_tests.checking_scope)]
fn malformed_body_is_not_silently_accepted() {
    let result = std::panic::catch_unwind(|| {
        compile_test! { source: "fn broken() { let = ; }" }
    });
    let panic = result.unwrap_err();
    let message = panic.downcast_ref::<String>().unwrap();
    assert!(
        message.contains("Sage diagnostic markers do not match"),
        "{message}"
    );
}

#[test]
#[sage::test(crates.sage_test_harness.compile_tests.checking_scope)]
fn malformed_private_enum_signature_is_checked() {
    let result = std::panic::catch_unwind(|| {
        compile_test! { source: "enum Hidden { Value(Missing) }" }
    });
    let panic = result.unwrap_err();
    let message = panic.downcast_ref::<String>().unwrap();
    assert!(
        message.contains("Sage diagnostic markers do not match"),
        "{message}"
    );
    assert!(message.contains("unresolved type"), "{message}");
}

#[test]
#[sage::test(crates.sage_test_harness.compile_tests.rendering)]
fn distinct_same_named_types_keep_their_identity() {
    compile_test! {
        source: indoc::indoc! {r#"
            mod a { pub struct Value; }
            mod b { pub struct Value; }
            fn observe(a: a::Value, b: b::Value) -> (a::Value, b::Value) {
                (a, b) //# ty
            }
        "#},
        observations: expect_test::expect![[r#"
            src/lib.rs: path `a`
              type: a::Value
            src/lib.rs: path `b`
              type: b::Value
            src/lib.rs: tuple `(a, b)`
              type: (a::Value, b::Value)
        "#]],
    }
}

#[test]
#[sage::test(crates.sage_test_harness.compile_tests.query_projection)]
fn query_projection_preserves_structure_and_sorts_only_unordered_siblings() {
    use sage_ir::db::{InspectionChildOrder, InspectionEvent, InspectionSource};
    for order in [
        InspectionChildOrder::Sequential,
        InspectionChildOrder::Unordered,
    ] {
        let nodes = sage_host::trace::snapshot_trace(vec![
            InspectionEvent::SpanEnter {
                operation: "parent",
                source: InspectionSource::Sage,
                child_order: order,
            },
            InspectionEvent::QueryLeaf {
                key: "zeta(Id(42))".into(),
                disposition: salsa::QueryDisposition::Reused,
                observations: 3,
            },
            InspectionEvent::QueryLeaf {
                key: "alpha(Id(99))".into(),
                disposition: salsa::QueryDisposition::Executed,
                observations: 1,
            },
            InspectionEvent::SpanExit {
                operation: "parent",
            },
        ]);
        assert_eq!(nodes.len(), 1);
        assert_eq!(nodes[0].operation, "parent");
        assert_eq!(nodes[0].children.len(), 2);
        let operations: Vec<_> = nodes[0]
            .children
            .iter()
            .map(|node| node.operation.as_str())
            .collect();
        assert_eq!(
            operations,
            match order {
                InspectionChildOrder::Sequential => vec!["zeta", "alpha"],
                InspectionChildOrder::Unordered => vec!["alpha", "zeta"],
            }
        );
        let zeta = nodes[0]
            .children
            .iter()
            .find(|node| node.operation == "zeta")
            .unwrap();
        assert_eq!(zeta.observations, 3);
        let json = serde_json::to_value(zeta).unwrap();
        assert_eq!(json["disposition"], "reused");
        assert_eq!(json["key"]["kind"], "unmapped");
        assert_eq!(json["key"]["ingredient"], "zeta");
        assert!(!serde_json::to_string(&nodes).unwrap().contains("Id("));
    }
}

#[test]
#[sage::test(crates.sage_test_harness.compile_tests.fixtures)]
fn path_attribute_inside_inline_module_uses_its_module_directory() {
    compile_test! {
        files: {
            "src/lib.rs": "mod outer { #[path = \"renamed.rs\"] mod inner; }",
            "src/outer/renamed.rs": "fn value() -> u16 {\n22 //# ty\n}",
        },
        observations: expect_test::expect![[r#"
            src/outer/renamed.rs: literal `22`
              type: u16
        "#]],
    }
}
#[test]
#[sage::test(crates.sage_test_harness.compile_tests.checking_scope)]
fn enum_discriminant_body_is_not_silently_accepted() {
    let result = std::panic::catch_unwind(|| {
        // Account for rustc's independent error so it cannot mask Sage's omission.
        compile_test! { source: "enum Hidden { Value = missing() } //# ERROR if rustc" }
    });
    let panic = result.expect_err(
        "enum discriminant bodies must be checked or explicitly rejected as unsupported",
    );
    let message = panic.downcast_ref::<String>().unwrap();
    assert!(
        message.contains("Sage diagnostic markers do not match"),
        "{message}"
    );
    assert!(
        message.contains("whole-crate checking of enum discriminants is not supported"),
        "{message}"
    );
}

#[test]
#[sage::test(crates.sage_test_harness.compile_tests.checking_scope)]
fn valid_enum_discriminant_reports_sage_limitation() {
    compile_test! {
        source: "enum Hidden { Value = 1 } //# ERROR if SAGE",
        diagnostics: expect_test::expect![[r#"
            error: whole-crate checking of enum discriminants is not supported
             --> src/lib.rs:1:23
              |
            1 | enum Hidden { Value = 1 } //# ERROR if SAGE
              |                       ^"#]],
    }
}

#[test]
#[sage::test(crates.sage_test_harness.compile_tests.checking_scope)]
fn enum_discriminants_survive_syntax_token_rendering() {
    use sage_ir::symbol::{EnumSymbol, SymbolData};
    use sage_ir::tokens::{StringSink, ToTokens, TokenCtx};
    let source = "enum Hidden { First = 1 + 2, Second = 7 }";
    sage_test_harness::with_test_crate(source, |db, root| {
        let symbol = root.expanded_module_items(db)[0];
        let SymbolData::EnumSymbol(EnumSymbol::Local(enum_symbol)) = symbol.data(db) else {
            panic!("expected the source enum");
        };
        let (stash, cst) = enum_symbol.cst(db).open_deref();
        let variants = &stash[cst.variants];
        assert_eq!(variants.len(), 2);
        assert!(
            variants
                .iter()
                .all(|variant| variant.discriminant.is_some())
        );
        let mut sink = StringSink::new();
        cst.to_tokens(
            &TokenCtx {
                db,
                stash,
                source_text: source,
            },
            &mut sink,
        );
        assert_eq!(
            sink.into_string(),
            "enum Hidden {First = 1 + 2, Second = 7}"
        );
    });
}
