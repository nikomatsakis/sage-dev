//! Milestone 1 demo test: `dump_expanded_module` end-to-end.
//!
//! Builds a small multi-file crate, calls `dump_expanded_module` with
//! a path string, and snapshot-asserts the rendered memmap.

mod common;

use common::fmt_memmap_entries;
use expect_test::{Expect, expect};
use sage_ir::db::Database;
use sage_ir::dump::dump_expanded_module;
use sage_ir::item::ModAst;
use sage_ir::module::ModSymbol;
use sage_ir::resolve::SourceRoot;
use sage_ir::source::SourceFile;
use salsa::Database as _;

fn check(files: &[(&str, &str)], path: &str, expected: Expect) {
    let db = Database::default();
    db.attach(|db| {
        let source_files: Vec<SourceFile> = files
            .iter()
            .map(|(p, t)| SourceFile::new(db, p.to_string(), t.to_string()))
            .collect();
        let source_root = SourceRoot::new(db, source_files.clone());
        let lib_file = source_files
            .iter()
            .find(|f| f.path(db) == "lib.rs")
            .copied()
            .expect("fixture must have lib.rs");
        let root = ModSymbol::ast(ModAst::crate_root(db, lib_file));

        let expanded = dump_expanded_module(db, root, source_root, path)
            .expect("path should resolve to a module");
        let rendered = fmt_memmap_entries(db, expanded.entries(db), 0);
        expected.assert_eq(&rendered);
    });
}

#[test]
fn root_path() {
    check(
        &[("lib.rs", "struct Foo; struct Bar;")],
        "",
        expect![[r#"
            Item Foo kind=Struct
            TupleStructCtor Foo
            Item Bar kind=Struct
            TupleStructCtor Bar"#]],
    );
}

#[test]
fn crate_root_path() {
    check(
        &[("lib.rs", "struct Foo;")],
        "crate",
        expect![[r#"
            Item Foo kind=Struct
            TupleStructCtor Foo"#]],
    );
}

#[test]
fn nested_inline_module() {
    check(
        &[(
            "lib.rs",
            r#"
            mod inner {
                struct InnerThing;
            }
        "#,
        )],
        "inner",
        expect![[r#"
            Item InnerThing kind=Struct
            TupleStructCtor InnerThing"#]],
    );
}

#[test]
fn nested_file_module() {
    check(
        &[
            ("lib.rs", "mod child;"),
            ("child.rs", "fn hello() {} struct ChildThing;"),
        ],
        "child",
        expect![[r#"
            Item hello kind=Function
            Item ChildThing kind=Struct
            TupleStructCtor ChildThing"#]],
    );
}

#[test]
fn deep_path() {
    check(
        &[(
            "lib.rs",
            r#"
            mod a {
                pub mod b {
                    struct Deep;
                }
            }
        "#,
        )],
        "a::b",
        expect![[r#"
            Item Deep kind=Struct
            TupleStructCtor Deep"#]],
    );
}

#[test]
fn unresolved_path_returns_none() {
    let db = Database::default();
    db.attach(|db| {
        let file = SourceFile::new(db, "lib.rs".to_owned(), "struct Foo;".to_owned());
        let source_root = SourceRoot::new(db, vec![file]);
        let root = ModSymbol::ast(ModAst::crate_root(db, file));
        assert!(dump_expanded_module(db, root, source_root, "nonexistent").is_none());
    });
}
