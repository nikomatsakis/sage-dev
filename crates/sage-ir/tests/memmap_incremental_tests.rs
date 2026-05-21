//! Phase 0: Baseline incremental tests for expanded_module.
//!
//! These tests lock in current incremental behavior using `expect_test` snapshots.
//! Any reorganization that changes which queries re-execute must update the snapshot,
//! making regressions immediately visible.

use expect_test::{Expect, expect};
use sage_ir::db::Database;
use sage_ir::item::ModAst;
use sage_ir::memmap::module_memmap;
use sage_ir::module::ModSymbol;
use sage_ir::resolve::SourceRoot;
use sage_ir::source::SourceFile;
use salsa::{Database as _, Setter};

fn check_log(actual: &str, expected: Expect) {
    let filtered: String = actual
        .lines()
        .filter(|l| {
            l.contains("parse_source_file")
                || l.contains("parse_macro_expansion")
                || l.contains("expand_macro")
                || l.contains("expanded_module")
                || l.contains("module_items")
        })
        .collect::<Vec<_>>()
        .join("\n");
    expected.assert_eq(&filtered);
}

/// Snapshot the query log for a fresh expanded_module computation.
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
        let root_module = ModSymbol::ast(ModAst::crate_root(db, file));

        let _ = module_memmap(db, root_module, source_root);
        let log = db.take_query_log();
        check_log(
            &log,
            expect![[r#"
                  salsa: expanded_module(Id(1400))
                  salsa: parse_source_file(Id(0))
                parse_source_file("lib.rs")
                  salsa: expand_macro(Id(2800))
                  salsa: parse_macro_expansion(Id(2c00))"#]],
        );
    });
}

/// Snapshot the query log when a function body changes.
/// After Phase 2: expanded_module does NOT re-execute — only parse_source_file does.
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
        let root_module = ModSymbol::ast(ModAst::crate_root(db, file));
        let _ = module_memmap(db, root_module, source_root);
        db.take_query_log(); // drain
    });

    // Mutate body only
    file.set_text(&mut db)
        .to("fn hello() { 2 }\nmacro_rules! m { () => { struct Foo; } }\nm!();\n".to_owned());

    // Re-query and snapshot
    db.attach(|db| {
        // Re-intern the module (same data → same ID)
        let root_module = ModSymbol::ast(ModAst::crate_root(db, file));
        let _ = module_memmap(db, root_module, source_root);
        let log = db.take_query_log();
        check_log(
            &log,
            expect![[r#"
                  salsa: parse_source_file(Id(0))
                parse_source_file("lib.rs")"#]],
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
        let root_module = ModSymbol::ast(ModAst::crate_root(db, lib_file));
        let module_a = ModSymbol::ast(ModAst::synthetic_child(db, "child", root_module, file_a));
        let _ = module_memmap(db, module_a, source_root);
        db.take_query_log(); // drain
    });

    // Change module b
    file_b.set_text(&mut db).to("fn bar() { 2 }".to_owned());

    // Re-query module_a and snapshot
    db.attach(|db| {
        let root_module = ModSymbol::ast(ModAst::crate_root(db, lib_file));
        let module_a = ModSymbol::ast(ModAst::synthetic_child(db, "child", root_module, file_a));
        let _ = module_memmap(db, module_a, source_root);
        let log = db.take_query_log();
        check_log(&log, expect![[r#""#]]);
    });
}

// ===========================================================================
// Phase 2 incremental tests: verify the key improvements from using
// parse_source_file as the incremental firewall.
// ===========================================================================

/// Body-only edits don't invalidate the memmap.
///
/// The item tree's `FnAst` is identified by name, so a body-only change
/// preserves the tracked-struct identity. `expanded_module` reads only the name,
/// so it stays cached.
#[test]
fn body_change_does_not_invalidate_memmap() {
    let mut db = Database::default();

    let file = SourceFile::new(&db, "lib.rs".to_owned(), "fn foo() { 1 }".to_owned());
    let source_root = SourceRoot::new(&db, vec![file]);

    db.attach(|db| {
        let root_module = ModSymbol::ast(ModAst::crate_root(db, file));
        let _ = module_memmap(db, root_module, source_root);
        db.take_query_log();
    });

    file.set_text(&mut db).to("fn foo() { 2 }".to_owned());

    db.attach(|db| {
        let root_module = ModSymbol::ast(ModAst::crate_root(db, file));
        let _ = module_memmap(db, root_module, source_root);
        let log = db.take_query_log();

        assert!(
            log.contains("parse_source_file"),
            "parse_source_file should re-execute after body change, got log:\n{log}"
        );
        assert!(
            !log.contains("expanded_module"),
            "expanded_module should NOT re-execute after body-only change, got log:\n{log}"
        );
    });
}

/// Signature changes (renaming a function) invalidate the memmap.
///
/// Renaming changes the `FnAst`'s identity (its `name` field), so the
/// old tracked struct is deleted and a new one is created. `expanded_module`
/// reads names, so it must re-execute.
#[test]
fn signature_change_invalidates_memmap() {
    let mut db = Database::default();

    let file = SourceFile::new(&db, "lib.rs".to_owned(), "fn foo() {}".to_owned());
    let source_root = SourceRoot::new(&db, vec![file]);

    db.attach(|db| {
        let root_module = ModSymbol::ast(ModAst::crate_root(db, file));
        let _ = module_memmap(db, root_module, source_root);
        db.take_query_log();
    });

    // Rename foo → bar (changes the identity of the FnAst)
    file.set_text(&mut db).to("fn bar() {}".to_owned());

    db.attach(|db| {
        let root_module = ModSymbol::ast(ModAst::crate_root(db, file));
        let _ = module_memmap(db, root_module, source_root);
        let log = db.take_query_log();

        assert!(
            log.contains("parse_source_file"),
            "parse_source_file should re-execute, got log:\n{log}"
        );
        assert!(
            log.contains("expanded_module"),
            "expanded_module should re-execute after rename, got log:\n{log}"
        );
    });
}

/// Adding a use statement invalidates the memmap.
///
/// A new `UseGroupAst` item changes the item tree's shape, so `expanded_module`
/// must re-execute to pick up the new entry.
#[test]
fn adding_use_statement_invalidates_memmap() {
    let mut db = Database::default();

    let file = SourceFile::new(&db, "lib.rs".to_owned(), "struct S;".to_owned());
    let source_root = SourceRoot::new(&db, vec![file]);

    db.attach(|db| {
        let root_module = ModSymbol::ast(ModAst::crate_root(db, file));
        let _ = module_memmap(db, root_module, source_root);
        db.take_query_log();
    });

    file.set_text(&mut db)
        .to("use foo::Bar;\nstruct S;".to_owned());

    db.attach(|db| {
        let root_module = ModSymbol::ast(ModAst::crate_root(db, file));
        let _ = module_memmap(db, root_module, source_root);
        let log = db.take_query_log();

        assert!(
            log.contains("expanded_module"),
            "expanded_module should re-execute when a use statement is added, got log:\n{log}"
        );
    });
}

/// `expanded_module` calls `parse_source_file` exactly once (no double-parse).
///
/// Before Phase 2, `seed_from_cst` parsed tree-sitter AND `parse_source_file`
/// was memoized separately — effectively two parses. After Phase 2, only
/// `parse_source_file` parses the source.
#[test]
fn module_memmap_calls_parse_source_file_only_once() {
    let db = Database::default();
    db.attach(|db| {
        let file = SourceFile::new(
            db,
            "lib.rs".to_owned(),
            "fn hello() {}\nmacro_rules! m { () => { struct Foo; } }\nm!();\n".to_owned(),
        );
        let source_root = SourceRoot::new(db, vec![file]);
        let root_module = ModSymbol::ast(ModAst::crate_root(db, file));

        let _ = module_memmap(db, root_module, source_root);
        let log = db.take_query_log();

        // Count parse_source_file invocations for lib.rs. There should be exactly one.
        let count = log
            .lines()
            .filter(|l| l.contains("parse_source_file(\"lib.rs\")"))
            .count();
        assert_eq!(
            count, 1,
            "expected exactly 1 parse_source_file call for lib.rs, got {count}. Log:\n{log}"
        );
    });
}

/// A body change in sibling module `b` doesn't invalidate `module_memmap(a)`.
///
/// Each module's memmap is keyed on its own `ModSymbol` tracked struct. Changes
/// to `b.rs` only flow through `parse_source_file(b.rs)` → `module_memmap(b)` —
/// they never touch the `a` chain.
#[test]
fn body_change_in_sibling_module_does_not_invalidate_memmap() {
    let mut db = Database::default();

    let file_a = SourceFile::new(&db, "a.rs".to_owned(), "fn a_fn() { 1 }".to_owned());
    let file_b = SourceFile::new(&db, "b.rs".to_owned(), "fn b_fn() { 1 }".to_owned());
    let lib_file = SourceFile::new(&db, "lib.rs".to_owned(), "mod a;\nmod b;\n".to_owned());
    let source_root = SourceRoot::new(&db, vec![lib_file, file_a, file_b]);

    db.attach(|db| {
        let root_module = ModSymbol::ast(ModAst::crate_root(db, lib_file));
        let module_a = ModSymbol::ast(ModAst::synthetic_child(db, "child", root_module, file_a));
        let _ = module_memmap(db, module_a, source_root);
        db.take_query_log();
    });

    // Change only module b's body
    file_b.set_text(&mut db).to("fn b_fn() { 2 }".to_owned());

    db.attach(|db| {
        let root_module = ModSymbol::ast(ModAst::crate_root(db, lib_file));
        let module_a = ModSymbol::ast(ModAst::synthetic_child(db, "child", root_module, file_a));
        let _ = module_memmap(db, module_a, source_root);
        let log = db.take_query_log();

        // Neither module_a's parse_source_file nor its memmap should re-execute.
        assert!(
            !log.contains("parse_source_file(\"a.rs\")"),
            "parse_source_file(a.rs) should not re-run, got log:\n{log}"
        );
        assert!(
            !log.contains("expanded_module"),
            "module_memmap(a) should not re-execute when b changes, got log:\n{log}"
        );
    });
}

// ===========================================================================
// Phase 5: External modules should never reach expanded_module.
// ===========================================================================

/// `module_memmap` on an external module returns an empty MEM-map.
///
/// External module contents are queried via `TcxDb` directly — there's
/// no source file or memmap to compute. The dispatch wrapper short-
/// circuits the Ext arm without invoking the underlying tracked query.
#[test]
fn external_module_memmap_is_empty() {
    use sage_ir::module::{CrateNum, DefIndex};

    let db = Database::default();
    db.attach(|db| {
        let file = SourceFile::new(db, "lib.rs".to_owned(), "".to_owned());
        let source_root = SourceRoot::new(db, vec![file]);
        let _root_module = ModSymbol::ast(ModAst::crate_root(db, file));
        let ext_module = ModSymbol::external(CrateNum(1), DefIndex(0));

        let memmap = module_memmap(db, ext_module, source_root);
        assert!(memmap.entries(db).is_empty());
    });
}
