#![feature(rustc_private)]
#![feature(register_tool)]
#![register_tool(sage)]

use expect_test::expect;
use indoc::indoc;
use sage_test_harness::compile_test;

#[test]
#[sage::test(crates.sage_ir.inference.numeric_domains)]
fn return_context_constrains_numeric_literals_through_generic_calls() {
    compile_test! {
        source: indoc! {r#"
            fn id<T>(value: T) -> T { value }
            fn integer() -> u16 {
                id(22) //# ty
            }
            fn float() -> f32 {
                id(1.0) //# ty
            }
        "#},
        observations: expect![[r#"
            src/lib.rs: path `id`
              type: fn(u16) -> u16
            src/lib.rs: literal `22`
              type: u16
            src/lib.rs: call `id(22)`
              type: u16
            src/lib.rs: path `id`
              type: fn(f32) -> f32
            src/lib.rs: literal `1.0`
              type: f32
            src/lib.rs: call `id(1.0)`
              type: f32
        "#]],
    }
}

#[test]
#[sage::test(crates.sage_ir.inference.numeric_domains)]
fn distinct_numeric_classes_take_independent_contextual_types() {
    compile_test! {
        source: indoc! {r#"
            fn example() -> (u8, u64, f32, f64) {
                (1, 2, 3.0, 4.0) //# ty
            }
        "#},
        observations: expect![[r#"
            src/lib.rs: literal `1`
              type: u8
            src/lib.rs: literal `2`
              type: u64
            src/lib.rs: literal `3.0`
              type: f32
            src/lib.rs: literal `4.0`
              type: f64
            src/lib.rs: tuple `(1, 2, 3.0, 4.0)`
              type: (u8, u64, f32, f64)
        "#]],
    }
}

#[test]
#[sage::test(crates.sage_ir.inference.numeric_domains)]
fn numeric_domains_reject_nonmembers() {
    compile_test! {
        source: indoc! {r#"
            fn integer_as_bool() -> bool {
                22 //# ERROR
            }
            fn integer_as_float() -> f32 {
                22 //# ERROR
            }
            fn float_as_integer() -> u32 {
                1.0 //# ERROR
            }
        "#},
        diagnostics: expect![[r#"
            error: type mismatch: expected `bool`, found an integer
             --> src/lib.rs:1:30
              |
            1 |   fn integer_as_bool() -> bool {
              |  _________________________----_^
              | |                         |
              | |                         expected `bool` because of return type
            2 | |     22 //# ERROR
            3 | | }
              | | ^
              | |_|
              |   found an integer
            error: type mismatch: expected `f32`, found an integer
             --> src/lib.rs:4:30
              |
            4 |   fn integer_as_float() -> f32 {
              |  __________________________---_^
              | |                          |
              | |                          expected `f32` because of return type
            5 | |     22 //# ERROR
            6 | | }
              | | ^
              | |_|
              |   found an integer
            error: type mismatch: expected `u32`, found a floating-point type
             --> src/lib.rs:7:30
              |
            7 |   fn float_as_integer() -> u32 {
              |  __________________________---_^
              | |                          |
              | |                          expected `u32` because of return type
            8 | |     1.0 //# ERROR
            9 | | }
              | | ^
              | |_|
              |   found a floating-point type"#]],
    }
}

#[test]
#[sage::test(crates.sage_ir.inference.numeric_domains)]
fn unconstrained_numeric_literals_report_errors_instead_of_defaulting() {
    compile_test! {
        source: indoc! {r#"
            fn integer() { let value = 22; } //# ERROR
            //# ty
            fn float() { let value = 1.0; } //# ERROR
            //# ty
        "#},
        observations: expect![[r#"
            src/lib.rs: literal `22`
              type: <error>
            src/lib.rs: block `{ let value = 22; }`
              type: ()
            src/lib.rs: literal `1.0`
              type: <error>
            src/lib.rs: block `{ let value = 1.0; }`
              type: ()
        "#]],
        diagnostics: expect![[r#"
            error: could not infer an integer type
             --> src/lib.rs:1:1
              |
            1 | fn integer() { let value = 22; } //# ERROR
              | ^
            error: could not infer a floating-point type
             --> src/lib.rs:3:1
              |
            3 | fn float() { let value = 1.0; } //# ERROR
              | ^"#]],
    }
}

#[test]
#[sage::test(crates.sage_ir.inference.subtyping_and_coercion)]
fn top_level_never_coercion_preserves_source_and_result_types() {
    compile_test! {
        source: indoc! {r#"
            fn diverge() -> u32 {
                loop {} //# ty
            }
            fn still_never() -> ! {
                loop {} //# ty
            }
        "#},
        observations: expect![[r#"
            src/lib.rs: block `{}`
              type: ()
            src/lib.rs: loop `loop {}`
              type: !
            src/lib.rs: coercion `loop {}`
              type: u32
            src/lib.rs: block `{}`
              type: ()
            src/lib.rs: loop `loop {}`
              type: !
        "#]],
    }
}

#[test]
#[sage::test(crates.sage_ir.inference.subtyping_and_coercion)]
fn never_to_any_is_not_subtyping_beneath_mutable_references() {
    compile_test! {
        source: indoc! {r#"
            fn invalid(value: &mut !) -> &mut u32 {
                value //# ERROR
            }
        "#},
        diagnostics: expect![[r#"
            error: type mismatch: expected `u32`, found `!`
             --> src/lib.rs:1:39
              |
            1 |   fn invalid(value: &mut !) -> &mut u32 {
              |  ______________________________--------_^
              | |                              |
              | |                              expected `u32` because of return type
            2 | |     value //# ERROR
            3 | | }
              | | ^
              | |_|
              |   found `!`"#]],
    }
}

#[test]
#[sage::test(crates.sage_ir.inference.subtyping_and_coercion)]
fn generic_never_coercions_follow_other_arguments_in_either_order() {
    compile_test! {
        source: indoc! {r#"
            fn pair<T>(first: T, second: T) {}
            fn single<T>(value: T) {}
            fn example() {
                pair(loop {}, true); //# ty
                pair(true, loop {}); //# ty
                single(loop {}); //# ty
            }
        "#},
        observations: expect![[r#"
            src/lib.rs: path `pair`
              type: fn(bool, bool) -> ()
            src/lib.rs: block `{}`
              type: ()
            src/lib.rs: loop `loop {}`
              type: !
            src/lib.rs: coercion `loop {}`
              type: bool
            src/lib.rs: literal `true`
              type: bool
            src/lib.rs: call `pair(loop {}, true)`
              type: ()
            src/lib.rs: path `pair`
              type: fn(bool, bool) -> ()
            src/lib.rs: literal `true`
              type: bool
            src/lib.rs: block `{}`
              type: ()
            src/lib.rs: loop `loop {}`
              type: !
            src/lib.rs: coercion `loop {}`
              type: bool
            src/lib.rs: call `pair(true, loop {})`
              type: ()
            src/lib.rs: path `single`
              type: fn(!) -> ()
            src/lib.rs: block `{}`
              type: ()
            src/lib.rs: loop `loop {}`
              type: !
            src/lib.rs: call `single(loop {})`
              type: ()
        "#]],
    }
}

#[test]
#[sage::test(crates.sage_ir.inference.numeric_domains)]
fn later_numeric_constraint_selects_an_impl_through_a_where_clause() {
    compile_test! {
        source: indoc! {r#"
            enum Maybe<T> { None, Some(T) }
            trait Marker {}
            impl Marker for Maybe<u8> {}
            impl Marker for Maybe<bool> {}
            fn require_marker<T>(value: &Maybe<T>) where Maybe<T>: Marker {}
            fn example() {
                let mut value = Maybe::None;
                require_marker(&value);
                value = Maybe::Some(
                    22, //# ty
                );
                return;
            }
        "#},
        observations: expect![[r#"
            src/lib.rs: literal `22`
              type: u8
        "#]],
    }
}
