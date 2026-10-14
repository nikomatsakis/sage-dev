#![feature(rustc_private)]
#![feature(register_tool)]
#![register_tool(sage)]

use sage_test_harness::compile_test;

#[test]
#[sage::test(crates.sage_test_harness.compile_tests.fixtures)]
fn ambiguous_module_files_are_not_silently_selected() {
    let result = std::panic::catch_unwind(|| {
        compile_test! {
            files: {
                "src/lib.rs": "mod child;",
                "src/child.rs": "fn first() {}",
                "src/child/mod.rs": "fn second() {}",
            },
        }
    });
    assert!(
        result.is_err(),
        "two candidate module files must fail instead of selecting one"
    );
}

#[test]
#[sage::test(crates.sage_test_harness.compile_tests.fixtures)]
fn explicit_module_path_selects_one_of_two_candidate_files() {
    compile_test! {
        files: {
            "src/lib.rs": "#[path = \"child.rs\"] mod child;",
            "src/child.rs": "fn first() -> u16 { 22 } //# ty",
            "src/child/mod.rs": "fn second() -> u16 { false }",
        },
        observations: expect_test::expect![[r#"
            src/child.rs: literal `22`
              type: u16
            src/child.rs: block `{ 22 }`
              type: u16
        "#]],
    }
}
