#![feature(rustc_private)]
//! Integration tests using `run_sage_with` with a real `TcxDb` backed by `TyCtxt`.
//!
//! These tests point at `test-fixtures/mini-redis` and exercise the full pipeline:
//! workspace loading, dep building, rustc_driver, RustcTcxDb, salsa Database,
//! module resolution, name resolution, and derive expansion.

use std::path::Path;

use expect_test::expect;
use sage_ir::Db;
use sage_ir::item::Item;
use sage_ir::resolve::{module_items, module_use_imports, resolve_module_path};

use sage::driver::run_sage_with;

fn mini_redis_dir() -> &'static Path {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("test-fixtures/mini-redis")
        .leak()
}

#[test]
fn resolve_cmd_get_with_real_tcx() {
    run_sage_with(mini_redis_dir(), &[], |sage| {
        let module = resolve_module_path(sage.db, sage.root, sage.source_root, &["cmd", "get"]);
        assert!(module.is_some(), "failed to resolve cmd::get");

        let module = module.unwrap();
        let items = module_items(sage.db, module);
        let item_names: Vec<_> = items
            .iter()
            .filter_map(|item| match item {
                Item::Struct(s) => Some(s.name(sage.db).text(sage.db).to_string()),
                Item::Function(f) => Some(f.name(sage.db).text(sage.db).to_string()),
                _ => None,
            })
            .collect();

        // cmd/get.rs has struct Get and impl Get with methods
        assert!(
            item_names.contains(&"Get".to_string()),
            "expected Get struct, got: {item_names:?}"
        );
    });
}

#[test]
fn resolve_use_imports_with_real_tcx() {
    run_sage_with(mini_redis_dir(), &[], |sage| {
        let module =
            resolve_module_path(sage.db, sage.root, sage.source_root, &["cmd", "get"]).unwrap();

        let imports = module_use_imports(sage.db, module);
        let mut out = String::new();
        for import in imports {
            out.push_str(&format!("{import}\n"));
        }

        expect![[r#"
            use crate::Connection
            use crate::Db
            use crate::Frame
            use crate::Parse
            use bytes::Bytes
            use tracing::debug
            use tracing::instrument
        "#]]
        .assert_eq(&out);
    });
}

#[test]
fn expand_derives_cmd_get() {
    run_sage_with(mini_redis_dir(), &[], |sage| {
        let module =
            resolve_module_path(sage.db, sage.root, sage.source_root, &["cmd", "get"]).unwrap();

        let items = module_items(sage.db, module);
        assert!(
            items.iter().any(
                |item| matches!(item, Item::Struct(s) if s.name(sage.db).text(sage.db) == "Get")
            ),
            "expected Get struct in cmd/get.rs"
        );

        // Verify derive resolution: resolve "Debug" in the macro namespace
        // via the std prelude. This exercises the full TcxDb pipeline.
        let debug_name = sage_ir::name::Name::new(sage.db, "Debug".to_owned());
        let result = sage_ir::resolve::resolve_name(
            sage.db,
            module,
            sage.source_root,
            sage.root,
            debug_name,
            sage_ir::resolve::Namespace::Macro,
        );

        assert!(result.is_ok(), "failed to resolve Debug in macro namespace");
        let symbol = result.unwrap();

        // Verify it's an external symbol (from std/core)
        match symbol.source(sage.db) {
            sage_ir::symbol::SymbolSource::External(cn, di) => {
                // Verify it's identified as a builtin derive
                assert!(
                    sage.db.tcx().is_builtin_derive(cn, di),
                    "Debug should be identified as a builtin derive"
                );
            }
            _ => panic!("expected external symbol for Debug derive"),
        }
    });
}

#[test]
fn query_log_demand_driven_with_real_tcx() {
    run_sage_with(mini_redis_dir(), &[], |sage| {
        // Clear any log from setup
        sage.db.take_query_log();

        // Resolve cmd::get — should only parse files on the path
        let module =
            resolve_module_path(sage.db, sage.root, sage.source_root, &["cmd", "get"]).unwrap();

        // Read the target module's items
        let _items = module_items(sage.db, module);

        let log = sage.db.take_query_log();

        // file_item_tree should fire for lib.rs, cmd/mod.rs, cmd/get.rs — exactly 3
        let file_item_tree_count = log.lines().filter(|l| l.contains("file_item_tree")).count();
        assert_eq!(
            file_item_tree_count, 3,
            "expected 3 file_item_tree calls, got {file_item_tree_count}:\n{log}"
        );

        // Should NOT contain file_item_tree for unrelated modules
        assert!(
            !log.contains("set.rs"),
            "log should not contain set.rs:\n{log}"
        );
        assert!(
            !log.contains("parse.rs"),
            "log should not contain parse.rs:\n{log}"
        );
    });
}

#[test]
fn expand_no_derives() {
    run_sage_with(mini_redis_dir(), &[], |sage| {
        // The root module (lib.rs) has no structs/enums with derives.
        // Verify that no items have derive attributes.
        let items = sage_ir::resolve::module_items(sage.db, sage.root);
        for item in items {
            let has_derive = match item {
                Item::Struct(s) => s.attrs(sage.db).iter().any(|a| {
                    a.path(sage.db)
                        .segments(sage.db)
                        .first()
                        .is_some_and(|s| s.text(sage.db) == "derive")
                }),
                Item::Enum(e) => e.attrs(sage.db).iter().any(|a| {
                    a.path(sage.db)
                        .segments(sage.db)
                        .first()
                        .is_some_and(|s| s.text(sage.db) == "derive")
                }),
                _ => false,
            };
            assert!(!has_derive, "expected no derive attributes on lib.rs items");
        }
    });
}
