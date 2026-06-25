//! Core entry point: `run_sage_with` sets up the full sage pipeline
//! and hands a live `SageContext` to a callback.
//!
//! Architecture: rustc runs on a spawned thread (providing `TyCtxt`).
//! Salsa work runs on the caller's thread. The two communicate via channels.
//! No unsafe code — the channel boundary copies all data into owned values.

use std::path::Path;
use std::sync::mpsc;

use rustc_driver::{Callbacks, Compilation};
use rustc_interface::interface;
use rustc_middle::ty::TyCtxt;

use sage_ir::db::Database;
use sage_ir::local_syms::mods::{LocalModSym, ModBodySource};
use sage_ir::name::Name;
use sage_ir::parse::parse_str_to_cst;
use sage_ir::scope::{LocalCrateSymbol, ScopeSymbol, local_crate};
use sage_ir::source::SourceFile;
use sage_ir::span::{AbsoluteSpan, ParseSource};
use sage_ir::symbol::ModSymbol;
use sage_ir::tcx::TcxRequest;
use sage_stash::{Stash, Stashed};
use salsa::Database as _;

use crate::metadata::{self, WorkspaceInfo};
use crate::tcx_impl::RustcTcxDb;

/// Everything needed to query sage inside the callback.
pub struct SageContext<'db> {
    pub db: &'db Database,
    pub krate: LocalCrateSymbol<'db>,
    pub root: ModSymbol<'db>,
}

impl<'db> SageContext<'db> {}

/// Set up the full sage pipeline for a project and call `f` with a live
/// `SageContext`. Handles: load_workspace, build rustc args, run_compiler,
/// create Database + root ModSymbol.
///
/// Rustc runs on a spawned thread (serving TyCtxt queries). Salsa work
/// runs on the caller's thread. No unsafe code.
pub fn run_sage_with<F, R>(project_dir: &Path, selected_packages: &[String], f: F) -> R
where
    F: FnOnce(&SageContext<'_>) -> R + Send,
    R: Send,
{
    let ws = metadata::load_workspace(project_dir, selected_packages);

    eprintln!(
        "sage: {} workspace crate(s) selected, {} direct deps",
        ws.selected.len(),
        ws.direct_dep_rlibs.len(),
    );

    let src_dir = ws
        .selected
        .first()
        .map(|k| k.manifest_dir.join("src"))
        .expect("no workspace crates");

    let source_files = collect_source_files(&src_dir);
    let args = build_rustc_args(&ws);

    // Channel: main thread (salsa) → rustc thread (TyCtxt).
    // Each request carries its own oneshot reply sender.
    let (req_tx, req_rx) = mpsc::channel::<TcxRequest>();

    std::thread::scope(|s| {
        // Spawn rustc on a background thread — it serves TcxDb requests.
        s.spawn(|| {
            let mut driver = Driver {
                req_rx: Some(req_rx),
            };
            let _ = rustc_driver::catch_fatal_errors(|| {
                rustc_driver::run_compiler(&args, &mut driver);
            });

            struct Driver {
                req_rx: Option<mpsc::Receiver<TcxRequest>>,
            }

            impl Callbacks for Driver {
                fn after_expansion<'tcx>(
                    &mut self,
                    _compiler: &interface::Compiler,
                    tcx: TyCtxt<'tcx>,
                ) -> Compilation {
                    let tcx_db = RustcTcxDb::new(tcx);

                    // Serve TcxDb requests until the main thread drops its sender.
                    for req in self.req_rx.take().unwrap() {
                        match req {
                            TcxRequest::ExternCrate { name, reply } => {
                                let _ = reply.send(tcx_db.extern_crate(&name));
                            }
                            TcxRequest::ModuleChildren {
                                crate_num,
                                def_index,
                                reply,
                            } => {
                                let _ = reply.send(tcx_db.module_children(crate_num, def_index));
                            }
                            TcxRequest::IsBuiltinDerive {
                                crate_num,
                                def_index,
                                reply,
                            } => {
                                let _ = reply.send(tcx_db.is_builtin_derive(crate_num, def_index));
                            }
                            TcxRequest::ItemName {
                                crate_num,
                                def_index,
                                reply,
                            } => {
                                let _ = reply.send(tcx_db.item_name(crate_num, def_index));
                            }
                            TcxRequest::IsModule {
                                crate_num,
                                def_index,
                                reply,
                            } => {
                                let _ = reply.send(tcx_db.is_module(crate_num, def_index));
                            }
                            TcxRequest::DefPath {
                                crate_num,
                                def_index,
                                reply,
                            } => {
                                let _ = reply.send(tcx_db.def_path(crate_num, def_index));
                            }
                            TcxRequest::StructuredDefPath {
                                crate_num,
                                def_index,
                                reply,
                            } => {
                                let _ =
                                    reply.send(tcx_db.structured_def_path(crate_num, def_index));
                            }
                            TcxRequest::ExpandDerive {
                                crate_num,
                                def_index,
                                item_source,
                                reply,
                            } => {
                                let _ = reply.send(tcx_db.expand_proc_macro_derive(
                                    crate_num,
                                    def_index,
                                    &item_source,
                                ));
                            }
                            TcxRequest::ExpandBang {
                                crate_num,
                                def_index,
                                input_tokens,
                                reply,
                            } => {
                                let _ = reply.send(tcx_db.expand_proc_macro_bang(
                                    crate_num,
                                    def_index,
                                    &input_tokens,
                                ));
                            }
                            TcxRequest::ExpandAttr {
                                crate_num,
                                def_index,
                                attr_args,
                                item_source,
                                reply,
                            } => {
                                let _ = reply.send(tcx_db.expand_proc_macro_attr(
                                    crate_num,
                                    def_index,
                                    &attr_args,
                                    &item_source,
                                ));
                            }
                        }
                    }

                    Compilation::Stop
                }
            }
        });

        // Main thread: run salsa work.
        let db = Database::with_proxy(req_tx);
        db.attach(|db| {
            let mut files = Vec::new();
            for (rel_path, text) in &source_files {
                files.push(SourceFile::new(db, rel_path.clone(), text.clone()));
            }

            let lib_file = files
                .iter()
                .find(|f| f.path(db) == "lib.rs")
                .or_else(|| files.iter().find(|f| f.path(db) == "main.rs"))
                .copied()
                .expect("no lib.rs or main.rs found");

            let mut empty_stash = Stash::new();
            let empty_slice = empty_stash.alloc_slice::<sage_ir::cst::attrs::AttrCst>(&[]);
            let empty_attrs = Stashed::new(empty_stash, empty_slice);
            let abs_span = AbsoluteSpan {
                source: ParseSource::SourceFile(lib_file),
                start: 0,
                end: lib_file.text(db).len() as u32,
            };

            let root_mod = LocalModSym::new(
                db,
                Name::new(db, String::new()),
                None,
                ModBodySource::File(lib_file),
                empty_attrs,
                abs_span,
            );

            let krate = local_crate(db, root_mod);
            let scope = ScopeSymbol::Crate(krate);

            let source = ParseSource::SourceFile(lib_file);
            let items = parse_str_to_cst(db, source, lib_file.text(db), scope);
            sage_ir::local_syms::mods::unexpanded_items::specify(db, root_mod, items);

            let root = ModSymbol::Local(root_mod);
            let ctx = SageContext { db, krate, root };

            f(&ctx)
        })
    })
}

/// Build rustc args for the stub driver.
pub fn build_rustc_args(ws: &WorkspaceInfo) -> Vec<String> {
    let sysroot = metadata::our_sysroot();

    let stub_dir = std::env::temp_dir().join("sage-stub");
    std::fs::create_dir_all(&stub_dir).unwrap();
    let stub_path = stub_dir.join("lib.rs");
    let mut stub_src = String::from("#![crate_type = \"lib\"]\n#![allow(unused_extern_crates)]\n");
    for name in ws.direct_dep_rlibs.keys() {
        stub_src.push_str(&format!("extern crate {name};\n"));
    }
    std::fs::write(&stub_path, &stub_src).unwrap();

    let mut args: Vec<String> = vec![
        "sage".into(),
        stub_path.to_string_lossy().into_owned(),
        "--edition=2021".into(),
        "--crate-type=lib".into(),
        format!("--sysroot={sysroot}"),
        format!("-Ldependency={}", ws.deps_dir.display()),
    ];

    for (name, path) in &ws.direct_dep_rlibs {
        args.push(format!("--extern={name}={}", path.display()));
    }

    args
}

/// Collect all .rs files under a directory, returning (relative_path, contents).
fn collect_source_files(src_dir: &Path) -> Vec<(String, String)> {
    let mut files = Vec::new();
    collect_rs_files_recursive(src_dir, src_dir, &mut files);
    files.sort_by(|a, b| a.0.cmp(&b.0));
    files
}

fn collect_rs_files_recursive(base: &Path, dir: &Path, out: &mut Vec<(String, String)>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries {
        let Ok(entry) = entry else { continue };
        let path = entry.path();
        if path.is_dir() {
            collect_rs_files_recursive(base, &path, out);
        } else if path.extension().is_some_and(|e| e == "rs") {
            let rel = path.strip_prefix(base).unwrap();
            let text = std::fs::read_to_string(&path).unwrap();
            out.push((rel.display().to_string(), text));
        }
    }
}
