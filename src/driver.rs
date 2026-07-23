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
use sage_ir::scope::{LocalCrateSymbol, ScopeSymbol, local_crate_with_edition};
use sage_ir::source::SourceFile;
use sage_ir::span::{AbsoluteSpan, ParseSource};
use sage_ir::symbol::ModSymbol;
use sage_ir::tcx::TcxRequest;
use sage_stash::{Stash, Stashed};
use salsa::Database as _;

use crate::metadata::{self, WorkspaceInfo};

/// Everything needed to query sage inside the callback.
pub struct SageContext<'db> {
    pub db: &'db Database,
    pub krate: LocalCrateSymbol<'db>,
    pub root: ModSymbol<'db>,
    pub target: metadata::SelectedTarget,
    pub direct_dependencies: Vec<String>,
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

    let target = ws
        .selected
        .first()
        .map(|krate| krate.target.clone())
        .expect("no workspace crates");
    let src_dir = target
        .src_path
        .parent()
        .expect("crate target must have a source directory");

    let source_files = collect_source_files(&src_dir);
    let args = build_rustc_args(&ws);
    let mut direct_dependencies: Vec<_> = ws.direct_dep_rlibs.keys().cloned().collect();
    direct_dependencies.sort();

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
                    sage_rustc_bridge::serve_tcx_requests(tcx, self.req_rx.take().unwrap());
                    Compilation::Stop
                }
            }
        });

        // Main thread: run salsa work.
        let mut db = Database::with_proxy(req_tx);
        let root_file_name = target
            .src_path
            .file_name()
            .expect("crate target must have a root source file")
            .to_string_lossy();
        let mut root_file = None;
        for (rel_path, text) in &source_files {
            let file = db.add_source_file(rel_path.clone(), text.clone());
            if rel_path == root_file_name.as_ref() {
                root_file = Some(file);
            }
        }
        let root_file = root_file.expect("selected target root source was not loaded");
        db.attach(|db| {
            let (krate, root) = setup_root_module(db, root_file, target.edition);
            let ctx = SageContext {
                db,
                krate,
                root,
                target,
                direct_dependencies,
            };

            f(&ctx)
        })
    })
}

#[salsa::tracked]
fn setup_root_module<'db>(
    db: &'db dyn sage_ir::Db,
    root_file: SourceFile,
    edition: sage_ir::scope::Edition,
) -> (LocalCrateSymbol<'db>, ModSymbol<'db>) {
    let mut empty_stash = Stash::new();
    let empty_slice = empty_stash.alloc_slice::<sage_ir::cst::attrs::AttrCst>(&[]);
    let empty_attrs = Stashed::new(empty_stash, empty_slice);
    let source = ParseSource::SourceFile(root_file);
    let abs_span = AbsoluteSpan {
        source,
        start: 0,
        end: root_file.text(db).len() as u32,
    };
    let root_mod = LocalModSym::new(
        db,
        Name::new(db, String::new()),
        edition,
        None,
        ModBodySource::File(root_file),
        empty_attrs,
        abs_span,
    );
    let krate = local_crate_with_edition(db, root_mod, edition);
    let items = parse_str_to_cst(db, source, root_file.text(db), ScopeSymbol::Crate(krate));
    sage_ir::local_syms::mods::unexpanded_items::specify(db, root_mod, items);
    (krate, ModSymbol::Local(root_mod))
}

/// Build rustc args for the stub driver.
pub fn build_rustc_args(ws: &WorkspaceInfo) -> Vec<String> {
    let sysroot = metadata::our_sysroot();

    let target = &ws.selected.first().expect("no workspace crates").target;
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
        format!("--edition={}", target.edition.as_str()),
        format!("--crate-name={}", target.name.replace('-', "_")),
        "--crate-type=lib".into(),
        format!("--sysroot={sysroot}"),
        format!("-Ldependency={}", ws.deps_dir.display()),
    ];

    for (name, path) in &ws.direct_dep_rlibs {
        args.push(format!("--extern={name}={}", path.display()));
    }
    for feature in &target.enabled_features {
        args.push("--cfg".into());
        args.push(format!("feature={feature:?}"));
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
