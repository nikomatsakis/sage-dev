#![feature(rustc_private)]
#![feature(register_tool)]
#![register_tool(sage)]
use expect_test::expect;
use indoc::indoc;
use sage_test_harness::compile_test;

#[test]
#[sage::test(crates.sage_test_harness.compile_tests.grouped_observations)]
#[sage::test(crates.sage_test_harness.compile_tests.resolved_symbols)]
fn symbols_and_types_share_expression_headers() {
    compile_test! {
        source: indoc! {r#"
            mod module { pub fn value() -> u16 { 22 } }
            fn example() -> u16 {
                module::value() //# ty
                //# sym
                //# ty
                //# sym
            }
        "#},
        observations: expect![[r#"
            src/lib.rs: path `module::value`
              symbol: fn crate::module::value
              type: fn() -> u16
            src/lib.rs: call `module::value()`
              type: u16
        "#]],
    }
}

#[test]
#[sage::test(crates.sage_test_harness.compile_tests.grouped_observations)]
fn reverse_marker_order_has_the_same_output() {
    compile_test! {
        source: indoc! {r#"
            mod module { pub fn value() -> u16 { 22 } }
            fn example() -> u16 {
                module::value() //# sym
                //# ty
            }
        "#},
        observations: expect![[r#"
            src/lib.rs: path `module::value`
              symbol: fn crate::module::value
              type: fn() -> u16
            src/lib.rs: call `module::value()`
              type: u16
        "#]],
    }
}

#[test]
#[sage::test(crates.sage_test_harness.compile_tests.resolved_symbols)]
fn aliases_and_qualified_paths_identify_the_definition() {
    compile_test! {
        source: indoc! {r#"
            mod first { pub fn value() -> u16 { 1 } }
            mod second { pub fn value() -> u16 { 2 } }
            use first::value as alias;
            fn example() -> u16 {
                let first = alias(); //# sym
                second::value() //# sym
            }
        "#},
        observations: expect![[r#"
            src/lib.rs: path `alias`
              symbol: fn crate::first::value
            src/lib.rs: path `second::value`
              symbol: fn crate::second::value
        "#]],
    }
}

#[test]
#[sage::test(crates.sage_test_harness.compile_tests.resolved_symbols)]
fn shadowed_locals_have_distinct_stable_identities() {
    compile_test! {
        source: indoc! {r#"
            fn example(value: u16) -> u16 {
                let observed = value; //# sym
                let unrelated = 0u16;
                let value = value; //# sym
                let observed = value; //# sym
                value //# sym
            }
        "#},
        observations: expect![[r#"
            src/lib.rs: path `value`
              symbol: local value#1
            src/lib.rs: path `value`
              symbol: local value#1
            src/lib.rs: path `value`
              symbol: local value#2
            src/lib.rs: path `value`
              symbol: local value#2
        "#]],
    }
}

#[test]
#[sage::test(crates.sage_test_harness.compile_tests.resolved_symbols)]
fn external_definitions_use_metadata_paths() {
    compile_test! {
        source: indoc! {r#"
            fn example() -> std::cmp::Ordering {
                std::cmp::Ordering::Less //# sym
            }
        "#},
        observations: expect![[r#"
            src/lib.rs: path `std::cmp::Ordering::Less`
              symbol: variant std::cmp::Ordering::Less
        "#]],
    }
}

#[test]
#[sage::test(crates.sage_test_harness.compile_tests.resolved_symbols)]
fn symbol_selection_excludes_field_names_types_and_let_patterns() {
    compile_test! {
        source: indoc! {r#"
            struct Record { field: u16 }
            fn example(record: Record) -> u16 {
                let field: u16 = record.field; //# sym
                field
            }
        "#},
        observations: expect![[r#"
            src/lib.rs: path `record`
              symbol: local record#1
        "#]],
    }
}
