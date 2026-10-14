#![feature(rustc_private)]
#![feature(register_tool)]
#![register_tool(sage)]
use expect_test::expect;
use indoc::indoc;
use sage_test_harness::compile_test;

#[test]
#[sage::test(crates.sage_test_harness.compile_tests.invocation)]
#[sage::test(crates.sage_test_harness.compile_tests.starting_expressions)]
#[sage::test(crates.sage_test_harness.compile_tests.completed_types)]
fn call_argument_types() {
    compile_test! {
        source: indoc! {r#"
            fn inner(value: u16) -> u32 { value as u32 }
            fn outer(first: u32, second: u16) -> u64 { first as u64 + second as u64 }
            fn example() -> u64 {
                outer(
                    inner(22u16), //# ty
                    33u16,
                )
            }
        "#},
        types: expect![[r#"
            src/lib.rs: path `inner`: fn(u16) -> u32
            src/lib.rs: literal `22u16`: u16
            src/lib.rs: call `inner(22u16)`: u32
        "#]],
        diagnostics: expect![""],
    }
}

#[test]
#[sage::test(crates.sage_test_harness.compile_tests.dependencies)]
fn real_std() {
    compile_test! {
        source: "fn example(value: std::string::String) {}",
        diagnostics: expect![""],
    }
}

#[test]
#[sage::test(crates.sage_test_harness.compile_tests.fixtures)]
fn modules_and_associated_bodies() {
    compile_test! {
        files: {
            "src/lib.rs": "mod nested;",
            "src/nested/mod.rs": indoc! {r#"
                struct Value;
                impl Value {
                    fn answer() -> u16 {
                        22 //# ty
                    }
                }
            "#},
        },
        types: expect![[r#"
            src/nested/mod.rs: literal `22`: u16
        "#]],
    }
}

#[test]
#[sage::test(crates.sage_test_harness.compile_tests.production_execution)]
#[sage::test(crates.sage_test_harness.compile_tests.diagnostic_counts)]
#[sage::test(crates.sage_test_harness.compile_tests.diagnostic_locations)]
fn invalid_source_reaches_sage() {
    compile_test! {
        source: indoc! {r#"
            fn example() -> u16 {
                true //# ERROR
            }
        "#},
        diagnostics: expect![[r#"
            error: type mismatch: expected `u16`, found `bool`
             --> src/lib.rs:1:21
              |
            1 |   fn example() -> u16 {
              |  _________________---_^
              | |                 |
              | |                 expected `u16` because of return type
            2 | |     true //# ERROR
            3 | | }
              | | ^
              | |_|
              |   found `bool`"#]],
    }
}

#[test]
#[sage::test(crates.sage_test_harness.compile_tests.compiler_conditions)]
#[sage::test(crates.sage_test_harness.compile_tests.edition)]
fn explicit_edition_and_inactive_rustc_marker() {
    compile_test! {
        diagnostics: expect![""],
        edition: "2021",
        source: "fn example() {} //# ERROR if rustc",
    }
}

#[test]
#[sage::test(crates.sage_test_harness.compile_tests.starting_expressions)]
#[sage::test(crates.sage_test_harness.compile_tests.rendering)]
fn tuples_and_multiline_descriptions() {
    compile_test! {
        source: indoc! {r#"
            fn example() -> (u16, bool) {
                (22, true) //# ty
                //# ty
            }
            fn multiline() -> (u16, bool) {
                ( //# ty
                    22,
                    true,
                )
            }
        "#},
        types: expect![[r#"
            src/lib.rs: literal `22`: u16
            src/lib.rs: literal `true`: bool
            src/lib.rs: tuple `(22, true)`: (u16, bool)
            src/lib.rs: tuple `( 22, true, )`: (u16, bool)
        "#]],
    }
}

#[test]
#[sage::test(crates.sage_test_harness.compile_tests.completed_types)]
fn coercion_wrapper_follows_its_source_expression() {
    compile_test! {
        source: indoc! {r#"
            fn example() -> u16 {
                return 22; //# ty
            }
        "#},
        types: expect![[r#"
            src/lib.rs: literal `22`: u16
            src/lib.rs: return `return 22`: !
            src/lib.rs: coercion `return 22`: u16
        "#]],
    }
}

#[test]
#[sage::test(crates.sage_test_harness.compile_tests.blessing_gate)]
fn blessing_gate_is_justified_by_implementation_structure() {
    // Implementation-only justification, not executable behavioral evidence:
    // CompileTest::prepare returns all validated, rendered results or an error.
    // It never invokes expect-test assertions. CompileTest::run stops on that
    // error; only its success branch can compare (and possibly update) values.
    // Review the associated implementation and this explanatory no-op together.
    // This structural argument avoids failure-injection tests at every stage.
}

#[test]
#[sage::test(crates.sage_test_harness.compile_tests.diagnostic_counts)]
#[sage::test(crates.sage_test_harness.compile_tests.checking_scope)]
fn signature_diagnostics_are_retained_once() {
    compile_test! {
        source: indoc! {r#"
            fn broken(value: Missing) {} //# ERROR
            fn one() { broken(true); }
            fn two() { broken(false); }
        "#},
        diagnostics: expect![[r#"
            error: unresolved type
             --> src/lib.rs:1:18
              |
            1 | fn broken(value: Missing) {} //# ERROR
              |                  ^^^^^^^"#]],
    }
}

#[test]
#[sage::test(crates.sage_test_harness.compile_tests.diagnostic_locations)]
fn macro_generated_errors_match_the_invocation() {
    compile_test! {
        source: indoc! {r#"
            macro_rules! make {
                () => { fn broken() -> u16 { true } }
            }
            make!(); //# ERROR
        "#},
        diagnostics: expect![[r#"
            error: type mismatch: expected `u16`, found `bool`
             --> src/lib.rs:4:1
              |
            4 | make!(); //# ERROR
              | ^^^^^^^
              | |
              | found `bool`
              | expected `u16` because of return type"#]],
    }
}

#[test]
#[sage::test(crates.sage_test_harness.compile_tests.starting_expressions)]
fn implicit_dereferences_do_not_duplicate_source_observations() {
    compile_test! {
        source: indoc! {r#"
            struct Pair { value: u16 }
            fn read(pair: &Pair) -> u16 {
                pair.value //# ty
            }
        "#},
        types: expect![[r#"
            src/lib.rs: path `pair`: &Pair
            src/lib.rs: field `pair.value`: u16
        "#]],
    }
}

#[test]
#[sage::test(crates.sage_test_harness.compile_tests.project_isolation)]
fn configuration_selects_items_before_checking() {
    compile_test! {
        source: indoc! {r#"
            #[cfg(all(not(test), not(feature = "unused")))]
            fn included() -> u16 { 22 }
            #[cfg(any())]
            mod absent;
            struct Value;
            impl Value {
                #[cfg(test)]
                fn excluded() -> u16 { true }
                #[cfg(not(test))]
                fn included() -> u16 { included() }
            }
        "#},
    }
}

#[test]
#[sage::test(crates.sage_test_harness.compile_tests.diagnostic_counts)]
#[sage::test(crates.sage_test_harness.compile_tests.checking_scope)]
fn unused_import_is_checked() {
    compile_test! {
        source: "use missing::Item; //# ERROR",
        diagnostics: expect![[r#"
            error: unresolved import
             --> src/lib.rs:1:5
              |
            1 | use missing::Item; //# ERROR
              |     ^^^^^^^^^^^^^"#]],
    }
}
