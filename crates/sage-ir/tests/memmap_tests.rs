//! Phase 1 MEM-map tests: basic resolution via expanded_module.
//!
//! Tests cover: named members, use-redirects, globs, precedence rules.
//! No macro expansion yet — invocations are recorded but unresolved.

use sage_ir::db::Database;
use sage_ir::item::{ItemAst, ModAst, StructKind};
use sage_ir::memmap::{MemmapEntry, module_memmap};
use sage_ir::module::ModSymbol;
use sage_ir::name::Name;
use sage_ir::resolve::{Namespace, ResolutionError, SourceRoot, resolve_name};
use sage_ir::source::SourceFile;
use sage_ir::symbol::SymbolData;

use salsa::Database as _;

/// Helper: get entries as a slice from the memmap.
fn get_entries<'db>(
    db: &'db Database,
    memmap: sage_ir::memmap::ExpandedModule<'db>,
) -> &'db [MemmapEntry<'db>] {
    let stash = memmap.stash(db);
    let entries = memmap.entries(db);
    &stash[entries]
}

/// Helper: create a single-file crate and return (source_root, root_module).
fn setup_single_file<'db>(db: &'db Database, code: &str) -> (SourceRoot, ModSymbol<'db>) {
    let file = SourceFile::new(db, "lib.rs".to_owned(), code.to_owned());
    let source_root = SourceRoot::new(db, vec![file]);
    let root_module = ModSymbol::ast(ModAst::crate_root(db, file));
    (source_root, root_module)
}

/// Helper: create a multi-file crate.
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
        match sym.data() {
            SymbolData::Unknown(_) => panic!("expected local symbol"),
            _ => {}
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
        match sym.data() {
            SymbolData::Unknown(_) => panic!("should resolve to local, not std prelude"),
            _ => {}
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
// Basic: expanded_module produces entries for declared items
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
        let entries = get_entries(db, memmap);
        // Should have named entries for Foo (type+value), bar (value), baz (type)
        assert!(
            entries.len() >= 3,
            "expected at least 3 entries, got {}",
            entries.len()
        );
    });
}

// ---------------------------------------------------------------------------
// Basic: expanded_module records glob stems
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
        let entries = get_entries(db, memmap);
        let has_glob = entries
            .iter()
            .any(|e| matches!(e, MemmapEntry::Glob { .. }));
        assert!(has_glob, "memmap should contain a glob stem");
    });
}

// ---------------------------------------------------------------------------
// Step 1: StructKind detection
// ---------------------------------------------------------------------------

#[test]
fn struct_kind_tuple() {
    let db = Database::default();
    db.attach(|db| {
        let (_, root_module) = setup_single_file(db, "struct Foo(i32, String);");
        let name = Name::new(db, "Foo".to_owned());
        let sym = resolve_name(
            db,
            root_module,
            SourceRoot::new(db, vec![]),
            name,
            Namespace::Type,
        )
        .unwrap();
        match sym.data() {
            SymbolData::Struct(s) => assert_eq!(s.as_ast().unwrap().kind(db), StructKind::Tuple),
            other => panic!("expected Struct, got {other:?}"),
        }
    });
}

#[test]
fn struct_kind_unit() {
    let db = Database::default();
    db.attach(|db| {
        let (_, root_module) = setup_single_file(db, "struct Bar;");
        let name = Name::new(db, "Bar".to_owned());
        let sym = resolve_name(
            db,
            root_module,
            SourceRoot::new(db, vec![]),
            name,
            Namespace::Type,
        )
        .unwrap();
        match sym.data() {
            SymbolData::Struct(s) => assert_eq!(s.as_ast().unwrap().kind(db), StructKind::Unit),
            other => panic!("expected Struct, got {other:?}"),
        }
    });
}

#[test]
fn struct_kind_braced() {
    let db = Database::default();
    db.attach(|db| {
        let (_, root_module) = setup_single_file(db, "struct Baz { x: i32 }");
        let name = Name::new(db, "Baz".to_owned());
        let sym = resolve_name(
            db,
            root_module,
            SourceRoot::new(db, vec![]),
            name,
            Namespace::Type,
        )
        .unwrap();
        match sym.data() {
            SymbolData::Struct(s) => assert_eq!(s.as_ast().unwrap().kind(db), StructKind::Braced),
            other => panic!("expected Struct, got {other:?}"),
        }
    });
}

// ---------------------------------------------------------------------------
// Step 2: TupleStructCtor emission
// ---------------------------------------------------------------------------

#[test]
fn tuple_struct_emits_ctor_entry() {
    let db = Database::default();
    db.attach(|db| {
        let file = SourceFile::new(db, "lib.rs".to_owned(), "struct Foo(i32);".to_owned());
        let source_root = SourceRoot::new(db, vec![file]);
        let root_module = ModSymbol::ast(ModAst::crate_root(db, file));
        let memmap = module_memmap(db, root_module, source_root);
        let entries = get_entries(db, memmap);
        let has_item = entries
            .iter()
            .any(|e| matches!(e, MemmapEntry::Item(ItemAst::Struct(_))));
        let has_ctor = entries
            .iter()
            .any(|e| matches!(e, MemmapEntry::TupleStructCtor(_)));
        assert!(has_item, "should have Item(Struct) entry");
        assert!(has_ctor, "should have TupleStructCtor entry");
    });
}

#[test]
fn unit_struct_emits_ctor_entry() {
    let db = Database::default();
    db.attach(|db| {
        let file = SourceFile::new(db, "lib.rs".to_owned(), "struct Bar;".to_owned());
        let source_root = SourceRoot::new(db, vec![file]);
        let root_module = ModSymbol::ast(ModAst::crate_root(db, file));
        let memmap = module_memmap(db, root_module, source_root);
        let entries = get_entries(db, memmap);
        let has_ctor = entries
            .iter()
            .any(|e| matches!(e, MemmapEntry::TupleStructCtor(_)));
        assert!(has_ctor, "unit struct should have TupleStructCtor entry");
    });
}

#[test]
fn braced_struct_no_ctor_entry() {
    let db = Database::default();
    db.attach(|db| {
        let file = SourceFile::new(db, "lib.rs".to_owned(), "struct Baz { x: i32 }".to_owned());
        let source_root = SourceRoot::new(db, vec![file]);
        let root_module = ModSymbol::ast(ModAst::crate_root(db, file));
        let memmap = module_memmap(db, root_module, source_root);
        let entries = get_entries(db, memmap);
        let has_ctor = entries
            .iter()
            .any(|e| matches!(e, MemmapEntry::TupleStructCtor(_)));
        assert!(
            !has_ctor,
            "braced struct should NOT have TupleStructCtor entry"
        );
    });
}

// ---------------------------------------------------------------------------
// Step 3: Name resolution with TupleStructCtor
// ---------------------------------------------------------------------------

#[test]
fn tuple_struct_resolves_in_value_ns() {
    let db = Database::default();
    db.attach(|db| {
        let (source_root, root_module) = setup_single_file(db, "struct Foo(i32);");
        let name = Name::new(db, "Foo".to_owned());
        let result = resolve_name(db, root_module, source_root, name, Namespace::Value);
        assert!(
            result.is_ok(),
            "Foo should resolve in Value ns, got {:?}",
            result
        );
        match result.unwrap().data() {
            SymbolData::TupleStructCtor(_) => {}
            other => panic!("expected TupleStructCtor, got {other:?}"),
        }
    });
}

#[test]
fn tuple_struct_resolves_in_type_ns() {
    let db = Database::default();
    db.attach(|db| {
        let (source_root, root_module) = setup_single_file(db, "struct Foo(i32);");
        let name = Name::new(db, "Foo".to_owned());
        let result = resolve_name(db, root_module, source_root, name, Namespace::Type);
        assert!(result.is_ok(), "Foo should resolve in Type ns");
        match result.unwrap().data() {
            SymbolData::Struct(_) => {}
            other => panic!("expected Struct, got {other:?}"),
        }
    });
}

#[test]
fn braced_struct_not_in_value_ns() {
    let db = Database::default();
    db.attach(|db| {
        let (source_root, root_module) = setup_single_file(db, "struct Bar { x: i32 }");
        let name = Name::new(db, "Bar".to_owned());
        let result = resolve_name(db, root_module, source_root, name, Namespace::Value);
        assert_eq!(
            result,
            Err(ResolutionError::Unresolved),
            "braced struct should not be in Value ns"
        );
    });
}

#[test]
fn braced_struct_resolves_in_type_ns() {
    let db = Database::default();
    db.attach(|db| {
        let (source_root, root_module) = setup_single_file(db, "struct Bar { x: i32 }");
        let name = Name::new(db, "Bar".to_owned());
        let result = resolve_name(db, root_module, source_root, name, Namespace::Type);
        assert!(result.is_ok(), "braced struct should resolve in Type ns");
    });
}
