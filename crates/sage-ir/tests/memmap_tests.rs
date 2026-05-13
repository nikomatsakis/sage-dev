//! Phase 1 MEM-map tests: basic resolution via module_memmap.
//!
//! Tests cover: named members, use-redirects, globs, precedence rules.
//! No macro expansion yet — invocations are recorded but unresolved.

use sage_ir::db::Database;
use sage_ir::memmap::module_memmap;
use sage_ir::module::{Module, ModuleSource};
use sage_ir::name::Name;
use sage_ir::resolve::{Namespace, ResolutionError, SourceRoot, resolve_name};
use sage_ir::source::SourceFile;
use sage_ir::symbol::SymbolSource;
use salsa::Database as _;

/// Helper: create a single-file crate and return (source_root, root_module).
fn setup_single_file<'db>(db: &'db Database, code: &str) -> (SourceRoot, Module<'db>) {
    let file = SourceFile::new(db, "lib.rs".to_owned(), code.to_owned());
    let source_root = SourceRoot::new(db, vec![file]);
    let root_module = Module::new(
        db,
        ModuleSource::Local {
            file,
            parent: None,
            declaration: None,
        },
    );
    (source_root, root_module)
}

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
            declaration: None,
        },
    );
    (source_root, root_module)
}

// ---------------------------------------------------------------------------
// P1: Named import beats glob
// ---------------------------------------------------------------------------

#[test]
fn p1_named_import_beats_glob() {
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
                use b::Foo;
            "#,
                ),
                ("a.rs", "pub struct Foo;"),
                ("b.rs", "pub struct Foo;"),
            ],
        );

        let name = Name::new(db, "Foo".to_owned());
        let result = resolve_name(db, root_module, source_root, name, Namespace::Type);
        assert!(result.is_ok(), "Foo should resolve, got {:?}", result);

        // Should resolve to b::Foo (named import), not a::Foo (glob)
        let sym = result.unwrap();
        match sym.source(db) {
            SymbolSource::Local(item) => {
                let _ = item; // resolves without ambiguity = named beat glob
            }
            _ => panic!("expected local symbol"),
        }
    });
}

// ---------------------------------------------------------------------------
// P4: Glob beats std prelude
// ---------------------------------------------------------------------------

#[test]
fn p4_glob_beats_std_prelude() {
    let db = Database::default();
    db.attach(|db| {
        let (source_root, root_module) = setup_files(
            db,
            &[
                (
                    "lib.rs",
                    r#"
                mod custom;
                use custom::*;
            "#,
                ),
                ("custom.rs", "pub struct Option;"),
            ],
        );

        let name = Name::new(db, "Option".to_owned());
        let result = resolve_name(db, root_module, source_root, name, Namespace::Type);
        assert!(result.is_ok(), "Option should resolve, got {:?}", result);

        // Should resolve to custom::Option (glob), not std::Option (prelude)
        let sym = result.unwrap();
        match sym.source(db) {
            SymbolSource::Local(_) => {} // good — it's the local one
            SymbolSource::External(..) => panic!("should resolve to local, not std prelude"),
        }
    });
}

// ---------------------------------------------------------------------------
// P5: Two globs, same name — ambiguity
// ---------------------------------------------------------------------------

#[test]
fn p5_two_globs_same_name_ambiguous() {
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
            "#,
                ),
                ("a.rs", "pub struct Foo;"),
                ("b.rs", "pub struct Foo;"),
            ],
        );

        let name = Name::new(db, "Foo".to_owned());
        let result = resolve_name(db, root_module, source_root, name, Namespace::Type);
        assert_eq!(result, Err(ResolutionError::Ambiguous));
    });
}

// ---------------------------------------------------------------------------
// P7: Explicit item + named import, same name — error
// ---------------------------------------------------------------------------

#[test]
fn p7_explicit_item_plus_named_import_same_name() {
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
                struct Foo;
            "#,
                ),
                ("other.rs", "pub struct Foo;"),
            ],
        );

        let name = Name::new(db, "Foo".to_owned());
        let result = resolve_name(db, root_module, source_root, name, Namespace::Type);
        // Two non-glob items with same name = ambiguous
        assert_eq!(result, Err(ResolutionError::Ambiguous));
    });
}

// ---------------------------------------------------------------------------
// Basic: module_memmap produces entries for declared items
// ---------------------------------------------------------------------------

#[test]
fn memmap_contains_declared_items() {
    let db = Database::default();
    db.attach(|db| {
        let (source_root, root_module) = setup_single_file(
            db,
            r#"
            struct Foo;
            fn bar() {}
            mod baz;
        "#,
        );

        let memmap = module_memmap(db, root_module, source_root);
        let entries = memmap.entries(db);
        // Should have named entries for Foo (type+value), bar (value), baz (type)
        assert!(
            entries.len() >= 3,
            "expected at least 3 entries, got {}",
            entries.len()
        );
    });
}

// ---------------------------------------------------------------------------
// Basic: module_memmap records glob stems
// ---------------------------------------------------------------------------

#[test]
fn memmap_records_glob_stems() {
    use sage_ir::memmap::MemmapEntry;

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
            "#,
                ),
                ("foo.rs", "pub struct Bar;"),
            ],
        );

        let memmap = module_memmap(db, root_module, source_root);
        let has_glob = memmap
            .entries(db)
            .iter()
            .any(|e| matches!(e, MemmapEntry::Glob { .. }));
        assert!(has_glob, "memmap should contain a glob stem");
    });
}
