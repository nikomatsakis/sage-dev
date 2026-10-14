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
    assert!(
        result.is_err(),
        "malformed source must emit a diagnostic or fail checking"
    );
}

#[test]
#[sage::test(crates.sage_test_harness.compile_tests.checking_scope)]
fn malformed_private_enum_signature_is_checked() {
    let result = std::panic::catch_unwind(|| {
        compile_test! { source: "enum Hidden { Value(Missing) }" }
    });
    assert!(result.is_err(), "private enum signature must be checked");
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
