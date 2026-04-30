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
        expect![[r#"
              salsa: definition(Id(1000))
            definition("lib.rs", "cmd")
              salsa: module_items(Id(800))
            module_items("lib.rs")
              salsa: file_item_tree(Id(10))
            file_item_tree("lib.rs")
              salsa: definition(Id(1001))
            definition("cmd/mod.rs", "get")
              salsa: module_items(Id(801))
            module_items("cmd/mod.rs")
              salsa: file_item_tree(Id(7))
            file_item_tree("cmd/mod.rs")
              salsa: module_items(Id(802))
            module_items("cmd/get.rs")
              salsa: file_item_tree(Id(6))
            file_item_tree("cmd/get.rs")"#]]
        .assert_eq(&log);
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

#[test]
fn expand_derives_cmd_get_full() {
    run_sage_with(mini_redis_dir(), &[], |sage| {
        // Clear log from setup
        sage.db.take_query_log();

        let module =
            resolve_module_path(sage.db, sage.root, sage.source_root, &["cmd", "get"]).unwrap();

        let items = module_items(sage.db, module);
        let get_struct = items
            .iter()
            .find(|item| matches!(item, Item::Struct(s) if s.name(sage.db).text(sage.db) == "Get"))
            .expect("Get struct not found in cmd/get.rs");

        let results = sage_ir::derive::expand_derives(
            sage.db,
            module,
            sage.source_root,
            sage.root,
            *get_struct,
        );

        let mut out = String::new();
        for result in &results {
            match result {
                sage_ir::derive::DeriveResult::Builtin { impls } => {
                    for impl_item in impls {
                        out.push_str(&format!("{impl_item}\n"));
                    }
                }
                sage_ir::derive::DeriveResult::ProcMacro { symbol } => {
                    match symbol.source(sage.db) {
                        sage_ir::symbol::SymbolSource::External(cn, di) => {
                            out.push_str(&format!("proc_macro: External({}, {})\n", cn.0, di.0));
                        }
                        sage_ir::symbol::SymbolSource::Local(_) => {
                            out.push_str("proc_macro: Local\n");
                        }
                    }
                }
            }
        }

        expect![[r#"
            impl ::std::fmt::Debug for Get {
              fn fmt(self: &Self, f: ::std::fmt::Formatter) -> ::std::fmt::Result
            }
        "#]]
        .assert_eq(&out);

        // Verify demand-driven: only queries needed for this module + derive resolution
        let log = sage.db.take_query_log();
        expect![[r#"
              salsa: definition(Id(1000))
            definition("lib.rs", "cmd")
              salsa: module_items(Id(800))
            module_items("lib.rs")
              salsa: file_item_tree(Id(10))
            file_item_tree("lib.rs")
              salsa: definition(Id(1001))
            definition("cmd/mod.rs", "get")
              salsa: module_items(Id(801))
            module_items("cmd/mod.rs")
              salsa: file_item_tree(Id(7))
            file_item_tree("cmd/mod.rs")
              salsa: module_items(Id(802))
            module_items("cmd/get.rs")
              salsa: file_item_tree(Id(6))
            file_item_tree("cmd/get.rs")
              salsa: module_use_imports(Id(802))
            module_use_imports("cmd/get.rs")
            tcx::extern_crate("Debug")
            tcx::extern_crate("std")
              salsa: definition(Id(1002))
            definition(extern(1, 0), "prelude")
            tcx::module_children(1, 0)
              salsa: definition(Id(1003))
            definition(extern(1, 3), "v1")
            tcx::module_children(1, 3)
            tcx::module_children(1, 4)
            tcx::is_builtin_derive(2, 12719)
              salsa: expand_builtin(Id(5c00))
            expand_builtin("Debug", "Get")"#]]
        .assert_eq(&log);
    });
}
