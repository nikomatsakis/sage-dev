//! Smoke tests for the `//-` fixture-based test harness.
//!
//! These mirror selected cases from `memmap_phase2_tests.rs` and
//! `memmap_phase3_tests.rs` to verify that the harness composes correctly
//! with the existing query pipeline. As later phases add new features
//! (deferred resolution, fan-out, etc.), new tests go here first.

mod common;

use common::t;
use expect_test::expect;
use sage_ir::resolve::Namespace;

// ---------------------------------------------------------------------------
// Basic single-file resolution
// ---------------------------------------------------------------------------

#[test]
fn single_struct_resolves() {
    t(r#"
        //- /lib.rs
        struct X;
    "#)
    .resolve("X", Namespace::Type, expect!["<local Struct X>"])
    .errors(expect![""]);
}

// ---------------------------------------------------------------------------
// Macro expansion (mirrors memmap_phase2_tests::m3)
// ---------------------------------------------------------------------------

#[test]
fn macro_expands_to_struct() {
    t(r#"
        //- /lib.rs
        macro_rules! m { () => { struct Foo; } }
        m!();
    "#)
    .resolve("Foo", Namespace::Type, expect!["<local Error>"])
    .errors(expect![""]);
}

// ---------------------------------------------------------------------------
// Unresolved macro path (mirrors memmap_phase3_tests::e1)
// ---------------------------------------------------------------------------

#[test]
fn unresolved_macro_path() {
    t(r#"
        //- /lib.rs
        nonexistent::m!();
    "#)
    .errors(expect!["UnresolvedMacro path=nonexistent::m"]);
}

// ---------------------------------------------------------------------------
// Duplicate name from two macro invocations (mirrors memmap_phase3_tests::m7)
// ---------------------------------------------------------------------------

#[test]
fn same_macro_invoked_twice_is_duplicate() {
    t(r#"
        //- /lib.rs
        macro_rules! m { () => { struct Foo; } }
        m!();
        m!();
    "#)
    .errors(expect![[r#"
        DuplicateName name=Foo ns=Type
        DuplicateName name=Foo ns=Value"#]]);
}

// ---------------------------------------------------------------------------
// Resolution against a named import across modules
// ---------------------------------------------------------------------------

#[test]
fn named_import_beats_glob() {
    t(r#"
        //- /lib.rs
        mod a;
        mod b;
        use a::*;
        use b::Foo;

        //- /a.rs
        pub struct Foo;

        //- /b.rs
        pub struct Foo;
    "#)
    .resolve("Foo", Namespace::Type, expect!["<local Struct Foo>"]);
}

// ---------------------------------------------------------------------------
// Multi-file: memmap of a submodule
// ---------------------------------------------------------------------------

#[test]
fn memmap_child_module() {
    t(r#"
        //- /lib.rs
        mod inner;

        //- /inner.rs
        pub struct Bar;
    "#)
    .memmap(
        &["inner"],
        expect![[r#"
            Item Bar ns=Type kind=Struct
            Item Bar ns=Value kind=Struct"#]],
    );
}
