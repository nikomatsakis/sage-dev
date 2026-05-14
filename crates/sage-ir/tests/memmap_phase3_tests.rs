//! Phase 3 MEM-map tests: validation query (memmap_errors).
//!
//! Tests cover: duplicate non-glob names, unresolved macros,
//! ambiguous macro resolution.

use sage_ir::db::Database;
use sage_ir::item::ModAst;
use sage_ir::memmap::{MemmapError, memmap_errors};
use sage_ir::module::ModSymbol;
use sage_ir::resolve::SourceRoot;
use sage_ir::source::SourceFile;
use salsa::Database as _;

fn setup_single<'db>(db: &'db Database, code: &str) -> (SourceRoot, ModSymbol<'db>) {
    let file = SourceFile::new(db, "lib.rs".to_owned(), code.to_owned());
    let source_root = SourceRoot::new(db, vec![file]);
    let root_module = ModSymbol::ast(ModAst::crate_root(db, file));
    (source_root, root_module)
}

fn setup_files<'db>(db: &'db Database, files: &[(&str, &str)]) -> (SourceRoot, ModSymbol<'db>) {
    let source_files: Vec<_> = files
        .iter()
        .map(|(path, text)| SourceFile::new(db, path.to_string(), text.to_string()))
        .collect();
    let source_root = SourceRoot::new(db, source_files.clone());
    let lib_file = source_files
        .iter()
        .find(|f| f.path(db) == "lib.rs")
        .copied()
        .expect("must have lib.rs");
    let root_module = ModSymbol::ast(ModAst::crate_root(db, lib_file));
    (source_root, root_module)
}

// ---------------------------------------------------------------------------
// P6: Named import + macro-expanded item, same name — duplicate error
// ---------------------------------------------------------------------------

#[test]
fn p6_named_import_plus_macro_expanded_same_name() {
    let db = Database::default();
    db.attach(|db| {
        let (source_root, root_module) = setup_files(
            db,
            &[
                (
                    "lib.rs",
                    r#"
mod other;
use other::Foo;
macro_rules! m { () => { struct Foo; } }
m!();
"#,
                ),
                ("other.rs", "pub struct Foo;"),
            ],
        );

        let errors = memmap_errors(db, root_module, source_root);
        let has_duplicate = errors
            .iter()
            .any(|e| matches!(e, MemmapError::DuplicateName { .. }));
        assert!(
            has_duplicate,
            "should report duplicate name error for Foo, got: {:?}",
            errors
        );
    });
}

// ---------------------------------------------------------------------------
// E1: Unresolved macro path — error
// ---------------------------------------------------------------------------

#[test]
fn e1_unresolved_macro_path() {
    let db = Database::default();
    db.attach(|db| {
        let (source_root, root_module) = setup_single(
            db,
            r#"
nonexistent::m!();
"#,
        );

        let errors = memmap_errors(db, root_module, source_root);
        let has_unresolved = errors
            .iter()
            .any(|e| matches!(e, MemmapError::UnresolvedMacro { .. }));
        assert!(
            has_unresolved,
            "should report unresolved macro error, got: {:?}",
            errors
        );
    });
}

// ---------------------------------------------------------------------------
// M7: Same macro invoked twice = duplicate names
// ---------------------------------------------------------------------------

#[test]
fn m7_same_macro_invoked_twice_duplicate() {
    let db = Database::default();
    db.attach(|db| {
        let (source_root, root_module) = setup_single(
            db,
            r#"
macro_rules! m { () => { struct Foo; } }
m!();
m!();
"#,
        );

        let errors = memmap_errors(db, root_module, source_root);
        let has_duplicate = errors
            .iter()
            .any(|e| matches!(e, MemmapError::DuplicateName { .. }));
        assert!(
            has_duplicate,
            "should report duplicate name error for Foo from two expansions, got: {:?}",
            errors
        );
    });
}

// ---------------------------------------------------------------------------
// T1: Classic time-travel (E0659) — macro expansion introduces non-glob name
//     that conflicts with a glob-imported name
// ---------------------------------------------------------------------------

#[test]
fn t1_time_travel_violation() {
    let db = Database::default();
    db.attach(|db| {
        let (source_root, root_module) = setup_files(
            db,
            &[
                (
                    "lib.rs",
                    r#"
mod foo;
use foo::*;
bar::m!();
"#,
                ),
                (
                    "foo.rs",
                    r#"
pub mod bar;
pub mod baz;
"#,
                ),
                (
                    "foo/bar.rs",
                    r#"
macro_rules! m { () => { mod baz { pub struct X; } } }
pub(crate) use m;
"#,
                ),
                ("foo/baz.rs", "pub struct X;"),
            ],
        );

        let errors = memmap_errors(db, root_module, source_root);
        let has_time_travel = errors
            .iter()
            .any(|e| matches!(e, MemmapError::TimeTravelViolation { .. }));
        assert!(
            has_time_travel,
            "should report time-travel violation for baz, got: {:?}",
            errors
        );
    });
}

// ---------------------------------------------------------------------------
// T3: Multiple macro candidates — ambiguous resolution
// ---------------------------------------------------------------------------

#[test]
fn t3_ambiguous_macro_resolution() {
    let db = Database::default();
    db.attach(|db| {
        let (source_root, root_module) = setup_files(
            db,
            &[
                (
                    "lib.rs",
                    r#"
mod a;
mod b;
use a::*;
use b::*;
m!();
"#,
                ),
                (
                    "a.rs",
                    r#"
macro_rules! m { () => { struct FromA; } }
pub(crate) use m;
"#,
                ),
                (
                    "b.rs",
                    r#"
macro_rules! m { () => { struct FromB; } }
pub(crate) use m;
"#,
                ),
            ],
        );

        let errors = memmap_errors(db, root_module, source_root);
        let has_ambiguous = errors
            .iter()
            .any(|e| matches!(e, MemmapError::AmbiguousMacro { .. }));
        assert!(
            has_ambiguous,
            "should report ambiguous macro error for m, got: {:?}",
            errors
        );
    });
}

// ---------------------------------------------------------------------------
// No errors for valid code
// ---------------------------------------------------------------------------

#[test]
fn no_errors_for_valid_expansion() {
    let db = Database::default();
    db.attach(|db| {
        let (source_root, root_module) = setup_single(
            db,
            r#"
macro_rules! m { () => { struct Foo; } }
m!();
"#,
        );

        let errors = memmap_errors(db, root_module, source_root);
        assert!(
            errors.is_empty(),
            "valid code should have no errors, got: {:?}",
            errors
        );
    });
}
