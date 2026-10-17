#![feature(rustc_private)]
#![feature(register_tool)]
#![register_tool(sage)]

#[test]
#[sage::test(crates.sage_test_harness.compile_tests.annotation_validation)]
#[should_panic(expected = "duplicate field: observations")]
fn duplicate_observation_expectations_are_rejected() {
    sage_test_harness::compile_test! {
        source: "fn value() -> u16 { 22 } //# ty",
        observations: expect_test::expect![""],
        observations: expect_test::expect![""],
    }
}
