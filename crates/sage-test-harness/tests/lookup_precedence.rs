#![feature(rustc_private)]
#![feature(register_tool)]
#![register_tool(sage)]
use expect_test::expect;
use indoc::indoc;
use sage_test_harness::compile_test;

#[test]
#[sage::test(crates.sage_ir.resolution.lookup.precedence)]
fn local_shadowing_does_not_change_qualified_lookup() {
    compile_test! {
        source: indoc! {r#"
            fn value() -> u16 { 1 }
            fn example(value: u16) -> u16 {
                let before = value; //# sym
                let value = crate::value(); //# sym
                value //# sym
            }
        "#},
        observations: expect![[r#"
            src/lib.rs: path `value`
              symbol: local value#1
            src/lib.rs: path `crate::value`
              symbol: fn crate::value
            src/lib.rs: path `value`
              symbol: local value#2
        "#]],
    }
}

#[test]
#[sage::test(crates.sage_ir.resolution.lookup.precedence)]
fn explicit_imports_take_precedence_over_globs() {
    compile_test! {
        source: indoc! {r#"
            mod first { pub fn value() -> u16 { 1 } }
            mod second { pub fn value() -> u16 { 2 } }
            use first::*;
            use second::value as selected;
            use second::value;
            fn example() -> u16 {
                let first = selected(); //# sym
                value() //# sym
            }
        "#},
        observations: expect![[r#"
            src/lib.rs: path `selected`
              symbol: fn crate::second::value
            src/lib.rs: path `value`
              symbol: fn crate::second::value
        "#]],
    }
}

#[test]
#[sage::test(crates.sage_ir.resolution.lookup.precedence)]
fn direct_items_take_precedence_over_globs() {
    compile_test! {
        source: indoc! {r#"
            mod imported { pub fn value() -> u16 { 1 } }
            use imported::*;
            fn value() -> u16 { 2 }
            fn example() -> u16 {
                value() //# sym
            }
        "#},
        observations: expect![[r#"
            src/lib.rs: path `value`
              symbol: fn crate::value
        "#]],
    }
}

#[test]
#[sage::test(crates.sage_ir.resolution.lookup.precedence)]
fn type_value_and_macro_namespaces_are_independent() {
    compile_test! {
        source: indoc! {r#"
            struct Same { field: u16 }
            fn Same() -> u16 { 1 }
            macro_rules! Same { () => { fn generated() -> u16 { Same() } } }
            Same!();
            fn example(value: Same) -> u16 {
                let result = Same(); //# sym
                let expanded = generated(); //# sym
                value.field //# ty
            }
        "#},
        observations: expect![[r#"
            src/lib.rs: path `Same`
              symbol: fn crate::Same
            src/lib.rs: path `generated`
              symbol: fn crate::generated
            src/lib.rs: path `value`
              type: Same
            src/lib.rs: field `value.field`
              type: u16
        "#]],
    }
}

#[test]
#[sage::test(crates.sage_ir.resolution.lookup.precedence)]
fn qualified_paths_select_the_named_module_scope() {
    compile_test! {
        source: indoc! {r#"
            fn value() -> u16 { 1 }
            mod outer {
                fn value() -> u16 { 2 }
                mod inner {
                    fn value() -> u16 { 3 }
                    fn example(outer: u16) -> u16 {
                        let here = self::value(); //# sym
                        let parent = super::value(); //# sym
                        let root = crate::value(); //# sym
                        crate::outer::value() //# sym
                    }
                }
            }
        "#},
        observations: expect![[r#"
            src/lib.rs: path `self::value`
              symbol: fn crate::outer::inner::value
            src/lib.rs: path `super::value`
              symbol: fn crate::outer::value
            src/lib.rs: path `crate::value`
              symbol: fn crate::value
            src/lib.rs: path `crate::outer::value`
              symbol: fn crate::outer::value
        "#]],
    }
}

#[test]
#[sage::test(crates.sage_ir.resolution.lookup.precedence)]
fn transitive_cyclic_globs_resolve_a_value_definition() {
    compile_test! {
        source: indoc! {r#"
            mod first { pub use crate::second::*; }
            mod second { pub use crate::third::*; }
            mod third { pub use crate::first::*; pub fn value() -> u16 { 3 } }
            use first::*;
            fn example() -> u16 {
                value() //# sym
            }
        "#},
        observations: expect![[r#"
            src/lib.rs: path `value`
              symbol: fn crate::third::value
        "#]],
    }
}

#[test]
#[sage::test(crates.sage_ir.resolution.lookup.precedence)]
fn rust_2021_prelude_supplies_try_into() {
    compile_test! {
        edition: "2021",
        source: indoc! {r#"
            fn example<T: TryInto<u16>>(value: T) -> T {
                value //# ty
            }
        "#},
        observations: expect![[r#"
            src/lib.rs: path `value`
              type: T
        "#]],
    }
}

#[test]
#[sage::test(crates.sage_ir.resolution.lookup.precedence)]
fn rust_2018_prelude_does_not_supply_try_into() {
    compile_test! {
        edition: "2018",
        source: "fn example<T: TryInto<u16>>(value: T) -> T { value } //# ERROR",
        diagnostics: expect![[r#"
            error: unresolved trait in type predicate
             --> src/lib.rs:1:15
              |
            1 | fn example<T: TryInto<u16>>(value: T) -> T { value } //# ERROR
              |               ^^^^^^^^^^^^"#]],
    }
}

#[test]
#[sage::test(crates.sage_ir.resolution.lookup.precedence)]
fn nested_scope_shadowing_restores_the_outer_binding() {
    compile_test! {
        source: indoc! {r#"
            fn example(value: u16) -> u16 {
                let nested = {
                    let value = true;
                    value //# sym
                    //# ty
                };
                let after = value; //# sym
                //# ty
                after //# sym
                //# ty
            }
        "#},
        observations: expect![[r#"
            src/lib.rs: path `value`
              symbol: local value#2
              type: bool
            src/lib.rs: path `value`
              symbol: local value#1
              type: u16
            src/lib.rs: path `after`
              symbol: local after#1
              type: u16
        "#]],
    }
}

#[test]
#[sage::test(crates.sage_ir.resolution.lookup.precedence)]
fn absolute_external_paths_bypass_a_shadowing_local_module() {
    compile_test! {
        edition: "2021",
        source: indoc! {r#"
            mod std { pub mod cmp { pub enum Ordering { Less } } }
            fn local() -> std::cmp::Ordering {
                std::cmp::Ordering::Less //# sym
            }
            fn external() -> ::std::cmp::Ordering {
                ::std::cmp::Ordering::Less //# sym
            }
        "#},
        observations: expect![[r#"
            src/lib.rs: path `std::cmp::Ordering::Less`
              symbol: variant crate::std::cmp::Ordering::Less
            src/lib.rs: path `::std::cmp::Ordering::Less`
              symbol: variant std::cmp::Ordering::Less
        "#]],
    }
}

#[test]
#[sage::test(crates.sage_ir.resolution.lookup.precedence)]
fn sibling_scopes_do_not_share_bindings_or_local_type_slots() {
    compile_test! {
        source: indoc! {r#"
            fn example(value: u16) -> u16 {
                let first = {
                    let value = true;
                    value //# sym
                    //# ty
                };
                let second = {
                    let value = 33u8;
                    value //# sym
                    //# ty
                };
                value //# sym
                //# ty
            }
        "#},
        observations: expect![[r#"
            src/lib.rs: path `value`
              symbol: local value#2
              type: bool
            src/lib.rs: path `value`
              symbol: local value#3
              type: u8
            src/lib.rs: path `value`
              symbol: local value#1
              type: u16
        "#]],
    }
}

#[test]
#[sage::test(crates.sage_ir.resolution.lookup.precedence)]
fn named_type_import_takes_precedence_over_the_standard_prelude() {
    compile_test! {
        source: indoc! {r#"
            mod custom { pub struct Option { field: u16 } }
            use custom::Option;
            fn example(value: Option) -> Option {
                value //# ty
            }
        "#},
        observations: expect![[r#"
            src/lib.rs: path `value`
              type: custom::Option
        "#]],
    }
}

#[test]
#[sage::test(crates.sage_ir.resolution.lookup.precedence)]
fn rust_2024_prelude_supplies_into_future() {
    compile_test! {
        edition: "2024",
        source: indoc! {r#"
            fn example<T: IntoFuture>(value: T) -> T {
                value //# ty
            }
        "#},
        observations: expect![[r#"
            src/lib.rs: path `value`
              type: T
        "#]],
    }
}

#[test]
#[sage::test(crates.sage_ir.resolution.lookup.precedence)]
fn rust_2021_prelude_does_not_supply_into_future() {
    compile_test! {
        edition: "2021",
        source: "fn example<T: IntoFuture>(value: T) -> T { value } //# ERROR",
        diagnostics: expect![[r#"
            error: unresolved trait in type predicate
             --> src/lib.rs:1:15
              |
            1 | fn example<T: IntoFuture>(value: T) -> T { value } //# ERROR
              |               ^^^^^^^^^^"#]],
    }
}

#[test]
#[sage::test(crates.sage_ir.resolution.lookup.precedence)]
fn competing_glob_definitions_remain_ambiguous_despite_equal_types() {
    compile_test! {
        source: indoc! {r#"
            mod first { pub fn value() -> u16 { 1 } }
            mod second { pub fn value() -> u16 { 2 } }
            use first::*;
            use second::*;
            fn example() -> u16 {
                value() //# ERROR
                //# sym
            }
        "#},
        observations: expect![[r#"
            src/lib.rs: path `value`
              symbol: <unresolved>
        "#]],
        diagnostics: expect![[r#"
            error: ambiguous name: 2 candidates
             --> src/lib.rs:6:5
              |
            6 |     value() //# ERROR
              |     ^^^^^"#]],
    }
}
