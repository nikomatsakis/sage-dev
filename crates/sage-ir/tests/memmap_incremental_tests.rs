//! Phase 0: Baseline incremental tests for module_memmap.
//!
//! These tests lock in current incremental behavior using `expect_test` snapshots.
//! Any reorganization that changes which queries re-execute must update the snapshot,
//! making regressions immediately visible.

use expect_test::{Expect, expect};
use sage_ir::db::Database;
use sage_ir::memmap::module_memmap;
use sage_ir::module::{Module, ModuleSource};
use sage_ir::resolve::SourceRoot;
use sage_ir::source::SourceFile;
use salsa::{Database as _, Setter};

fn check_log(actual: &str, expected: Expect) {
    let filtered: String = actual
        .lines()
        .filter(|l| {
            l.contains("file_item_tree")
                || l.contains("module_memmap")
                || l.contains("module_items")
        })
        .collect::<Vec<_>>()
        .join("\n");
    expected.assert_eq(&filtered);
}

/// Snapshot the query log for a fresh module_memmap computation.
#[test]
fn baseline_initial_memmap_computation() {
    let db = Database::default();
    db.attach(|db| {
        let file = SourceFile::new(
            db,
            "lib.rs".to_owned(),
            "fn hello() {}\nmacro_rules! m { () => { struct Foo; } }\nm!();\n".to_owned(),
        );
        let source_root = SourceRoot::new(db, vec![file]);
        let root_module = Module::new(
            db,
            ModuleSource::Local {
                file,
                parent: None,
                declaration: None,
            },
        );

        let _ = module_memmap(db, root_module, source_root, root_module);
        let log = db.take_query_log();
        check_log(
            &log,
            expect![[r#"
              salsa: module_memmap(Id(c00))
              salsa: file_item_tree(Id(0))
            file_item_tree("lib.rs")
              salsa: file_item_tree(Id(1))
            file_item_tree("<macro:m>")"#]],
        );
    });
}

/// Snapshot the query log when a function body changes.
/// After Phase 2: module_memmap does NOT re-execute — only file_item_tree does.
/// This is the key incremental improvement: body edits don't invalidate the memmap.
#[test]
fn baseline_body_change_behavior() {
    let mut db = Database::default();

    // Create the input outside attach (SourceFile is #[salsa::input], no 'db lifetime)
    let file = SourceFile::new(
        &db,
        "lib.rs".to_owned(),
        "fn hello() { 1 }\nmacro_rules! m { () => { struct Foo; } }\nm!();\n".to_owned(),
    );
    let source_root = SourceRoot::new(&db, vec![file]);

    // Initial computation
    db.attach(|db| {
        let root_module = Module::new(
            db,
            ModuleSource::Local {
                file,
                parent: None,
                declaration: None,
            },
        );
        let _ = module_memmap(db, root_module, source_root, root_module);
        db.take_query_log(); // drain
    });

    // Mutate body only
    file.set_text(&mut db)
        .to("fn hello() { 2 }\nmacro_rules! m { () => { struct Foo; } }\nm!();\n".to_owned());

    // Re-query and snapshot
    db.attach(|db| {
        // Re-intern the module (same data → same ID)
        let root_module = Module::new(
            db,
            ModuleSource::Local {
                file,
                parent: None,
                declaration: None,
            },
        );
        let _ = module_memmap(db, root_module, source_root, root_module);
        let log = db.take_query_log();
        check_log(
            &log,
            expect![[r#"
                  salsa: file_item_tree(Id(0))
                file_item_tree("lib.rs")"#]],
        );
    });
}

/// Snapshot the query log when a sibling module changes.
/// module_memmap(a) should NOT re-execute when b changes.
#[test]
fn baseline_sibling_module_isolation() {
    let mut db = Database::default();

    let file_a = SourceFile::new(&db, "a.rs".to_owned(), "fn foo() { 1 }".to_owned());
    let file_b = SourceFile::new(&db, "b.rs".to_owned(), "fn bar() { 1 }".to_owned());
    let lib_file = SourceFile::new(&db, "lib.rs".to_owned(), "mod a;\nmod b;\n".to_owned());
    let source_root = SourceRoot::new(&db, vec![lib_file, file_a, file_b]);

    // Initial computation
    db.attach(|db| {
        let root_module = Module::new(
            db,
            ModuleSource::Local {
                file: lib_file,
                parent: None,
                declaration: None,
            },
        );
        let module_a = Module::new(
            db,
            ModuleSource::Local {
                file: file_a,
                parent: Some(root_module),
                declaration: None,
            },
        );
        let _ = module_memmap(db, module_a, source_root, root_module);
        db.take_query_log(); // drain
    });

    // Change module b
    file_b.set_text(&mut db).to("fn bar() { 2 }".to_owned());

    // Re-query module_a and snapshot
    db.attach(|db| {
        let root_module = Module::new(
            db,
            ModuleSource::Local {
                file: lib_file,
                parent: None,
                declaration: None,
            },
        );
        let module_a = Module::new(
            db,
            ModuleSource::Local {
                file: file_a,
                parent: Some(root_module),
                declaration: None,
            },
        );
        let _ = module_memmap(db, module_a, source_root, root_module);
        let log = db.take_query_log();
        check_log(&log, expect![[r#""#]]);
    });
}

// ===========================================================================
// Phase 2 incremental tests: verify the key improvements from using
// file_item_tree as the incremental firewall.
// ===========================================================================

/// Body-only edits don't invalidate the memmap.
///
/// The item tree's `FunctionItem` is identified by name, so a body-only change
/// preserves the tracked-struct identity. `module_memmap` reads only the name,
/// so it stays cached.
#[test]
fn body_change_does_not_invalidate_memmap() {
    let mut db = Database::default();

    let file = SourceFile::new(&db, "lib.rs".to_owned(), "fn foo() { 1 }".to_owned());
    let source_root = SourceRoot::new(&db, vec![file]);

    db.attach(|db| {
        let root_module = Module::new(
            db,
            ModuleSource::Local {
                file,
                parent: None,
                declaration: None,
            },
        );
        let _ = module_memmap(db, root_module, source_root, root_module);
        db.take_query_log();
    });

    file.set_text(&mut db).to("fn foo() { 2 }".to_owned());

    db.attach(|db| {
        let root_module = Module::new(
            db,
            ModuleSource::Local {
                file,
                parent: None,
                declaration: None,
            },
        );
        let _ = module_memmap(db, root_module, source_root, root_module);
        let log = db.take_query_log();

        assert!(
            log.contains("file_item_tree"),
            "file_item_tree should re-execute after body change, got log:\n{log}"
        );
        assert!(
            !log.contains("module_memmap"),
            "module_memmap should NOT re-execute after body-only change, got log:\n{log}"
        );
    });
}

/// Signature changes (renaming a function) invalidate the memmap.
///
/// Renaming changes the `FunctionItem`'s identity (its `name` field), so the
/// old tracked struct is deleted and a new one is created. `module_memmap`
/// reads names, so it must re-execute.
#[test]
fn signature_change_invalidates_memmap() {
    let mut db = Database::default();

    let file = SourceFile::new(&db, "lib.rs".to_owned(), "fn foo() {}".to_owned());
    let source_root = SourceRoot::new(&db, vec![file]);

    db.attach(|db| {
        let root_module = Module::new(
            db,
            ModuleSource::Local {
                file,
                parent: None,
                declaration: None,
            },
        );
        let _ = module_memmap(db, root_module, source_root, root_module);
        db.take_query_log();
    });

    // Rename foo → bar (changes the identity of the FunctionItem)
    file.set_text(&mut db).to("fn bar() {}".to_owned());

    db.attach(|db| {
        let root_module = Module::new(
            db,
            ModuleSource::Local {
                file,
                parent: None,
                declaration: None,
            },
        );
        let _ = module_memmap(db, root_module, source_root, root_module);
        let log = db.take_query_log();

        assert!(
            log.contains("file_item_tree"),
            "file_item_tree should re-execute, got log:\n{log}"
        );
        assert!(
            log.contains("module_memmap"),
            "module_memmap should re-execute after rename, got log:\n{log}"
        );
    });
}

/// Adding a use statement invalidates the memmap.
///
/// A new `UseGroup` item changes the item tree's shape, so `module_memmap`
/// must re-execute to pick up the new entry.
#[test]
fn adding_use_statement_invalidates_memmap() {
    let mut db = Database::default();

    let file = SourceFile::new(&db, "lib.rs".to_owned(), "struct S;".to_owned());
    let source_root = SourceRoot::new(&db, vec![file]);

    db.attach(|db| {
        let root_module = Module::new(
            db,
            ModuleSource::Local {
                file,
                parent: None,
                declaration: None,
            },
        );
        let _ = module_memmap(db, root_module, source_root, root_module);
        db.take_query_log();
    });

    file.set_text(&mut db)
        .to("use foo::Bar;\nstruct S;".to_owned());

    db.attach(|db| {
        let root_module = Module::new(
            db,
            ModuleSource::Local {
                file,
                parent: None,
                declaration: None,
            },
        );
        let _ = module_memmap(db, root_module, source_root, root_module);
        let log = db.take_query_log();

        assert!(
            log.contains("module_memmap"),
            "module_memmap should re-execute when a use statement is added, got log:\n{log}"
        );
    });
}

/// `module_memmap` calls `file_item_tree` exactly once (no double-parse).
///
/// Before Phase 2, `seed_from_cst` parsed tree-sitter AND `file_item_tree`
/// was memoized separately — effectively two parses. After Phase 2, only
/// `file_item_tree` parses the source.
#[test]
fn module_memmap_calls_file_item_tree_only_once() {
    let db = Database::default();
    db.attach(|db| {
        let file = SourceFile::new(
            db,
            "lib.rs".to_owned(),
            "fn hello() {}\nmacro_rules! m { () => { struct Foo; } }\nm!();\n".to_owned(),
        );
        let source_root = SourceRoot::new(db, vec![file]);
        let root_module = Module::new(
            db,
            ModuleSource::Local {
                file,
                parent: None,
                declaration: None,
            },
        );

        let _ = module_memmap(db, root_module, source_root, root_module);
        let log = db.take_query_log();

        // Count file_item_tree invocations for lib.rs. There should be exactly one.
        let count = log
            .lines()
            .filter(|l| l.contains("file_item_tree(\"lib.rs\")"))
            .count();
        assert_eq!(
            count, 1,
            "expected exactly 1 file_item_tree call for lib.rs, got {count}. Log:\n{log}"
        );
    });
}

/// A body change in sibling module `b` doesn't invalidate `module_memmap(a)`.
///
/// Each module's memmap is keyed on its own `Module` tracked struct. Changes
/// to `b.rs` only flow through `file_item_tree(b.rs)` → `module_memmap(b)` —
/// they never touch the `a` chain.
#[test]
fn body_change_in_sibling_module_does_not_invalidate_memmap() {
    let mut db = Database::default();

    let file_a = SourceFile::new(&db, "a.rs".to_owned(), "fn a_fn() { 1 }".to_owned());
    let file_b = SourceFile::new(&db, "b.rs".to_owned(), "fn b_fn() { 1 }".to_owned());
    let lib_file = SourceFile::new(&db, "lib.rs".to_owned(), "mod a;\nmod b;\n".to_owned());
    let source_root = SourceRoot::new(&db, vec![lib_file, file_a, file_b]);

    db.attach(|db| {
        let root_module = Module::new(
            db,
            ModuleSource::Local {
                file: lib_file,
                parent: None,
                declaration: None,
            },
        );
        let module_a = Module::new(
            db,
            ModuleSource::Local {
                file: file_a,
                parent: Some(root_module),
                declaration: None,
            },
        );
        let _ = module_memmap(db, module_a, source_root, root_module);
        db.take_query_log();
    });

    // Change only module b's body
    file_b.set_text(&mut db).to("fn b_fn() { 2 }".to_owned());

    db.attach(|db| {
        let root_module = Module::new(
            db,
            ModuleSource::Local {
                file: lib_file,
                parent: None,
                declaration: None,
            },
        );
        let module_a = Module::new(
            db,
            ModuleSource::Local {
                file: file_a,
                parent: Some(root_module),
                declaration: None,
            },
        );
        let _ = module_memmap(db, module_a, source_root, root_module);
        let log = db.take_query_log();

        // Neither module_a's file_item_tree nor its memmap should re-execute.
        assert!(
            !log.contains("file_item_tree(\"a.rs\")"),
            "file_item_tree(a.rs) should not re-run, got log:\n{log}"
        );
        assert!(
            !log.contains("module_memmap"),
            "module_memmap(a) should not re-execute when b changes, got log:\n{log}"
        );
    });
}

// ===========================================================================
// Phase 5: External modules should never reach module_memmap.
// ===========================================================================

/// Calling `module_memmap` on an external module panics (debug assertion).
///
/// External module contents are queried via `TcxDb` directly — they have no
/// source file to parse.
#[test]
#[should_panic(expected = "module_memmap should not be called on external modules")]
fn external_module_memmap_panics() {
    use sage_ir::module::{CrateNum, DefIndex};

    let db = Database::default();
    db.attach(|db| {
        let file = SourceFile::new(db, "lib.rs".to_owned(), "".to_owned());
        let source_root = SourceRoot::new(db, vec![file]);
        let root_module = Module::new(
            db,
            ModuleSource::Local {
                file,
                parent: None,
                declaration: None,
            },
        );
        let ext_module = Module::new(db, ModuleSource::External(CrateNum(1), DefIndex(0)));

        // This should panic with the debug_assert message.
        let _ = module_memmap(db, ext_module, source_root, root_module);
    });
}
