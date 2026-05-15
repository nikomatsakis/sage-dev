#![feature(rustc_private)]
//! Integration tests using `run_sage_with` with a real `TcxDb` backed by `TyCtxt`.
//!
//! These tests point at `test-fixtures/mini-redis` and exercise the full pipeline:
//! workspace loading, dep building, rustc_driver, RustcTcxDb, salsa Database,
//! module resolution, name resolution, and derive expansion.

use std::path::Path;

use expect_test::expect;
use sage_ir::Db;
use sage_ir::item::ItemAst;
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
                ItemAst::Struct(s) => Some(s.name(sage.db).text(sage.db).to_string()),
                ItemAst::Function(f) => Some(f.name(sage.db).text(sage.db).to_string()),
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
                |item| matches!(item, ItemAst::Struct(s) if s.name(sage.db).text(sage.db) == "Get")
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
            debug_name,
            sage_ir::resolve::Namespace::Macro(sage_ir::resolve::MacroKind::Derive),
        );

        assert!(result.is_ok(), "failed to resolve Debug in macro namespace");
        let symbol = result.unwrap();

        // Verify it's an external symbol (from std/core)
        match symbol.data() {
            sage_ir::symbol::SymbolData::Ext(ext) => {
                // Verify it's identified as a builtin derive
                assert!(
                    sage.db
                        .tcx()
                        .is_builtin_derive(ext.crate_num, ext.def_index),
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
              salsa: expanded_module(Id(1000))
              salsa: parse_source_file(Id(10))
            parse_source_file("lib.rs")
              salsa: resolve_mod_tracked(Id(3800))
              salsa: expanded_module(Id(1001))
              salsa: parse_source_file(Id(7))
            parse_source_file("cmd/mod.rs")
              salsa: resolve_mod_tracked(Id(3801))
            module_items("cmd/get.rs")
              salsa: parse_source_file(Id(6))
            parse_source_file("cmd/get.rs")"#]]
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
                ItemAst::Struct(s) => s.attrs(sage.db).iter().any(|a| {
                    a.path(sage.db)
                        .segments(sage.db)
                        .first()
                        .is_some_and(|s| s.text(sage.db) == "derive")
                }),
                ItemAst::Enum(e) => e.attrs(sage.db).iter().any(|a| {
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
            .find(
                |item| matches!(item, ItemAst::Struct(s) if s.name(sage.db).text(sage.db) == "Get"),
            )
            .expect("Get struct not found in cmd/get.rs");

        let results =
            sage_ir::derive::expand_derives(sage.db, module, sage.source_root, *get_struct);

        let mut out = String::new();
        for result in &results {
            match result {
                sage_ir::derive::DeriveResult::Builtin { impls } => {
                    for impl_item in impls {
                        out.push_str(&format!("{impl_item}\n"));
                    }
                }
                sage_ir::derive::DeriveResult::ProcMacro { symbol } => match symbol.data() {
                    sage_ir::symbol::SymbolData::Ext(ext) => {
                        out.push_str(&format!(
                            "proc_macro: External({}, {})\n",
                            ext.crate_num.0, ext.def_index.0
                        ));
                    }
                    sage_ir::symbol::SymbolData::Ast(_) => {
                        out.push_str("proc_macro: Local\n");
                    }
                },
                sage_ir::derive::DeriveResult::Expanded { items } => {
                    for item in items {
                        out.push_str(&format!("expanded: {item}\n"));
                    }
                }
            }
        }

        expect![[r#"
            impl ::std::fmt::Debug for Get {
              fn fmt(self: &Self, f: ::std::fmt::Formatter) -> ::std::fmt::Result {missing}
            }
        "#]]
        .assert_eq(&out);

        // Verify demand-driven: only queries needed for this module + derive resolution
        let log = sage.db.take_query_log();
        expect![[r#"
              salsa: expanded_module(Id(1000))
              salsa: parse_source_file(Id(10))
            parse_source_file("lib.rs")
              salsa: resolve_mod_tracked(Id(3800))
              salsa: expanded_module(Id(1001))
              salsa: parse_source_file(Id(7))
            parse_source_file("cmd/mod.rs")
              salsa: resolve_mod_tracked(Id(3801))
            module_items("cmd/get.rs")
              salsa: parse_source_file(Id(6))
            parse_source_file("cmd/get.rs")
              salsa: expanded_module(Id(1002))
            tcx::extern_crate("Debug")
            tcx::extern_crate("std")
            definition(extern(1, 0), "prelude")
            tcx::module_children(1, 0)
            tcx::is_module(1, 3)
            definition(extern(1, 3), "v1")
            tcx::module_children(1, 3)
            tcx::is_module(1, 4)
            tcx::module_children(1, 4)
            tcx::is_builtin_derive(2, 12719)
              salsa: expand_builtin(Id(5800))
            expand_builtin("Debug", "Get")"#]]
        .assert_eq(&log);
    });
}

#[test]
fn expand_proc_macro_clap_parser() {
    run_sage_with(mini_redis_dir(), &[], |sage| {
        // Resolve clap crate
        let clap_cn = sage.db.tcx().extern_crate("clap").expect("clap not found");

        // Find the Parser derive's DefIndex by walking clap's children
        let clap_root_children = sage
            .db
            .tcx()
            .module_children(clap_cn, sage_ir::module::DefIndex(0));
        let parser_child = clap_root_children
            .iter()
            .find(|c| {
                c.name == "Parser"
                    && matches!(
                        c.namespace,
                        sage_ir::resolve::Namespace::Macro(sage_ir::resolve::MacroKind::Derive)
                    )
            })
            .expect("Parser derive not found in clap");

        let source = r#"#[command(name = "test")]
struct Cli {
    #[arg(long)]
    port: Option<u16>,
}"#;

        let expanded = sage.db.tcx().expand_proc_macro_derive(
            parser_child.crate_num,
            parser_child.def_index,
            source,
        );
        assert!(expanded.is_some(), "Parser expansion should succeed");
        let text = expanded.unwrap();
        assert!(text.contains("impl"), "expanded output should contain impl");
    });
}

#[test]
fn expanded_output_is_parseable() {
    run_sage_with(mini_redis_dir(), &[], |sage| {
        let clap_cn = sage.db.tcx().extern_crate("clap").unwrap();
        let children = sage
            .db
            .tcx()
            .module_children(clap_cn, sage_ir::module::DefIndex(0));
        let parser = children
            .iter()
            .find(|c| {
                c.name == "Parser"
                    && matches!(
                        c.namespace,
                        sage_ir::resolve::Namespace::Macro(sage_ir::resolve::MacroKind::Derive)
                    )
            })
            .unwrap();

        let source = "struct Simple { x: i32 }";
        let expanded = sage
            .db
            .tcx()
            .expand_proc_macro_derive(parser.crate_num, parser.def_index, source)
            .unwrap();

        // The expanded text should parse as valid Rust via tree-sitter
        let mut parser_ts = tree_sitter::Parser::new();
        parser_ts
            .set_language(&tree_sitter_rust::LANGUAGE.into())
            .unwrap();
        let tree = parser_ts.parse(&expanded, None).unwrap();
        assert!(
            !tree.root_node().has_error(),
            "expanded output has parse errors:\n{expanded}"
        );
    });
}

#[test]
fn expand_derives_clap_parser() {
    run_sage_with(mini_redis_dir(), &[], |sage| {
        // bin/server.rs is not part of the module tree (it's a binary target),
        // so we find it directly in the source root.
        let server_file = sage
            .source_root
            .files(sage.db)
            .iter()
            .find(|f| f.path(sage.db) == "bin/server.rs")
            .copied()
            .expect("bin/server.rs not found");
        let module = sage_ir::module::ModSymbol::ast(sage_ir::item::ModAst::crate_root(
            sage.db,
            server_file,
        ));
        let items = module_items(sage.db, module);
        let cli_struct = items
            .iter()
            .find(|i| matches!(i, ItemAst::Struct(s) if s.name(sage.db).text(sage.db) == "Cli"))
            .expect("Cli struct not found");

        let results =
            sage_ir::derive::expand_derives(sage.db, module, sage.source_root, *cli_struct);

        // Should have Parser (expanded) + Debug (builtin)
        let has_builtin = results
            .iter()
            .any(|r| matches!(r, sage_ir::derive::DeriveResult::Builtin { .. }));
        let has_expanded = results
            .iter()
            .any(|r| matches!(r, sage_ir::derive::DeriveResult::Expanded { .. }));
        assert!(has_builtin, "should have builtin Debug derive");
        assert!(has_expanded, "should have expanded Parser derive");

        // No more ProcMacro stubs
        let has_stub = results
            .iter()
            .any(|r| matches!(r, sage_ir::derive::DeriveResult::ProcMacro { .. }));
        assert!(!has_stub, "should not have unexpanded ProcMacro stubs");
    });
}

#[test]
fn expanded_items_are_valid_ir() {
    run_sage_with(mini_redis_dir(), &[], |sage| {
        let server_file = sage
            .source_root
            .files(sage.db)
            .iter()
            .find(|f| f.path(sage.db) == "bin/server.rs")
            .copied()
            .expect("bin/server.rs not found");
        let module = sage_ir::module::ModSymbol::ast(sage_ir::item::ModAst::crate_root(
            sage.db,
            server_file,
        ));
        let items = module_items(sage.db, module);
        let cli_struct = items
            .iter()
            .find(|i| matches!(i, ItemAst::Struct(s) if s.name(sage.db).text(sage.db) == "Cli"))
            .expect("Cli struct not found");

        let results =
            sage_ir::derive::expand_derives(sage.db, module, sage.source_root, *cli_struct);

        for result in &results {
            if let sage_ir::derive::DeriveResult::Expanded { items } = result {
                assert!(!items.is_empty(), "expanded items should not be empty");
                for item in items {
                    assert!(
                        matches!(
                            item,
                            ItemAst::Impl(_) | ItemAst::Function(_) | ItemAst::Const(_)
                        ),
                        "unexpected expanded item kind: {item:?}",
                    );
                }
            }
        }

        // Snapshot the expanded IR items
        let mut out = String::new();
        for result in &results {
            if let sage_ir::derive::DeriveResult::Expanded { items } = result {
                for item in items {
                    out.push_str(&format!("{item}\n"));
                }
            }
        }
        expect![[r#"
            #[# [automatically_derived]
            #[# [allow(unused_qualifications ,)]
            impl clap::Parser for Cli {
            }
            #[# [allow(dead_code , unreachable_code , unused_variables , unused_braces , unused_qualifications ,)]
            #[# [allow(clippy :: style , clippy :: complexity , clippy :: pedantic , clippy :: restriction , clippy :: perf , clippy :: deprecated , clippy :: nursery , clippy :: cargo , clippy :: suspicious_else_formatting , clippy :: almost_swapped ,)]
            #[# [automatically_derived]
            impl clap::CommandFactory for Cli {
              fn command() -> clap::Command {
              let __clap_app = clap::Command::new(String);
              < Self as clap :: Args >::augment_args(__clap_app)
            }
              fn command_for_update() -> clap::Command {
              let __clap_app = clap::Command::new(String);
              < Self as clap :: Args >::augment_args_for_update(__clap_app)
            }
            }
            #[# [allow(dead_code , unreachable_code , unused_variables , unused_braces , unused_qualifications ,)]
            #[# [allow(clippy :: style , clippy :: complexity , clippy :: pedantic , clippy :: restriction , clippy :: perf , clippy :: deprecated , clippy :: nursery , clippy :: cargo , clippy :: suspicious_else_formatting , clippy :: almost_swapped ,)]
            #[# [automatically_derived]
            impl clap::FromArgMatches for Cli {
              fn from_arg_matches(__clap_arg_matches: &clap::ArgMatches) -> ::std::result::Result {
              Self::from_arg_matches_mut(&mut __clap_arg_matches.clone())
            }
              fn from_arg_matches_mut(__clap_arg_matches: &mut clap::ArgMatches) -> ::std::result::Result {
              let v = Cli { port: __clap_arg_matches.remove_one()(String) };
              ::std::result::Result::Ok(v)
            }
              fn update_from_arg_matches(self: &mut Self, __clap_arg_matches: &clap::ArgMatches) -> ::std::result::Result {
              self.update_from_arg_matches_mut(&mut __clap_arg_matches.clone())
            }
              fn update_from_arg_matches_mut(self: &mut Self, __clap_arg_matches: &mut clap::ArgMatches) -> ::std::result::Result {
              if __clap_arg_matches.contains_id(String) {
                let port = &mut self.port;
                Derefport = __clap_arg_matches.remove_one()(String)
              };
              ::std::result::Result::Ok(())
            }
            }
            #[# [allow(dead_code , unreachable_code , unused_variables , unused_braces , unused_qualifications ,)]
            #[# [allow(clippy :: style , clippy :: complexity , clippy :: pedantic , clippy :: restriction , clippy :: perf , clippy :: deprecated , clippy :: nursery , clippy :: cargo , clippy :: suspicious_else_formatting , clippy :: almost_swapped ,)]
            #[# [automatically_derived]
            impl clap::Args for Cli {
              fn group_id() -> Option {
              Some(clap::Id::from(String))
            }
              fn augment_args(__clap_app: clap::Command) -> clap::Command {
              {
                let __clap_app = __clap_app.group(clap::ArgGroup::new(String).multiple(Bool(true)).args({
                  let members: [clap :: Id ; 1usize] = [clap::Id::from(String)];
                  members
                }));
                let __clap_app = __clap_app.arg({
                  let arg = clap::Arg::new(String).value_name(String).value_parser(clap::value_parser!(u16)).action(clap::ArgAction::Set);
                  let arg = arg.long(String);
                  let arg = arg;
                  arg
                });
                __clap_app
              };
            }
              fn augment_args_for_update(__clap_app: clap::Command) -> clap::Command {
              {
                let __clap_app = __clap_app.group(clap::ArgGroup::new(String).multiple(Bool(true)).args({
                  let members: [clap :: Id ; 1usize] = [clap::Id::from(String)];
                  members
                }));
                let __clap_app = __clap_app.arg({
                  let arg = clap::Arg::new(String).value_name(String).value_parser(clap::value_parser!(u16)).action(clap::ArgAction::Set);
                  let arg = arg.long(String);
                  let arg = arg.required(Bool(false));
                  arg
                });
                __clap_app
              };
            }
            }
        "#]]
        .assert_eq(&out);
    });
}

#[test]
fn snapshot_expanded_clap_parser() {
    run_sage_with(mini_redis_dir(), &[], |sage| {
        let server_file = sage
            .source_root
            .files(sage.db)
            .iter()
            .find(|f| f.path(sage.db) == "bin/server.rs")
            .copied()
            .expect("bin/server.rs not found");
        let module = sage_ir::module::ModSymbol::ast(sage_ir::item::ModAst::crate_root(
            sage.db,
            server_file,
        ));
        let items = module_items(sage.db, module);
        let cli_struct = items
            .iter()
            .find(|i| matches!(i, ItemAst::Struct(s) if s.name(sage.db).text(sage.db) == "Cli"))
            .expect("Cli struct not found");

        let results =
            sage_ir::derive::expand_derives(sage.db, module, sage.source_root, *cli_struct);

        let mut out = String::new();
        for result in &results {
            match result {
                sage_ir::derive::DeriveResult::Builtin { impls } => {
                    for impl_item in impls {
                        out.push_str(&format!("builtin: {impl_item}\n"));
                    }
                }
                sage_ir::derive::DeriveResult::Expanded { items } => {
                    for item in items {
                        out.push_str(&format!("expanded: {item}\n"));
                    }
                }
                sage_ir::derive::DeriveResult::ProcMacro { symbol } => {
                    out.push_str(&format!("unexpanded: {symbol:?}\n"));
                }
            }
        }
        expect![[r#"
            expanded: #[# [automatically_derived]
            #[# [allow(unused_qualifications ,)]
            impl clap::Parser for Cli {
            }
            expanded: #[# [allow(dead_code , unreachable_code , unused_variables , unused_braces , unused_qualifications ,)]
            #[# [allow(clippy :: style , clippy :: complexity , clippy :: pedantic , clippy :: restriction , clippy :: perf , clippy :: deprecated , clippy :: nursery , clippy :: cargo , clippy :: suspicious_else_formatting , clippy :: almost_swapped ,)]
            #[# [automatically_derived]
            impl clap::CommandFactory for Cli {
              fn command() -> clap::Command {
              let __clap_app = clap::Command::new(String);
              < Self as clap :: Args >::augment_args(__clap_app)
            }
              fn command_for_update() -> clap::Command {
              let __clap_app = clap::Command::new(String);
              < Self as clap :: Args >::augment_args_for_update(__clap_app)
            }
            }
            expanded: #[# [allow(dead_code , unreachable_code , unused_variables , unused_braces , unused_qualifications ,)]
            #[# [allow(clippy :: style , clippy :: complexity , clippy :: pedantic , clippy :: restriction , clippy :: perf , clippy :: deprecated , clippy :: nursery , clippy :: cargo , clippy :: suspicious_else_formatting , clippy :: almost_swapped ,)]
            #[# [automatically_derived]
            impl clap::FromArgMatches for Cli {
              fn from_arg_matches(__clap_arg_matches: &clap::ArgMatches) -> ::std::result::Result {
              Self::from_arg_matches_mut(&mut __clap_arg_matches.clone())
            }
              fn from_arg_matches_mut(__clap_arg_matches: &mut clap::ArgMatches) -> ::std::result::Result {
              let v = Cli { port: __clap_arg_matches.remove_one()(String) };
              ::std::result::Result::Ok(v)
            }
              fn update_from_arg_matches(self: &mut Self, __clap_arg_matches: &clap::ArgMatches) -> ::std::result::Result {
              self.update_from_arg_matches_mut(&mut __clap_arg_matches.clone())
            }
              fn update_from_arg_matches_mut(self: &mut Self, __clap_arg_matches: &mut clap::ArgMatches) -> ::std::result::Result {
              if __clap_arg_matches.contains_id(String) {
                let port = &mut self.port;
                Derefport = __clap_arg_matches.remove_one()(String)
              };
              ::std::result::Result::Ok(())
            }
            }
            expanded: #[# [allow(dead_code , unreachable_code , unused_variables , unused_braces , unused_qualifications ,)]
            #[# [allow(clippy :: style , clippy :: complexity , clippy :: pedantic , clippy :: restriction , clippy :: perf , clippy :: deprecated , clippy :: nursery , clippy :: cargo , clippy :: suspicious_else_formatting , clippy :: almost_swapped ,)]
            #[# [automatically_derived]
            impl clap::Args for Cli {
              fn group_id() -> Option {
              Some(clap::Id::from(String))
            }
              fn augment_args(__clap_app: clap::Command) -> clap::Command {
              {
                let __clap_app = __clap_app.group(clap::ArgGroup::new(String).multiple(Bool(true)).args({
                  let members: [clap :: Id ; 1usize] = [clap::Id::from(String)];
                  members
                }));
                let __clap_app = __clap_app.arg({
                  let arg = clap::Arg::new(String).value_name(String).value_parser(clap::value_parser!(u16)).action(clap::ArgAction::Set);
                  let arg = arg.long(String);
                  let arg = arg;
                  arg
                });
                __clap_app
              };
            }
              fn augment_args_for_update(__clap_app: clap::Command) -> clap::Command {
              {
                let __clap_app = __clap_app.group(clap::ArgGroup::new(String).multiple(Bool(true)).args({
                  let members: [clap :: Id ; 1usize] = [clap::Id::from(String)];
                  members
                }));
                let __clap_app = __clap_app.arg({
                  let arg = clap::Arg::new(String).value_name(String).value_parser(clap::value_parser!(u16)).action(clap::ArgAction::Set);
                  let arg = arg.long(String);
                  let arg = arg.required(Bool(false));
                  arg
                });
                __clap_app
              };
            }
            }
            builtin: impl ::std::fmt::Debug for Cli {
              fn fmt(self: &Self, f: ::std::fmt::Formatter) -> ::std::fmt::Result {missing}
            }
        "#]]
        .assert_eq(&out);
    });
}
