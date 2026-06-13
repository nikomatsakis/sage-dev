use std::path::Path;

use expect_test::expect;
use sage_ir::db::Database;
use sage_ir::item::ItemAst;
use sage_ir::item::ModAst;
use sage_ir::module::ModSymbol;
use sage_ir::resolve::{SourceRoot, module_items, resolve_module_path};
use sage_ir::source::SourceFile;
use salsa::Database as _;

fn collect_rs_files(dir: &Path) -> Vec<std::path::PathBuf> {
    let mut files = Vec::new();
    if !dir.is_dir() {
        return files;
    }
    for entry in std::fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            files.extend(collect_rs_files(&path));
        } else if path.extension().is_some_and(|e| e == "rs") {
            files.push(path);
        }
    }
    files.sort();
    files
}

/// Set up the mini-redis fixture: create all SourceFiles and a SourceRoot.
fn setup_mini_redis(db: &Database) -> (SourceRoot, ModSymbol<'_>) {
    let fixture_dir =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../test-fixtures/mini-redis/src");
    let paths = collect_rs_files(&fixture_dir);

    let mut source_files = Vec::new();
    for path in &paths {
        let rel = path.strip_prefix(&fixture_dir).unwrap();
        let text = std::fs::read_to_string(path).unwrap();
        let file = SourceFile::new(db, rel.display().to_string(), text);
        source_files.push(file);
    }

    let source_root = SourceRoot::new(db, source_files.clone());

    let lib_file = source_files
        .iter()
        .find(|f| f.path(db) == "lib.rs")
        .expect("mini-redis has no lib.rs");

    let root_module = ModSymbol::ast(ModAst::crate_root(db, *lib_file));

    (source_root, root_module)
}

#[test]
fn resolve_cmd_get_module() {
    let db = Database::default();
    db.attach(|db| {
        let (source_root, root_module) = setup_mini_redis(db);

        let module = resolve_module_path(db, root_module, source_root, &["cmd", "get"]);
        assert!(module.is_some(), "failed to resolve cmd::get module");

        let module = module.unwrap();
        let file = match module {
            sage_ir::module::ModSymbol::Ast(a) => {
                a.file(db).expect("expected file-backed local module")
            }
            _ => panic!("expected local module"),
        };
        assert_eq!(file.path(db), "cmd/get.rs");
    });
}

#[test]
fn resolve_cmd_get_use_imports() {
    let db = Database::default();
    db.attach(|db| {
        let (source_root, root_module) = setup_mini_redis(db);
        let module = resolve_module_path(db, root_module, source_root, &["cmd", "get"]).unwrap();

        let items = module_items(db, module);
        let mut out = String::new();
        for item in &items {
            if let ItemAst::Use(group) = item {
                out.push_str(&format!("{group}\n"));
            }
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
fn query_log_demand_driven() {
    let db = Database::default();
    db.attach(|db| {
        let (source_root, root_module) = setup_mini_redis(db);

        // Clear any log from setup
        db.take_query_log();

        // Resolve the module path — this only parses files on the path,
        // not the target module itself.
        let module = resolve_module_path(db, root_module, source_root, &["cmd", "get"]).unwrap();

        // Now read the target module's items — this triggers parse_source_file for cmd/get.rs
        let _items = module_items(db, module);

        let log = db.take_query_log();
        expect![[r#"
              salsa: expanded_module(Id(1000))
              salsa: parse_source_file(Id(10))
            parse_source_file("lib.rs")
              salsa: resolve_mod_tracked(Id(2c00))
              salsa: expanded_module(Id(1001))
              salsa: parse_source_file(Id(7))
            parse_source_file("cmd/mod.rs")
              salsa: resolve_mod_tracked(Id(2c01))
            module_items("cmd/get.rs")
              salsa: parse_source_file(Id(6))
            parse_source_file("cmd/get.rs")"#]]
        .assert_eq(&log);
    });
}

#[test]
fn resolve_clients_module() {
    let db = Database::default();
    db.attach(|db| {
        let (source_root, root_module) = setup_mini_redis(db);

        let module = resolve_module_path(db, root_module, source_root, &["clients"]);
        assert!(module.is_some(), "failed to resolve clients module");

        let module = module.unwrap();
        let file = match module {
            sage_ir::module::ModSymbol::Ast(a) => {
                a.file(db).expect("expected file-backed local module")
            }
            _ => panic!("expected local module"),
        };
        assert_eq!(file.path(db), "clients/mod.rs");
    });
}

#[test]
fn resolve_no_cross_module_parsing() {
    let db = Database::default();
    db.attach(|db| {
        let (source_root, root_module) = setup_mini_redis(db);

        // Clear log
        db.take_query_log();

        // Resolve just the "clients" module — should NOT parse cmd/ files
        // resolve_module_path only parses lib.rs (to find mod clients)
        let module = resolve_module_path(db, root_module, source_root, &["clients"]).unwrap();

        // Read the module's items to trigger parse_source_file for clients/mod.rs
        let _items = module_items(db, module);

        let log = db.take_query_log();
        expect![[r#"
              salsa: expanded_module(Id(1000))
              salsa: parse_source_file(Id(10))
            parse_source_file("lib.rs")
              salsa: resolve_mod_tracked(Id(2c00))
            module_items("clients/mod.rs")
              salsa: parse_source_file(Id(5))
            parse_source_file("clients/mod.rs")"#]]
        .assert_eq(&log);
    });
}
