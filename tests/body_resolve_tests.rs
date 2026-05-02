#![feature(rustc_private)]

use std::path::Path;

use sage_ir::Db;
use sage_ir::body_resolve::resolve_body;
use sage_ir::item::Item;
use sage_ir::resolve::{module_items, resolve_module_path};
use sage_ir::types::TypeRefKind;

use sage::driver::run_sage_with;

fn mini_redis_dir() -> &'static Path {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("test-fixtures/mini-redis")
        .leak()
}

fn find_method<'db>(
    db: &'db dyn Db,
    module: sage_ir::module::Module<'db>,
    type_name: &str,
    method_name: &str,
) -> sage_ir::item::FunctionItem<'db> {
    let items = module_items(db, module);
    for item in items {
        if let Item::Impl(impl_item) = item {
            if let TypeRefKind::Path(path) = impl_item.self_ty(db).kind(db) {
                if path.segments(db).last().map(|n| n.text(db).as_str()) == Some(type_name) {
                    for sub_item in impl_item.items(db) {
                        if let Item::Function(f) = sub_item {
                            if f.name(db).text(db) == method_name {
                                return *f;
                            }
                        }
                    }
                }
            }
        }
    }
    panic!("{type_name}::{method_name} not found");
}

#[test]
fn resolve_body_get_parse_frames() {
    run_sage_with(mini_redis_dir(), &[], |sage| {
        let module =
            resolve_module_path(sage.db, sage.root, sage.source_root, &["cmd", "get"]).unwrap();
        let method = find_method(sage.db, module, "Get", "parse_frames");
        let resolved = resolve_body(sage.db, method, module, sage.source_root, sage.root);

        let stash = resolved.stash();
        let body = &stash[*resolved.root()];
        let locals = &stash[body.locals];

        // Param: parse
        assert_eq!(locals[0].name.text(sage.db), "parse");
        // Let binding: key
        assert_eq!(locals[1].name.text(sage.db), "key");
    });
}

#[test]
fn resolve_body_get_apply() {
    run_sage_with(mini_redis_dir(), &[], |sage| {
        let module =
            resolve_module_path(sage.db, sage.root, sage.source_root, &["cmd", "get"]).unwrap();
        let method = find_method(sage.db, module, "Get", "apply");
        let resolved = resolve_body(sage.db, method, module, sage.source_root, sage.root);

        let stash = resolved.stash();
        let body = &stash[*resolved.root()];
        let locals = &stash[body.locals];

        // Params: self, db, dst
        assert_eq!(locals[0].name.text(sage.db), "self");
        assert_eq!(locals[1].name.text(sage.db), "db");
        assert_eq!(locals[2].name.text(sage.db), "dst");

        // if-let binding: "value" from `if let Some(value) = ...`
        assert!(locals.iter().any(|l| l.name.text(sage.db) == "value"));

        // "response" from `let response = ...`
        assert!(locals.iter().any(|l| l.name.text(sage.db) == "response"));
    });
}
