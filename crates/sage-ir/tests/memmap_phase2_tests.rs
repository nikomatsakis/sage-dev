//! Phase 2 MEM-map tests: macro expansion in the fixpoint.
//!
//! Tests cover: macro_rules! lowering, expand_macro, resolve_memmap_path,
//! fixpoint convergence, depth limit.

use sage_ir::db::Database;
use sage_ir::memmap::{MacroUseState, MemmapEntry, module_memmap};
use sage_ir::module::{Module, ModuleSource};
use sage_ir::name::Name;
use sage_ir::resolve::{Namespace, SourceRoot, resolve_name};
use sage_ir::source::SourceFile;
use sage_ir::symbol::SymbolSource;
use salsa::Database as _;

/// Helper: create a multi-file crate.
fn setup_files<'db>(db: &'db Database, files: &[(&str, &str)]) -> (SourceRoot, Module<'db>) {
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
    let root_module = Module::new(
        db,
        ModuleSource::Local {
            file: lib_file,
            parent: None,
        },
    );
    (source_root, root_module)
}

fn setup_single<'db>(db: &'db Database, code: &str) -> (SourceRoot, Module<'db>) {
    let file = SourceFile::new(db, "lib.rs".to_owned(), code.to_owned());
    let source_root = SourceRoot::new(db, vec![file]);
    let root_module = Module::new(db, ModuleSource::Local { file, parent: None });
    (source_root, root_module)
}

// ---------------------------------------------------------------------------
// M3: Macro expands to a struct (basic expansion)
// ---------------------------------------------------------------------------

#[test]
fn m3_macro_expands_to_struct() {
    let db = Database::default();
    db.attach(|db| {
        let (source_root, root_module) = setup_single(
            db,
            r#"
macro_rules! m { () => { struct Foo; } }
m!();
"#,
        );

        let name = Name::new(db, "Foo".to_owned());
        let result = resolve_name(
            db,
            root_module,
            source_root,
            root_module,
            name,
            Namespace::Type,
        );
        assert!(
            result.is_ok(),
            "Foo should resolve from macro expansion, got {:?}",
            result
        );
        match result.unwrap().source(db) {
            SymbolSource::Local(_) => {} // good
            _ => panic!("expected local symbol"),
        }
    });
}

// ---------------------------------------------------------------------------
// M5: Macro expansion produces only impl (anonymous)
// ---------------------------------------------------------------------------

#[test]
fn m5_macro_expands_to_impl_only() {
    let db = Database::default();
    db.attach(|db| {
        let (source_root, root_module) = setup_single(
            db,
            r#"
struct Foo;
macro_rules! m { () => { impl Foo { fn bar() {} } } }
m!();
"#,
        );

        // m!() expands to an impl — no new names introduced
        let memmap = module_memmap(db, root_module, source_root, root_module);
        let macro_uses: Vec<_> = memmap
            .entries(db)
            .iter()
            .filter_map(|e| match e {
                MemmapEntry::MacroUse(mu) => Some(mu),
                _ => None,
            })
            .collect();
        assert_eq!(macro_uses.len(), 1);
        match &macro_uses[0].state {
            MacroUseState::Expanded(entries) => {
                // Should have one Anon entry (the impl)
                assert_eq!(entries.len(), 1);
                assert!(matches!(entries[0], MemmapEntry::Anon(_)));
            }
            other => panic!("expected Expanded, got {:?}", other),
        }
    });
}

// ---------------------------------------------------------------------------
// M6: Empty macro expansion
// ---------------------------------------------------------------------------

#[test]
fn m6_empty_macro_expansion() {
    let db = Database::default();
    db.attach(|db| {
        let (source_root, root_module) = setup_single(
            db,
            r#"
macro_rules! m { () => {} }
m!();
"#,
        );

        let memmap = module_memmap(db, root_module, source_root, root_module);
        let macro_uses: Vec<_> = memmap
            .entries(db)
            .iter()
            .filter_map(|e| match e {
                MemmapEntry::MacroUse(mu) => Some(mu),
                _ => None,
            })
            .collect();
        assert_eq!(macro_uses.len(), 1);
        match &macro_uses[0].state {
            MacroUseState::Expanded(entries) => {
                assert!(entries.is_empty(), "empty macro should expand to nothing");
            }
            other => panic!("expected Expanded, got {:?}", other),
        }
    });
}

// ---------------------------------------------------------------------------
// R1: self:: path for macro resolution
// ---------------------------------------------------------------------------

#[test]
fn r1_self_path_macro_resolution() {
    let db = Database::default();
    db.attach(|db| {
        let (source_root, root_module) = setup_single(
            db,
            r#"
macro_rules! m { () => { struct Foo; } }
self::m!();
"#,
        );

        let name = Name::new(db, "Foo".to_owned());
        let result = resolve_name(
            db,
            root_module,
            source_root,
            root_module,
            name,
            Namespace::Type,
        );
        assert!(
            result.is_ok(),
            "Foo should resolve via self::m!(), got {:?}",
            result
        );
    });
}

// ---------------------------------------------------------------------------
// R6: Multi-segment path through nested modules
// ---------------------------------------------------------------------------

#[test]
fn r6_multi_segment_path_macro() {
    let db = Database::default();
    db.attach(|db| {
        let (source_root, root_module) = setup_files(
            db,
            &[
                (
                    "lib.rs",
                    r#"
mod a;
a::b::m!();
"#,
                ),
                (
                    "a.rs",
                    r#"
pub mod b;
"#,
                ),
                (
                    "a/b.rs",
                    r#"
macro_rules! m { () => { struct Deep; } }
pub(crate) use m;
"#,
                ),
            ],
        );

        let name = Name::new(db, "Deep".to_owned());
        let result = resolve_name(
            db,
            root_module,
            source_root,
            root_module,
            name,
            Namespace::Type,
        );
        assert!(
            result.is_ok(),
            "Deep should resolve via a::b::m!(), got {:?}",
            result
        );
    });
}

// ---------------------------------------------------------------------------
// E3: Depth limit exceeded (recursive macro)
// ---------------------------------------------------------------------------

#[test]
fn e3_depth_limit_exceeded() {
    let db = Database::default();
    db.attach(|db| {
        let (source_root, root_module) = setup_single(
            db,
            r#"
macro_rules! m { () => { m!(); } }
m!();
"#,
        );

        let memmap = module_memmap(db, root_module, source_root, root_module);
        // The recursive expansion should hit the depth limit somewhere in the tree
        fn has_error_recursive(entries: &[MemmapEntry]) -> bool {
            for entry in entries {
                if let MemmapEntry::MacroUse(mu) = entry {
                    match &mu.state {
                        MacroUseState::Error => return true,
                        MacroUseState::Expanded(sub) => {
                            if has_error_recursive(sub) {
                                return true;
                            }
                        }
                        _ => {}
                    }
                }
            }
            false
        }
        assert!(
            has_error_recursive(memmap.entries(db)),
            "recursive macro should hit depth limit and produce Error somewhere in the tree"
        );
    });
}
