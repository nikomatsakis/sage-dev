#![feature(rustc_private)]
#![feature(register_tool)]
#![register_tool(sage)]

use sage_test_harness::compile_test;

#[test]
#[sage::test(crates.sage_ir.types.lifetimes.unchecked_borrows)]
fn unsuffixed_numeric_dereference_reports_a_diagnostic_without_panicking() {
    sage_test_harness::with_test_crate("fn read() { *1; }", |db, root| {
        let checked = sage_test_harness::local_function_named(db, root, "read").body(db);
        assert!(
            checked
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.message
                    == "dereference of an unresolved operand type is not supported yet"),
            "numeric dereference must report a source error: {:?}",
            checked.diagnostics
        );
    });
}

#[test]
#[sage::test(crates.sage_ir.types.lifetimes.unchecked_borrows)]
fn unconstrained_operand_dereference_reports_a_diagnostic_without_panicking() {
    sage_test_harness::with_test_crate(
        "fn make<T>() -> T { loop {} } fn read() { *make(); }",
        |db, root| {
            let checked = sage_test_harness::local_function_named(db, root, "read").body(db);
            assert!(
                checked
                    .diagnostics
                    .iter()
                    .any(|diagnostic| diagnostic.message
                        == "dereference of an unresolved operand type is not supported yet"),
                "unresolved dereference must report its limitation: {:?}",
                checked.diagnostics
            );
        },
    );
}

mod todo {
    #[test]
    #[sage::test(crates.sage_ir.types.lifetimes.unchecked_borrows)]
    fn deferred_reference_shape() {
        // TODO: Retain builtin dereference as deferred work, then verify that
        // later constraints can resolve its operand to a reference and produce
        // a typed Deref node. Unresolved operands currently report an explicit
        // unsupported diagnostic so body checking can reach finalization.
    }
}

#[test]
#[sage::test(crates.sage_ir.types.lifetimes.unchecked_borrows)]
fn explicit_reference_dereference_preserves_ordinary_types() {
    compile_test! {
        source: "fn read(value: &u32) -> u32 { *value }\nfn round_trip(value: u32) -> u32 { *&value }\nfn mutable(value: &mut u32) -> u32 { *value }",
    }
}

#[test]
#[sage::test(crates.sage_ir.types.lifetimes.unchecked_borrows)]
fn non_reference_dereference_is_rejected() {
    compile_test! {
        source: "fn read(value: u32) { *value; } //# ERROR",
        diagnostics: expect_test::expect![[r#"
            error: dereference requires a reference type
             --> src/lib.rs:1:23
              |
            1 | fn read(value: u32) { *value; } //# ERROR
              |                       ^^^^^^"#]],
    }
}

#[test]
#[sage::test(crates.sage_ir.types.lifetimes.unchecked_borrows)]
fn references_ignore_liveness_and_overlapping_mutable_borrows() {
    // Only rustc checks the returned local's validity and overlapping borrows.
    compile_test! {
        source: r#"
fn dangling<'a>() -> &'a u32 {
    let value = 1u32;
    &value //# ERROR if rustc
}

fn overlap() {
    let mut value = 1u32;
    let first = &mut value;
    let second = &mut value; //# ERROR if rustc
    let pair = (first, second);
}
"#,
    }
}

#[test]
#[sage::test(crates.sage_ir.types.lifetimes.unchecked_borrows)]
fn erroneous_operand_does_not_add_a_dereference_error() {
    compile_test! {
        source: "fn read() { *missing; } //# ERROR",
        diagnostics: expect_test::expect![[r#"
            error: unresolved name
             --> src/lib.rs:1:14
              |
            1 | fn read() { *missing; } //# ERROR
              |              ^^^^^^^"#]],
    }
}
