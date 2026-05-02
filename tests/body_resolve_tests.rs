#![feature(rustc_private)]

use std::path::Path;

use expect_test::expect;
use sage_ir::Db;
use sage_ir::body_resolve::resolve_body;
use sage_ir::display::pretty_print_resolved;
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

fn resolve_and_print(
    sage: &sage::driver::SageContext<'_>,
    module_path: &[&str],
    type_name: &str,
    method_name: &str,
) -> String {
    let module = resolve_module_path(sage.db, sage.root, sage.source_root, module_path).unwrap();
    let method = find_method(sage.db, module, type_name, method_name);
    let resolved = resolve_body(sage.db, method, module, sage.source_root, sage.root);
    pretty_print_resolved(sage.db.tcx(), &resolved)
}

#[test]
fn resolve_get_parse_frames() {
    run_sage_with(mini_redis_dir(), &[], |sage| {
        let output = resolve_and_print(sage, &["cmd", "get"], "Get", "parse_frames");
        expect![[r#"
            locals:
              0: parse
              1: key
            {
              let <bind:1> = <local:0>.next_string()?;
              <ext std::prelude::v1::Ok>(<def Get> { key: <local:1> })
            }
        "#]]
        .assert_eq(&output);
    });
}

#[test]
fn resolve_get_apply() {
    run_sage_with(mini_redis_dir(), &[], |sage| {
        let output = resolve_and_print(sage, &["cmd", "get"], "Get", "apply");
        expect![[r#"
            locals:
              0: self
              1: db
              2: dst
              3: value
              4: response
            {
              let <bind:4> = if let <ext std::prelude::v1::Some>(<bind:3>) = <local:1>.get(&<local:0>.key) {
                <unresolved>(<local:3>)
              } else {
                <unresolved>
              };
              <ext tracing::debug>!(?response);
              <local:2>.write_frame(&<local:4>).await?;
              <ext std::prelude::v1::Ok>(())
            }
        "#]]
        .assert_eq(&output);
    });
}

#[test]
fn resolve_get_key() {
    run_sage_with(mini_redis_dir(), &[], |sage| {
        let output = resolve_and_print(sage, &["cmd", "get"], "Get", "key");
        expect![[r#"
            locals:
              0: self
            {
              &<local:0>.key
            }
        "#]]
        .assert_eq(&output);
    });
}

#[test]
fn resolve_get_into_frame() {
    run_sage_with(mini_redis_dir(), &[], |sage| {
        let output = resolve_and_print(sage, &["cmd", "get"], "Get", "into_frame");
        expect![[r#"
            locals:
              0: self
              1: frame
            {
              let <bind:1> = <unresolved>();
              <local:1>.push_bulk(<unresolved>(String.as_bytes()));
              <local:1>.push_bulk(<unresolved>(<local:0>.key.into_bytes()));
              <local:1>
            }
        "#]]
        .assert_eq(&output);
    });
}

#[test]
fn resolve_connection_read_frame() {
    run_sage_with(mini_redis_dir(), &[], |sage| {
        let output = resolve_and_print(sage, &["connection"], "Connection", "read_frame");
        expect![[r#"
            locals:
              0: self
              1: frame
            {
              loop {
                if let <ext std::prelude::v1::Some>(<bind:1>) = <local:0>.parse_frame()? {
                  return <ext std::prelude::v1::Ok>(<ext std::prelude::v1::Some>(<local:1>));
                };
                if Int Eq <local:0>.stream.read_buf(&mut <local:0>.buffer).await? {
                  if <local:0>.buffer.is_empty() {
                    return <ext std::prelude::v1::Ok>(<ext std::prelude::v1::None>);
                  } else {
                    return <ext std::prelude::v1::Err>(String.into());
                  };
                };
              };
            }
        "#]]
        .assert_eq(&output);
    });
}

#[test]
fn query_log_body_resolve_demand_driven() {
    run_sage_with(mini_redis_dir(), &[], |sage| {
        sage.db.take_query_log();

        let module =
            resolve_module_path(sage.db, sage.root, sage.source_root, &["cmd", "get"]).unwrap();
        let method = find_method(sage.db, module, "Get", "apply");
        let _resolved = resolve_body(sage.db, method, module, sage.source_root, sage.root);

        let log = sage.db.take_query_log();
        assert!(
            !log.contains("cmd/set.rs"),
            "demand-driven violation: resolved cmd/get but touched cmd/set.rs:\n{log}"
        );
        assert!(
            log.contains("cmd/get.rs"),
            "expected cmd/get.rs in query log:\n{log}"
        );
    });
}
