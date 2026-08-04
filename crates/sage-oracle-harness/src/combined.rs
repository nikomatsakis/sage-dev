use std::sync::mpsc;

use rust_ref::{Crate, NormalizedDef};
use rustc_driver::{Callbacks, Compilation};
use rustc_interface::interface;
use rustc_middle::ty::TyCtxt;
use sage_ir::db::Database;
use sage_ir::tcx::TcxRequest;

use crate::Fixture;

/// Run a Sage-side operation against the live metadata for `fixture` without
/// asking rustc to solve any Sage semantic goal.
pub fn with_proxy_database<R>(
    fixture: &Fixture,
    f: impl FnOnce(Database, Vec<(String, String)>) -> R,
) -> R {
    let entry = fixture_entry(fixture);
    let args = compiler_args(&entry);
    let (ready_tx, ready_rx) = mpsc::channel();
    let (req_tx, req_rx) = mpsc::channel::<TcxRequest>();

    std::thread::scope(|scope| {
        scope.spawn(|| {
            let mut callbacks = MetadataCallbacks {
                ready_tx: Some(ready_tx),
                req_rx: Some(req_rx),
            };
            let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                rustc_driver::run_compiler(&args, &mut callbacks);
            }));
        });

        ready_rx
            .recv()
            .expect("rustc stopped before external metadata became available");
        f(Database::with_proxy(req_tx), fixture_files(fixture))
    })
}

/// Run both oracle and sage on a fixture in a single rustc session.
///
/// The rustc compilation compiles the actual fixture file. Inside `after_analysis`:
/// 1. Emit the oracle output (full `TyCtxt` access)
/// 2. Serve `TcxRequest`s for sage until the main thread is done
///
/// The main thread receives the oracle output, then runs sage with a
/// `ProxyTcxDb` backed by the same compilation.
pub fn run_combined(
    fixture: &Fixture,
) -> (Result<Crate<NormalizedDef>, String>, Crate<NormalizedDef>) {
    let entry = fixture_entry(fixture);
    let args = compiler_args(&entry);

    // Channels:
    // - oracle_tx/oracle_rx: oneshot for the oracle output
    // - req_tx/req_rx: TcxDb request stream for sage
    let (oracle_tx, oracle_rx) = mpsc::channel::<Result<Crate<NormalizedDef>, String>>();
    let (req_tx, req_rx) = mpsc::channel::<TcxRequest>();

    std::thread::scope(|s| {
        s.spawn(|| {
            let mut callbacks = CombinedCallbacks {
                oracle_tx: Some(oracle_tx),
                req_rx: Some(req_rx),
            };
            let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                rustc_driver::run_compiler(&args, &mut callbacks);
            }));
            // If rustc panicked before after_analysis, send an error.
            if let Some(tx) = callbacks.oracle_tx.take() {
                let _ = tx.send(Err("rustc panicked before analysis".to_string()));
            }
        });

        // Main thread: receive oracle output, then run sage.
        let oracle_result = oracle_rx.recv().unwrap_or(Err(
            "rustc thread died without sending oracle output".to_string(),
        ));

        let sage_result = run_sage_side(fixture, req_tx);

        (oracle_result, sage_result)
    })
}

struct MetadataCallbacks {
    ready_tx: Option<mpsc::Sender<()>>,
    req_rx: Option<mpsc::Receiver<TcxRequest>>,
}

impl Callbacks for MetadataCallbacks {
    fn after_analysis(&mut self, _compiler: &interface::Compiler, tcx: TyCtxt<'_>) -> Compilation {
        let _ = self.ready_tx.take().unwrap().send(());
        sage_rustc_bridge::serve_tcx_requests(tcx, self.req_rx.take().unwrap());
        Compilation::Stop
    }
}

fn fixture_entry(fixture: &Fixture) -> std::path::PathBuf {
    let entry = match fixture {
        Fixture::SingleFile(path) => path.clone(),
        Fixture::Directory { entry, .. } => entry.clone(),
    };
    entry
        .canonicalize()
        .unwrap_or_else(|error| panic!("cannot canonicalize {}: {error}", entry.display()))
}

fn compiler_args(entry: &std::path::Path) -> Vec<String> {
    vec![
        "sage-oracle".to_string(),
        entry.to_string_lossy().to_string(),
        "--edition".to_string(),
        "2021".to_string(),
        "--sysroot".to_string(),
        sage_rustc_bridge::find_sysroot(),
        "--crate-type".to_string(),
        "lib".to_string(),
    ]
}

fn fixture_files(fixture: &Fixture) -> Vec<(String, String)> {
    match fixture {
        Fixture::SingleFile(path) => {
            vec![("lib.rs".to_string(), std::fs::read_to_string(path).unwrap())]
        }
        Fixture::Directory { entry, files } => {
            let src_dir = entry.parent().unwrap();
            files
                .iter()
                .map(|file| {
                    (
                        file.strip_prefix(src_dir)
                            .unwrap()
                            .to_string_lossy()
                            .to_string(),
                        std::fs::read_to_string(file).unwrap(),
                    )
                })
                .collect()
        }
    }
}

struct CombinedCallbacks {
    oracle_tx: Option<mpsc::Sender<Result<Crate<NormalizedDef>, String>>>,
    req_rx: Option<mpsc::Receiver<TcxRequest>>,
}

impl Callbacks for CombinedCallbacks {
    fn after_analysis(&mut self, _compiler: &interface::Compiler, tcx: TyCtxt<'_>) -> Compilation {
        // Step 1: emit oracle output
        let oracle_result = sage_oracle::emit_crate(tcx).map_err(|e| format!("{}", e));
        let _ = self.oracle_tx.take().unwrap().send(oracle_result);

        // Step 2: serve TcxDb requests until sage is done
        sage_rustc_bridge::serve_tcx_requests(tcx, self.req_rx.take().unwrap());

        Compilation::Stop
    }
}

fn run_sage_side(fixture: &Fixture, req_tx: mpsc::Sender<TcxRequest>) -> Crate<NormalizedDef> {
    let db = Database::with_proxy(req_tx);

    match fixture {
        Fixture::SingleFile(path) => {
            let source = std::fs::read_to_string(path).unwrap();
            sage_test_harness::with_test_crate_files_using_db(
                db,
                &[("lib.rs", &source)],
                |db, root| sage_emit::emit_module(db, root),
            )
        }
        Fixture::Directory { entry, files } => {
            let src_dir = entry.parent().unwrap();
            let pairs: Vec<(String, String)> = files
                .iter()
                .map(|f| {
                    let rel = f
                        .strip_prefix(src_dir)
                        .unwrap()
                        .to_string_lossy()
                        .to_string();
                    let content = std::fs::read_to_string(f).unwrap();
                    (rel, content)
                })
                .collect();
            let refs: Vec<(&str, &str)> = pairs
                .iter()
                .map(|(p, c)| (p.as_str(), c.as_str()))
                .collect();
            sage_test_harness::with_test_crate_files_using_db(db, &refs, |db, root| {
                sage_emit::emit_module(db, root)
            })
        }
    }
}
