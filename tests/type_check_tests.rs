use sage_test_harness::{TestCrate, expect};

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
         --> lib.rs:2:40
          |
        2 |          fn f() -> Wrapper { Wrapper { value: true } }
          |                                        ^^^^^^^^^^^
          |                                        |
          |                                        found `bool`
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
          |                                    ^ found `bool`"#]]);
}
