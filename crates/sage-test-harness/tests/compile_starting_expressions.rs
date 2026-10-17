#![feature(rustc_private)]
#![feature(register_tool)]
#![register_tool(sage)]

use expect_test::expect;
use sage_test_harness::compile_test;

#[test]
#[sage::test(crates.sage_test_harness.compile_tests.starting_expressions)]
fn files_lines_and_identical_siblings_keep_source_order() {
    compile_test! {
        files: {
            "src/z.rs": "fn z() -> u8 {\n 3 //# ty\n}",
            "src/lib.rs": "mod z; mod a;",
            "src/a.rs": "fn a() -> (u8, (bool, u8)) {\n (1, (true, 1)) //# ty\n //# ty\n}\nfn b() -> u8 {\n 2 //# ty\n}",
        },
        observations: expect![[r#"
            src/a.rs: literal `1`
              type: u8
            src/a.rs: literal `true`
              type: bool
            src/a.rs: literal `1`
              type: u8
            src/a.rs: tuple `(true, 1)`
              type: (bool, u8)
            src/a.rs: tuple `(1, (true, 1))`
              type: (u8, (bool, u8))
            src/a.rs: literal `2`
              type: u8
            src/z.rs: literal `3`
              type: u8
        "#]],
    }
}

#[test]
#[sage::test(crates.sage_test_harness.compile_tests.starting_expressions)]
#[should_panic(expected = "no expression with a completed type starts on this line")]
fn function_declaration_is_not_an_expression() {
    compile_test! {
        source: "fn f() //# ty\n{ }",
        observations: expect![""],
    }
}
