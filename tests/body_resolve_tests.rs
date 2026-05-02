#![feature(rustc_private)]

use std::path::Path;

use sage_ir::Db;
use sage_ir::body_resolve::resolve_body;
use sage_ir::item::Item;
use sage_ir::resolve::{module_items, resolve_module_path};
use sage_ir::resolved::Res;
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

#[test]
fn resolve_body_pattern_some_resolves() {
    use sage_ir::resolved::{RExprKind, RPatKind};

    run_sage_with(mini_redis_dir(), &[], |sage| {
        let module =
            resolve_module_path(sage.db, sage.root, sage.source_root, &["cmd", "get"]).unwrap();
        let method = find_method(sage.db, module, "Get", "apply");
        let resolved = resolve_body(sage.db, method, module, sage.source_root, sage.root);

        let stash = resolved.stash();
        let body = &stash[*resolved.root()];
        let root = &stash[body.root];

        // The root is a block. Walk its stmts to find the IfLet.
        let RExprKind::Block(stmts, _) = &root.kind else {
            panic!("expected block at root");
        };
        // Find the let stmt whose init is the if-let
        let mut found_some = false;
        for stmt in &stash[*stmts] {
            if let sage_ir::resolved::RStmtKind::Let(_, _, Some(init)) = &stmt.kind {
                let init_expr = &stash[*init];
                if let RExprKind::IfLet(pat, _, _, _) = &init_expr.kind {
                    let pat_data = &stash[*pat];
                    if let RPatKind::TupleStruct(res, _) = &pat_data.kind {
                        // `Some(value)` should resolve to Res::Def (from std prelude)
                        assert!(
                            matches!(res, Res::Def(_)),
                            "Some should resolve to Res::Def, got {:?}",
                            res
                        );
                        found_some = true;
                    }
                }
            }
        }
        assert!(
            found_some,
            "did not find TupleStruct(Some) pattern in if-let"
        );
    });
}

#[test]
fn resolve_body_macro_calls() {
    use sage_ir::resolved::{RExprKind, RStmtKind};
    use sage_ir::symbol::SymbolSource;

    run_sage_with(mini_redis_dir(), &[], |sage| {
        let module =
            resolve_module_path(sage.db, sage.root, sage.source_root, &["cmd", "get"]).unwrap();
        let method = find_method(sage.db, module, "Get", "apply");
        let resolved = resolve_body(sage.db, method, module, sage.source_root, sage.root);

        let stash = resolved.stash();
        let body = &stash[*resolved.root()];
        let root = &stash[body.root];

        // Walk the block to find MacroCall nodes
        let RExprKind::Block(stmts, _) = &root.kind else {
            panic!("expected block at root");
        };

        let mut macro_res = Vec::new();
        for stmt in &stash[*stmts] {
            match &stmt.kind {
                RStmtKind::Expr(e) => {
                    if let RExprKind::MacroCall(res, _) = &stash[*e].kind {
                        macro_res.push(*res);
                    }
                }
                _ => {}
            }
        }

        // `debug!(...)` should resolve to Res::Def pointing at tracing crate
        assert!(
            !macro_res.is_empty(),
            "expected at least one macro call in Get::apply"
        );
        let debug_res = macro_res[0];
        match debug_res {
            Res::Def(sym) => match sym.source(sage.db) {
                SymbolSource::External(_, _) => {} // expected: tracing::debug
                other => panic!("expected external symbol for debug!, got {:?}", other),
            },
            other => panic!("debug! should resolve to Res::Def, got {:?}", other),
        }
    });
}
