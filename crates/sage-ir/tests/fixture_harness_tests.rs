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
    .resolve("Foo", Namespace::Type, expect!["<local Struct Foo>"])
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
// Phase 2: Deferred glob/redirect resolution
// ---------------------------------------------------------------------------

/// `use foo::*` where `foo` doesn't exist anywhere — flags UnresolvedGlob.
#[test]
fn unresolved_glob_path_is_an_error() {
    t(r#"
        //- /lib.rs
        use nonexistent::*;
    "#)
    .errors(expect!["UnresolvedGlob path=nonexistent"]);
}

/// A redirect target that never resolves flags UnresolvedRedirect.
#[test]
fn unresolved_redirect_is_an_error() {
    t(r#"
        //- /lib.rs
        use nonexistent::Thing;
    "#)
    .errors(expect!["UnresolvedRedirect name=Thing"]);
}

/// Named entries beat glob imports globally, even when the glob was
/// introduced by a macro expansion.
#[test]
fn named_at_root_beats_glob_from_expansion() {
    t(r#"
        //- /lib.rs
        mod other;
        struct X;
        macro_rules! m { () => { use other::*; } }
        m!();

        //- /other.rs
        pub struct X;
    "#)
    .resolve("X", Namespace::Type, expect!["<local Struct X>"])
    .errors(expect![""]);
}

/// Regression guard: named import beats same-name glob from a sibling
/// module.
#[test]
fn same_level_named_beats_glob() {
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

/// A redirect whose target is a macro-introduced item in a file-based
/// module resolves correctly.
///
/// The macro creates `X` inside `other.rs`. The redirect `use other::X`
/// walks via the MEM-map of `other`, which contains the expanded `X`.
#[test]
fn redirect_to_macro_expanded_item_in_file_module() {
    t(r#"
        //- /lib.rs
        mod other;
        use other::X;

        //- /other.rs
        macro_rules! m { () => { pub struct X; } }
        m!();
    "#)
    .resolve("X", Namespace::Type, expect!["<local Struct X>"])
    .errors(expect![""]);
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
    .memmap(&["inner"], expect!["Item Bar kind=Struct"]);
}
