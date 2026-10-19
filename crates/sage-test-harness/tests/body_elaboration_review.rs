#![feature(rustc_private)]
#![feature(register_tool)]
#![register_tool(sage)]

use sage_test_harness::{local_function_named, with_test_crate};

#[test]
#[sage::test(crates.sage_ir.checking.body.elaboration)]
fn indexing_reports_unsupported_until_call_elaboration_exists() {
    with_test_crate("fn f(x: &[i32]) -> i32 { x[0usize] }", |db, root| {
        let checked = local_function_named(db, root, "f").body(db);
        // Successful indexing elaboration requires materialized Index/IndexMut calls.
        assert!(
            checked.diagnostics.iter().any(|diagnostic| diagnostic.message
                == "indexing elaboration is not supported yet"),
            "unsupported indexing must not masquerade as successful elaboration: {:?}",
            checked.diagnostics
        );
    });
}
