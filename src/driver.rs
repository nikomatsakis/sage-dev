//! Core entry point: `run_sage_with` sets up the full sage pipeline
//! and hands a live `SageContext` to a callback.

use std::path::{Path, PathBuf};

use rustc_driver::{Callbacks, Compilation};
use rustc_interface::interface;
use rustc_middle::ty::TyCtxt;

use sage_ir::db::Database;
use sage_ir::module::{Module, ModuleSource};
use sage_ir::resolve::SourceRoot;
use sage_ir::source::SourceFile;
use salsa::Database as _;

use crate::metadata::{self, WorkspaceInfo};
use crate::tcx_impl::RustcTcxDb;

/// Everything needed to query sage inside the callback.
pub struct SageContext<'db> {
    pub db: &'db Database,
    pub root: Module<'db>,
    pub source_root: SourceRoot,
}

/// Set up the full sage pipeline for a project and call `f` with a live
/// `SageContext`. Handles: load_workspace, build rustc args, run_compiler,
/// create RustcTcxDb + Database + root Module.
///
/// The callback runs inside `after_expansion` — `TyCtxt` and the Database
/// are both live for its duration.
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

    // Channel to send the result back from the callback.
    let mut result: Option<R> = None;
    let result_ptr = &mut result as *mut Option<R>;

    struct Driver<F, R> {
        f: Option<F>,
        source_files: Vec<(String, String)>,
        result_ptr: *mut Option<R>,
    }

    // SAFETY: result_ptr points to a local on the current thread's stack.
    // The callback runs synchronously inside run_compiler on the same thread.
    unsafe impl<F: Send, R: Send> Send for Driver<F, R> {}

    impl<F, R> Callbacks for Driver<F, R>
    where
        F: FnOnce(&SageContext<'_>) -> R,
    {
        fn after_expansion<'tcx>(
            &mut self,
            _compiler: &interface::Compiler,
            tcx: TyCtxt<'tcx>,
        ) -> Compilation {
            let tcx_db = RustcTcxDb::new(tcx);
            // SAFETY: Database is dropped at the end of this function,
            // before TyCtxt<'tcx> is invalidated.
            let db = unsafe { Database::with_tcx(Box::new(tcx_db)) };

            db.attach(|db| {
                // Create SourceFile inputs
                let mut files = Vec::new();
                for (rel_path, text) in &self.source_files {
                    files.push(SourceFile::new(db, rel_path.clone(), text.clone()));
                }

                let source_root = SourceRoot::new(db, files.clone());

                let lib_file = files
                    .iter()
                    .find(|f| f.path(db) == "lib.rs")
                    .or_else(|| files.iter().find(|f| f.path(db) == "main.rs"))
                    .expect("no lib.rs or main.rs found");

                let root = Module::new(
                    db,
                    ModuleSource::Local {
                        file: *lib_file,
                        parent: None,
                    },
                );

                let ctx = SageContext {
                    db,
                    root,
                    source_root,
                };

                let r = (self.f.take().unwrap())(&ctx);
                // SAFETY: result_ptr is valid for the duration of run_compiler.
                unsafe { *self.result_ptr = Some(r) };
            });

            Compilation::Stop
        }
    }

    let mut driver = Driver {
        f: Some(f),
        source_files: source_files,
        result_ptr: result_ptr,
    };

    let _ = rustc_driver::catch_fatal_errors(|| {
        rustc_driver::run_compiler(&args, &mut driver);
    });

    result.expect("after_expansion callback did not run")
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
