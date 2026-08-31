//! Core entry point: `run_sage_with` sets up the full sage pipeline
//! and hands a live `SageContext` to a callback.
//!
//! Architecture: rustc runs on a spawned thread (providing `TyCtxt`).
//! Salsa work runs on the caller's thread. The two communicate via channels.
//! No unsafe code — the channel boundary copies all data into owned values.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc;

use rustc_driver::{Callbacks, Compilation};
use rustc_interface::interface;
use rustc_middle::ty::TyCtxt;

use sage_ir::Db as _;
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

/// A persistent Sage workspace whose database can serve many coherent reads
/// and source updates while the rustc metadata provider remains alive.
pub struct SageHost {
    db: Option<Database>,
    root_file: SourceFile,
    workspace_root: PathBuf,
    package_root: PathBuf,
    source_root: PathBuf,
    package_name: String,
    target: metadata::SelectedTarget,
    direct_dependencies: Vec<String>,
    project_dir: PathBuf,
    selected_packages: Vec<String>,
    metadata_thread: Option<std::thread::JoinHandle<()>>,
    _metadata_stub: MetadataStub,
}

impl SageHost {
    pub fn open(project_dir: &Path, selected_packages: &[String]) -> Self {
        Self::try_open(project_dir, selected_packages)
            .unwrap_or_else(|error| panic!("failed to open Sage workspace: {error}"))
    }

    pub fn try_open(project_dir: &Path, selected_packages: &[String]) -> Result<Self, String> {
        Self::try_open_with_args(project_dir, selected_packages, |_| {})
    }

    fn try_open_with_args(
        project_dir: &Path,
        selected_packages: &[String],
        configure_args: impl FnOnce(&mut Vec<String>),
    ) -> Result<Self, String> {
        let ws = metadata::load_workspace(project_dir, selected_packages)?;
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            Self::open_loaded(project_dir, selected_packages, ws, configure_args)
        }))
        .map_err(panic_message)?
    }

    fn open_loaded(
        project_dir: &Path,
        selected_packages: &[String],
        ws: WorkspaceInfo,
        configure_args: impl FnOnce(&mut Vec<String>),
    ) -> Result<Self, String> {
        eprintln!(
            "sage: {} workspace crate(s) selected, {} direct deps",
            ws.selected.len(),
            ws.direct_dep_rlibs.len(),
        );

        let selected = ws.selected.first().cloned().expect("no workspace crates");
        let package_name = selected.name.clone();
        let target = selected.target;
        let src_dir = target
            .src_path
            .parent()
            .expect("crate target must have a source directory")
            .to_path_buf();

        let source_files = collect_source_files(&src_dir);
        let (mut args, metadata_stub) = build_rustc_args(&ws)?;
        configure_args(&mut args);
        let mut direct_dependencies: Vec<_> = ws.direct_dep_rlibs.keys().cloned().collect();
        direct_dependencies.sort();

        let (req_tx, req_rx) = mpsc::channel::<TcxRequest>();
        let (ready_tx, ready_rx) = mpsc::channel::<Result<(), String>>();
        let metadata_thread = std::thread::Builder::new()
            .name("sage-rustc-metadata".to_owned())
            .spawn(move || {
                let mut driver = Driver {
                    req_rx: Some(req_rx),
                    ready_tx: Some(ready_tx),
                };
                let _ = rustc_driver::catch_fatal_errors(|| {
                    rustc_driver::run_compiler(&args, &mut driver);
                });
                if let Some(ready_tx) = driver.ready_tx.take() {
                    let _ = ready_tx.send(Err(
                        "rustc metadata provider exited before expansion metadata became available"
                            .to_owned(),
                    ));
                }
            })
            .map_err(|error| format!("failed to start the rustc metadata provider: {error}"))?;
        match ready_rx.recv() {
            Ok(Ok(())) => {}
            Ok(Err(error)) => {
                let _ = metadata_thread.join();
                return Err(error);
            }
            Err(error) => {
                let _ = metadata_thread.join();
                return Err(format!(
                    "rustc metadata provider stopped before reporting readiness: {error}"
                ));
            }
        }

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

        Ok(Self {
            db: Some(db),
            root_file: root_file.expect("selected target root source was not loaded"),
            workspace_root: ws.workspace_root,
            package_root: selected.manifest_dir,
            source_root: src_dir,
            package_name,
            target,
            direct_dependencies,
            project_dir: project_dir.to_owned(),
            selected_packages: selected_packages.to_vec(),
            metadata_thread: Some(metadata_thread),
            _metadata_stub: metadata_stub,
        })
    }

    pub fn reload(&mut self) -> Result<(), String> {
        self.reload_with_args(|_| {})
    }

    fn reload_with_args(
        &mut self,
        configure_args: impl FnOnce(&mut Vec<String>),
    ) -> Result<(), String> {
        let replacement =
            Self::try_open_with_args(&self.project_dir, &self.selected_packages, configure_args)?;
        *self = replacement;
        Ok(())
    }

    fn db(&self) -> &Database {
        self.db.as_ref().expect("SageHost database was dropped")
    }

    fn db_mut(&mut self) -> &mut Database {
        self.db.as_mut().expect("SageHost database was dropped")
    }

    pub fn with_context<R>(&self, f: impl FnOnce(&SageContext<'_>) -> R) -> R {
        self.db().attach(|db| {
            let (krate, root) = setup_root_module(db, self.root_file, self.target.edition);
            f(&SageContext {
                db,
                krate,
                root,
                target: self.target.clone(),
                direct_dependencies: self.direct_dependencies.clone(),
            })
        })
    }

    pub fn source_root(&self) -> &Path {
        &self.source_root
    }

    pub fn workspace_root(&self) -> &Path {
        &self.workspace_root
    }

    pub fn package_root(&self) -> &Path {
        &self.package_root
    }

    pub fn target(&self) -> &metadata::SelectedTarget {
        &self.target
    }

    pub fn package_name(&self) -> &str {
        &self.package_name
    }

    pub fn source_paths(&self) -> Vec<String> {
        let mut paths: Vec<_> = self
            .db()
            .source_files()
            .map(|(path, _)| path.to_owned())
            .collect();
        paths.sort();
        paths
    }

    pub fn source_text(&self, path: &str) -> Option<String> {
        let file = self
            .db()
            .source_files()
            .find_map(|(candidate, file)| (candidate == path).then_some(file))?;
        Some(self.db().attach(|db| file.text(db).clone()))
    }

    pub fn set_source_text(&mut self, path: &str, text: String) -> Result<String, String> {
        self.db_mut().set_source_text(path, text)
    }

    pub fn take_query_log(&self) -> String {
        self.db().take_query_log()
    }

    pub fn take_inspection_log(&self) -> Vec<sage_ir::db::InspectionEvent> {
        self.db().take_inspection_log()
    }

    pub fn salsa_revision(&self) -> u64 {
        self.db().salsa_revision()
    }

    pub fn log_inspection_phase(&self, phase: &'static str, entering: bool) {
        self.db().log_inspection_phase(phase, entering);
    }
}

fn panic_message(payload: Box<dyn std::any::Any + Send>) -> String {
    if let Some(message) = payload.downcast_ref::<String>() {
        message.clone()
    } else if let Some(message) = payload.downcast_ref::<&'static str>() {
        (*message).to_owned()
    } else {
        "workspace reconstruction panicked".to_owned()
    }
}

impl Drop for SageHost {
    fn drop(&mut self) {
        drop(self.db.take());
        if let Some(thread) = self.metadata_thread.take() {
            let _ = thread.join();
        }
    }
}

struct Driver {
    req_rx: Option<mpsc::Receiver<TcxRequest>>,
    ready_tx: Option<mpsc::Sender<Result<(), String>>>,
}

impl Callbacks for Driver {
    fn after_expansion<'tcx>(
        &mut self,
        _compiler: &interface::Compiler,
        tcx: TyCtxt<'tcx>,
    ) -> Compilation {
        if tcx.dcx().has_errors().is_some() {
            if let Some(ready_tx) = self.ready_tx.take() {
                let _ = ready_tx.send(Err(
                    "rustc metadata provider could not expand its metadata stub".to_owned(),
                ));
            }
            return Compilation::Stop;
        }
        if let Some(ready_tx) = self.ready_tx.take() {
            let _ = ready_tx.send(Ok(()));
        }
        sage_rustc_bridge::serve_tcx_requests(tcx, self.req_rx.take().unwrap());
        Compilation::Stop
    }
}

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
    run_sage_host_with(project_dir, selected_packages, |host| host.with_context(f))
}

/// Set up a persistent Sage host and keep its rustc metadata thread alive for
/// the entire callback.
pub fn run_sage_host_with<F, R>(project_dir: &Path, selected_packages: &[String], f: F) -> R
where
    F: FnOnce(&mut SageHost) -> R + Send,
    R: Send,
{
    let mut host = SageHost::open(project_dir, selected_packages);
    f(&mut host)
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

#[derive(Debug)]
struct MetadataStub {
    directory: PathBuf,
    source: PathBuf,
}

impl MetadataStub {
    fn create() -> Result<Self, String> {
        static NEXT_STUB: AtomicU64 = AtomicU64::new(0);

        for _ in 0..128 {
            let sequence = NEXT_STUB.fetch_add(1, Ordering::Relaxed);
            let directory = std::env::temp_dir().join(format!(
                "sage-metadata-stub-{}-{sequence}",
                std::process::id()
            ));
            match std::fs::create_dir(&directory) {
                Ok(()) => {
                    let source = directory.join("lib.rs");
                    return Ok(Self { directory, source });
                }
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(error) => {
                    return Err(format!(
                        "failed to create rustc metadata stub directory {}: {error}",
                        directory.display()
                    ));
                }
            }
        }

        Err("failed to allocate a unique rustc metadata stub directory".to_owned())
    }
}

impl Drop for MetadataStub {
    fn drop(&mut self) {
        if let Err(error) = std::fs::remove_dir_all(&self.directory)
            && error.kind() != std::io::ErrorKind::NotFound
        {
            eprintln!(
                "sage: failed to remove metadata stub {}: {error}",
                self.directory.display()
            );
        }
    }
}

/// Build rustc arguments and a session-unique source stub for the metadata
/// provider. Raw identifiers keep Cargo extern aliases such as `type` valid.
fn build_rustc_args(ws: &WorkspaceInfo) -> Result<(Vec<String>, MetadataStub), String> {
    let sysroot = metadata::our_sysroot();

    let target = &ws.selected.first().expect("no workspace crates").target;
    let stub = MetadataStub::create()?;
    let mut stub_src = String::from("#![crate_type = \"lib\"]\n#![allow(unused_extern_crates)]\n");
    let mut extern_names: Vec<_> = ws.direct_dep_rlibs.keys().collect();
    extern_names.sort();
    for name in extern_names {
        stub_src.push_str(&format!("extern crate r#{name};\n"));
    }
    std::fs::write(&stub.source, &stub_src).map_err(|error| {
        format!(
            "failed to write rustc metadata stub {}: {error}",
            stub.source.display()
        )
    })?;

    let mut args: Vec<String> = vec![
        "sage".into(),
        stub.source.to_string_lossy().into_owned(),
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

    Ok((args, stub))
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn failed_metadata_provider_replacement_preserves_the_live_host() {
        let fixture = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("test-fixtures/semantic-inspector/db-drop-guard/source");
        let mut host = SageHost::try_open(&fixture, &[]).unwrap();
        let revision = host.salsa_revision();
        let source_paths = host.source_paths();

        let error = host
            .reload_with_args(|args| {
                args.push("--sage-intentional-invalid-option".to_owned());
            })
            .unwrap_err();

        assert!(error.contains("metadata provider"), "{error}");
        assert_eq!(host.salsa_revision(), revision);
        assert_eq!(host.source_paths(), source_paths);
        host.with_context(|context| {
            assert!(!context.root.expanded_module_items(context.db).is_empty());
        });
    }

    #[test]
    fn metadata_stub_directories_are_session_unique_and_owned() {
        let first = MetadataStub::create().unwrap();
        let second = MetadataStub::create().unwrap();
        assert_ne!(first.directory, second.directory);
        let first_directory = first.directory.clone();
        drop(first);
        assert!(!first_directory.exists());
    }
}
