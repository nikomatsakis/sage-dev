use sage_ir::ty::{Ty, UintTy};
use sage_ir::tytree::TyExprData;
use sage_test_harness::{TestCrate, expect, local_function_named, with_test_crate};

#[test]
fn identity_no_errors() {
    TestCrate::in_memory("fn identity(x: u32) -> u32 { x }").check_ok();
}

#[test]
fn return_type_mismatch() {
    TestCrate::in_memory("fn bad(x: u32) -> bool { x }").check_errors(expect![[r#"
        error: type mismatch: expected `bool`, found `u32`
         --> lib.rs:1:24
          |
        1 | fn bad(x: u32) -> bool { x }
          |                   ---- ^^^^^ found `u32`
          |                   |
          |                   expected `bool` because of return type"#]]);
}

#[test]
fn binary_add_same_type() {
    TestCrate::in_memory("fn add(x: u32, y: u32) -> u32 { x + y }").check_ok();
}

#[test]
fn binary_add_type_mismatch() {
    TestCrate::in_memory("fn bad(x: u32, y: bool) -> u32 { x + y }").check_errors(expect![[r#"
        error: type mismatch: expected `u32`, found `bool`
         --> lib.rs:1:34
          |
        1 | fn bad(x: u32, y: bool) -> u32 { x + y }
          |                                  ^^^^^ found `bool`"#]]);
}

#[test]
fn if_else_same_type() {
    TestCrate::in_memory("fn pick(b: bool) -> u32 { if b { 1 } else { 2 } }").check_ok();
}

#[test]
fn if_else_branch_mismatch() {
    TestCrate::in_memory("fn bad(b: bool) -> u32 { if b { 1 } else { true } }").check_errors(
        expect![[r#"
            error: type mismatch: expected `u32`, found `bool`
             --> lib.rs:1:24
              |
            1 | fn bad(b: bool) -> u32 { if b { 1 } else { true } }
              |                    --- ^^^^^^^^^^^^^^^^^^^^^^^^^^^^ found `bool`
              |                    |
              |                    expected `u32` because of return type"#]],
    );
}

#[test]
fn never_to_any_does_not_apply_beneath_mutable_reference() {
    TestCrate::in_memory("fn invalid(value: &mut !) -> &mut u32 { value }").check_errors(expect![
        [r#"
            error: type mismatch: expected `u32`, found `!`
             --> lib.rs:1:39
              |
            1 | fn invalid(value: &mut !) -> &mut u32 { value }
              |                              -------- ^^^^^^^^^ found `!`
              |                              |
              |                              expected `u32` because of return type"#]
    ]);
}

#[test]
fn top_level_never_to_any_is_explicit_in_typed_ir() {
    with_test_crate("fn diverge() -> u32 { loop {} }", |db, root| {
        let function = local_function_named(db, root, "diverge");

        let checked = function.body(db);
        assert!(checked.diagnostics.is_empty(), "{:?}", checked.diagnostics);
        let (stash, body) = checked.body.open_deref();
        let root = stash[body.root];
        let TyExprData::Block(_, Some(tail)) = root.data else {
            panic!("expected function body block, found {:?}", root.data);
        };
        let TyExprData::NeverToAny(source) = stash[tail].data else {
            panic!(
                "expected explicit never-to-any coercion, found {:?}",
                stash[tail].data
            );
        };
        assert!(matches!(stash[root.ty], Ty::Uint(UintTy::U32)));
        assert!(matches!(stash[stash[tail].ty], Ty::Uint(UintTy::U32)));
        assert!(matches!(stash[stash[source].ty], Ty::Never));
    });
}

#[test]
fn never_result_does_not_create_a_never_to_never_coercion() {
    with_test_crate("fn diverge() -> ! { loop {} }", |db, root| {
        let function = local_function_named(db, root, "diverge");

        let checked = function.body(db);
        assert!(checked.diagnostics.is_empty(), "{:?}", checked.diagnostics);
        let (stash, body) = checked.body.open_deref();
        let root = stash[body.root];
        assert!(!matches!(root.data, TyExprData::NeverToAny(_)));
        assert!(matches!(stash[root.ty], Ty::Never));
    });
}

#[test]
fn inferred_never_join_does_not_retain_never_to_never_coercions() {
    with_test_crate(
        "fn diverge(condition: bool) -> ! { if condition { loop {} } else { loop {} } }",
        |db, root| {
            let function = local_function_named(db, root, "diverge");

            let checked = function.body(db);
            assert!(checked.diagnostics.is_empty(), "{:?}", checked.diagnostics);
            let (stash, body) = checked.body.open_deref();
            let TyExprData::Block(_, Some(tail)) = stash[body.root].data else {
                panic!("expected function body block");
            };
            let TyExprData::If(_, then_branch, Some(else_branch)) = stash[tail].data else {
                panic!("expected if expression, found {:?}", stash[tail].data);
            };
            assert!(matches!(stash[stash[then_branch].ty], Ty::Never));
            assert!(matches!(stash[stash[else_branch].ty], Ty::Never));
            let TyExprData::Block(_, Some(then_tail)) = stash[then_branch].data else {
                panic!("expected then-branch block");
            };
            let TyExprData::Block(_, Some(else_tail)) = stash[else_branch].data else {
                panic!("expected else-branch block");
            };
            assert!(!matches!(stash[then_tail].data, TyExprData::NeverToAny(_)));
            assert!(!matches!(stash[else_tail].data, TyExprData::NeverToAny(_)));
        },
    );
}

#[test]
fn call_argument_never_to_any_is_explicit_in_typed_ir() {
    with_test_crate(
        "fn takes(_: u32) -> u32 { 0 } fn diverge() -> u32 { takes(loop {}) }",
        |db, root| {
            let function = local_function_named(db, root, "diverge");

            let checked = function.body(db);
            assert!(checked.diagnostics.is_empty(), "{:?}", checked.diagnostics);
            let (stash, body) = checked.body.open_deref();
            let root = stash[body.root];
            let TyExprData::Block(_, Some(tail)) = root.data else {
                panic!("expected function body block, found {:?}", root.data);
            };
            let TyExprData::Call(_, arguments) = stash[tail].data else {
                panic!("expected call expression, found {:?}", stash[tail].data);
            };
            let [argument] = stash[arguments] else {
                panic!("expected one call argument");
            };
            let TyExprData::NeverToAny(source) = stash[argument].data else {
                panic!(
                    "expected explicit never-to-any argument coercion, found {:?}",
                    stash[argument].data
                );
            };
            assert!(matches!(stash[stash[argument].ty], Ty::Uint(UintTy::U32)));
            assert!(matches!(stash[stash[source].ty], Ty::Never));
        },
    );
}

#[test]
fn direct_call_rejects_too_few_arguments() {
    TestCrate::in_memory("fn takes(_: u32, _: bool) {} fn caller() { takes(22); }").check_errors(
        expect![[r#"
            error: function takes 2 arguments but 1 argument was supplied
             --> lib.rs
              |"#]],
    );
}

#[test]
fn direct_call_rejects_too_many_arguments() {
    TestCrate::in_memory("fn takes(_: u32) {} fn caller() { takes(22, true); }").check_errors(
        expect![[r#"
            error: function takes 1 argument but 2 arguments were supplied
             --> lib.rs
              |"#]],
    );
}

#[test]
fn direct_call_checks_an_extra_argument_after_reporting_arity() {
    TestCrate::in_memory(
        "fn needs_bool(_: bool) {} fn takes(_: u32) {} \
         fn caller() { takes(22, needs_bool(\"not bool\")); }",
    )
    .check_errors(expect![[r#"
        error: function takes 1 argument but 2 arguments were supplied
         --> lib.rs
          |
        error: type mismatch: expected `bool`, found `&str`
         --> lib.rs:1:82
          |
        1 | fn needs_bool(_: bool) {} fn takes(_: u32) {} fn caller() { takes(22, needs_bool("not bool")); }
          |                                                                       -----------^^^^^^^^^^-
          |                                                                       |          |
          |                                                                       |          found `&str`
          |                                                                       expected `bool` for argument 1"#]]);
}

#[test]
fn explicit_return_never_to_any_uses_the_value_span() {
    with_test_crate("fn diverge() -> u32 { return loop {} }", |db, root| {
        let function = local_function_named(db, root, "diverge");

        let checked = function.body(db);
        assert!(checked.diagnostics.is_empty(), "{:?}", checked.diagnostics);
        let (stash, body) = checked.body.open_deref();
        let TyExprData::Block(_, Some(body_tail)) = stash[body.root].data else {
            panic!("expected function body block");
        };
        let TyExprData::NeverToAny(return_expr) = stash[body_tail].data else {
            panic!("expected diverging return to be coerced at the body boundary");
        };
        let TyExprData::Return(Some(value)) = stash[return_expr].data else {
            panic!("expected return expression");
        };
        let TyExprData::NeverToAny(source) = stash[value].data else {
            panic!("expected explicit never-to-any return-value coercion");
        };
        assert_eq!(stash[value].span, stash[source].span);
        assert_ne!(stash[value].span, stash[return_expr].span);
    });
}

#[test]
fn struct_field_never_to_any_uses_the_value_span() {
    with_test_crate(
        "struct Holder { value: u32 } fn make() -> Holder { Holder { value: loop {} } }",
        |db, root| {
            let function = local_function_named(db, root, "make");

            let checked = function.body(db);
            assert!(checked.diagnostics.is_empty(), "{:?}", checked.diagnostics);
            let (stash, body) = checked.body.open_deref();
            let TyExprData::Block(_, Some(struct_lit)) = stash[body.root].data else {
                panic!("expected function body block");
            };
            let TyExprData::StructLit(_, fields) = stash[struct_lit].data else {
                panic!("expected struct literal");
            };
            let [field] = stash[fields] else {
                panic!("expected one field initializer");
            };
            let TyExprData::NeverToAny(source) = stash[field.value].data else {
                panic!("expected explicit never-to-any field coercion");
            };
            assert_eq!(stash[field.value].span, stash[source].span);
            assert_ne!(stash[field.value].span, field.span);
        },
    );
}

#[test]
fn generic_call_arguments_are_order_independent_and_fall_back_to_never() {
    TestCrate::in_memory(
        "fn pair<T>(_: T, _: T) {} fn single<T>(_: T) {} \
         fn diverge() { pair(loop {}, true); pair(true, loop {}); single(loop {}); }",
    )
    .check_ok();
}

#[test]
fn never_fallback_wakes_suspended_method_lookup() {
    TestCrate::in_memory("fn diverge() { let x = loop {}; x.clone(); }").check_errors(expect![[
        r#"
            error: method lookup requires unsupported or incomplete candidate information
             --> lib.rs
              |"#
    ]]);
}

#[test]
fn quiescent_never_fallback_does_not_settle_unrelated_targets() {
    TestCrate::in_memory(
        "fn takes(_: &mut bool) {} fn diverge() { \
         let mut first = loop {}; let second = loop {}; \
         second.clone(); takes(&mut first); }",
    )
    .check_errors(expect![[r#"
        error: method lookup requires unsupported or incomplete candidate information
         --> lib.rs
          |"#]]);
}

#[test]
fn let_binding_inferred() {
    TestCrate::in_memory("fn f(x: u32) -> u32 { let y = x; y }").check_ok();
}

#[test]
fn let_binding_mismatch_return() {
    TestCrate::in_memory("fn f(x: u32) -> bool { let y = x; y }").check_errors(expect![[r#"
        error: type mismatch: expected `bool`, found `u32`
         --> lib.rs:1:22
          |
        1 | fn f(x: u32) -> bool { let y = x; y }
          |                 ---- ^^^^^^^^^^^^^^^^ found `u32`
          |                 |
          |                 expected `bool` because of return type"#]]);
}

#[test]
fn multiple_params() {
    TestCrate::in_memory("fn f(a: u32, b: u32, c: u32) -> u32 { a + b + c }").check_ok();
}

#[test]
fn unit_return() {
    TestCrate::in_memory("fn f() { }").check_ok();
}

#[test]
fn bool_literal() {
    TestCrate::in_memory("fn f() -> bool { true }").check_ok();
}

// ---------------------------------------------------------------------------
// Compound types: struct construction and field access
// ---------------------------------------------------------------------------

#[test]
fn struct_lit_basic() {
    TestCrate::in_memory(
        "struct Wrapper { value: u32 }
         fn f() -> Wrapper { Wrapper { value: 42 } }",
    )
    .check_ok();
}

#[test]
fn struct_field_access() {
    TestCrate::in_memory(
        "struct Wrapper { value: u32 }
         fn f(w: Wrapper) -> u32 { w.value }",
    )
    .check_ok();
}

#[test]
fn struct_field_type_mismatch() {
    TestCrate::in_memory(
        "struct Wrapper { value: u32 }
         fn f(w: Wrapper) -> bool { w.value }",
    )
    .check_errors(expect![[r#"
        error: type mismatch: expected `bool`, found `u32`
         --> lib.rs:2:35
          |
        2 |          fn f(w: Wrapper) -> bool { w.value }
          |                              ---- ^^^^^^^^^^^ found `u32`
          |                              |
          |                              expected `bool` because of return type"#]]);
}

#[test]
fn struct_lit_field_mismatch() {
    TestCrate::in_memory(
        "struct Wrapper { value: u32 }
         fn f() -> Wrapper { Wrapper { value: true } }",
    )
    .check_errors(expect![[r#"
        error: type mismatch: expected `u32`, found `bool`
         --> lib.rs:2:47
          |
        2 |          fn f() -> Wrapper { Wrapper { value: true } }
          |                                        -------^^^^
          |                                        |      |
          |                                        |      found `bool`
          |                                        expected `u32` for this field"#]]);
}

// ---------------------------------------------------------------------------
// Generic structs: type parameter propagation
// ---------------------------------------------------------------------------

#[test]
fn generic_struct_lit() {
    TestCrate::in_memory(
        "struct Pair<A, B> { first: A, second: B }
         fn f() -> Pair<u32, bool> { Pair { first: 1, second: true } }",
    )
    .check_ok();
}

#[test]
fn generic_struct_field_access() {
    TestCrate::in_memory(
        "struct Wrapper<T> { value: T }
         fn f(w: Wrapper<u32>) -> u32 { w.value }",
    )
    .check_ok();
}

#[test]
fn generic_struct_field_mismatch() {
    TestCrate::in_memory(
        "struct Wrapper<T> { value: T }
         fn f(w: Wrapper<u32>) -> bool { w.value }",
    )
    .check_errors(expect![[r#"
        error: type mismatch: expected `bool`, found `u32`
         --> lib.rs:2:40
          |
        2 |          fn f(w: Wrapper<u32>) -> bool { w.value }
          |                                   ---- ^^^^^^^^^^^ found `u32`
          |                                   |
          |                                   expected `bool` because of return type"#]]);
}

#[test]
fn generic_struct_infer_from_field() {
    // The type arg of Wrapper is inferred from the field value
    TestCrate::in_memory(
        "struct Wrapper<T> { value: T }
         fn f(x: u32) -> Wrapper<u32> { Wrapper { value: x } }",
    )
    .check_ok();
}

#[test]
fn generic_struct_infer_mismatch() {
    // T inferred as u32 from field, but return expects Wrapper<bool>
    TestCrate::in_memory(
        "struct Wrapper<T> { value: T }
         fn f(x: u32) -> Wrapper<bool> { Wrapper { value: x } }",
    )
    .check_errors(expect![[r#"
        error: type mismatch: expected `bool`, found `u32`
         --> lib.rs:2:40
          |
        2 |          fn f(x: u32) -> Wrapper<bool> { Wrapper { value: x } }
          |                          ------------- ^^^^^^^^^^^^^^^^^^^^^^^^ found `u32`
          |                          |
          |                          expected `bool` because of return type"#]]);
}

#[test]
fn generic_pair_field_propagation() {
    // Accessing .first on Pair<u32, bool> should yield u32
    TestCrate::in_memory(
        "struct Pair<A, B> { first: A, second: B }
         fn f(p: Pair<u32, bool>) -> u32 { p.first }",
    )
    .check_ok();
}

#[test]
fn generic_pair_wrong_field() {
    // Accessing .second on Pair<u32, bool> yields bool, not u32
    TestCrate::in_memory(
        "struct Pair<A, B> { first: A, second: B }
         fn f(p: Pair<u32, bool>) -> u32 { p.second }",
    )
    .check_errors(expect![[r#"
        error: type mismatch: expected `u32`, found `bool`
         --> lib.rs:2:42
          |
        2 |          fn f(p: Pair<u32, bool>) -> u32 { p.second }
          |                                      --- ^^^^^^^^^^^^ found `bool`
          |                                      |
          |                                      expected `u32` because of return type"#]]);
}

#[test]
fn nested_generic_struct() {
    // Wrapper<Wrapper<u32>> — field access should propagate through
    TestCrate::in_memory(
        "struct Wrapper<T> { value: T }
         fn f(w: Wrapper<Wrapper<u32>>) -> Wrapper<u32> { w.value }",
    )
    .check_ok();
}

#[test]
fn nested_generic_mismatch() {
    TestCrate::in_memory(
        "struct Wrapper<T> { value: T }
         fn f(w: Wrapper<Wrapper<u32>>) -> u32 { w.value }",
    )
    .check_errors(expect![[r#"
        error: type mismatch: expected `u32`, found `Wrapper<u32>`
         --> lib.rs:2:48
          |
        2 |          fn f(w: Wrapper<Wrapper<u32>>) -> u32 { w.value }
          |                                            --- ^^^^^^^^^^^ found `Wrapper<u32>`
          |                                            |
          |                                            expected `u32` because of return type"#]]);
}

#[test]
fn struct_construct_then_access() {
    // Build a struct, bind it, access a field
    TestCrate::in_memory(
        "struct Point { x: u32, y: u32 }
         fn f() -> u32 { let p = Point { x: 1, y: 2 }; p.x }",
    )
    .check_ok();
}

#[test]
fn generic_construct_then_access() {
    TestCrate::in_memory(
        "struct Wrapper<T> { value: T }
         fn f() -> u32 { let w = Wrapper { value: 42 }; w.value }",
    )
    .check_ok();
}

// ---------------------------------------------------------------------------
// Cross-module: struct in another module, accessed from root
// ---------------------------------------------------------------------------

#[test]
fn cross_module_struct_field_access() {
    TestCrate::in_memory("mod other; fn f(w: other::Wrapper) -> u32 { w.value }")
        .file("other.rs", "pub struct Wrapper { pub value: u32 }")
        .check_ok();
}

#[test]
fn cross_module_struct_field_non_intrinsic() {
    // The struct's field type (Inner) must be resolved from the *defining*
    // module's scope, not the caller's. This test would fail if the type
    // checker passed its own module for signature resolution.
    TestCrate::in_memory("mod other; fn f(w: other::Wrapper) -> other::Inner { w.value }")
        .file(
            "other.rs",
            "pub struct Inner { pub x: u32 } pub struct Wrapper { pub value: Inner }",
        )
        .check_ok();
}

// ---------------------------------------------------------------------------
// TyDisplay: exercises non-trivial type formatting
// ---------------------------------------------------------------------------

#[test]
fn ty_display_unit_return() {
    // Empty block body has type `()`, return type is `u32`
    TestCrate::in_memory("fn f() -> u32 { }").check_errors(expect![[r#"
        error: type mismatch: expected `u32`, found `()`
         --> lib.rs:1:15
          |
        1 | fn f() -> u32 { }
          |           --- ^^^ found `()`
          |           |
          |           expected `u32` because of return type"#]]);
}

#[test]
fn ty_display_fn_pointer() {
    // g has type `fn(u32) -> bool`, return type is `u32`
    TestCrate::in_memory("fn f(g: fn(u32) -> bool) -> u32 { g }").check_errors(expect![[r#"
        error: type mismatch: expected `u32`, found `fn(u32) -> bool`
         --> lib.rs:1:33
          |
        1 | fn f(g: fn(u32) -> bool) -> u32 { g }
          |                             --- ^^^^^ found `fn(u32) -> bool`
          |                             |
          |                             expected `u32` because of return type"#]]);
}

// ---------------------------------------------------------------------------
// Async infrastructure: block_on + await_concrete
// ---------------------------------------------------------------------------

#[test]
fn await_concrete_immediate() {
    // await_concrete on an already-concrete type returns immediately
    TestCrate::in_memory("fn f(x: u32) -> u32 { x }").check_ok();
}

#[test]
fn await_concrete_via_unification() {
    // The call `id(x)` exercises: callee type is infer var → unified to fn(u32)->u32
    // → check_call_ty resolves it → return type checks
    TestCrate::in_memory(
        "fn id(x: u32) -> u32 { x }
         fn f(x: u32) -> u32 { id(x) }",
    )
    .check_ok();
}

#[test]
fn fn_call_arg_mismatch() {
    TestCrate::in_memory(
        "fn id(x: u32) -> u32 { x }
         fn f(x: bool) -> u32 { id(x) }",
    )
    .check_errors(expect![[r#"
        error: type mismatch: expected `u32`, found `bool`
         --> lib.rs:2:36
          |
        2 |          fn f(x: bool) -> u32 { id(x) }
          |                                 ---^-
          |                                 |  |
          |                                 |  found `bool`
          |                                 expected `u32` for argument 1"#]]);
}

// ---------------------------------------------------------------------------
// Return expression: constrain against declared return type
// ---------------------------------------------------------------------------

#[test]
fn return_expr_ok() {
    TestCrate::in_memory("fn f(x: u32) -> u32 { return x; }").check_ok();
}

#[test]
fn return_expr_mismatch() {
    TestCrate::in_memory("fn f(x: u32) -> bool { return x; }").check_errors(expect![[r#"
        error: type mismatch: expected `bool`, found `u32`
         --> lib.rs:1:31
          |
        1 | fn f(x: u32) -> bool { return x; }
          |                 ----          ^ found `u32`
          |                 |
          |                 expected `bool` because of return type"#]]);
}

#[test]
fn return_unit_mismatch() {
    TestCrate::in_memory("fn f() -> u32 { return; }").check_errors(expect![[r#"
        error: type mismatch: expected `u32`, found `()`
         --> lib.rs:1:17
          |
        1 | fn f() -> u32 { return; }
          |           ---   ^^^^^^ found `()`
          |           |
          |           expected `u32` because of return type"#]]);
}

#[test]
fn early_return_with_later_code() {
    TestCrate::in_memory(
        "fn f(x: u32) -> u32 {
             if x > 0 { return x; }
             x + 1
         }",
    )
    .check_ok();
}
