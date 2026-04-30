use crate::tcx::{NoopTcxDb, TcxDb};

/// Salsa database for sage-ir.
///
/// The `tcx` field holds a lifetime-erased `TcxDb` trait object. When backed
/// by `RustcTcxDb<'tcx>`, the real lifetime is `'tcx` — the database must
/// not outlive the `after_expansion` callback. The `run_sage_with` entry
/// point enforces this via a closure-scoped borrow.
#[salsa::db]
#[derive(Clone)]
pub struct Database {
    storage: salsa::Storage<Self>,
    /// Lifetime-erased TcxDb. See [`Database::with_tcx`] for construction.
    tcx: std::sync::Arc<dyn TcxDb + Send + Sync>,
    query_log: std::sync::Arc<std::sync::Mutex<Vec<String>>>,
}

impl Database {
    /// Create a database with a `'static` TcxDb (e.g. `NoopTcxDb` for tests).
    pub fn new(tcx: impl TcxDb + Send + Sync + 'static) -> Self {
        let log = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let log_clone = log.clone();
        Self {
            storage: salsa::Storage::new(Some(Box::new(move |event| {
                if let salsa::EventKind::WillExecute { database_key } = event.kind {
                    log_clone.lock().unwrap().push(format!("{database_key:?}"));
                }
            }))),
            tcx: std::sync::Arc::new(tcx),
            query_log: log,
        }
    }

    /// Create a database with a non-`'static` TcxDb by erasing its lifetime.
    ///
    /// # Safety
    ///
    /// The returned `Database` must not outlive `'tcx`. The caller must ensure
    /// the database is dropped before the `TcxDb` source (e.g. `TyCtxt<'tcx>`)
    /// is invalidated. The `run_sage_with` pattern enforces this.
    pub unsafe fn with_tcx<'tcx>(tcx: Box<dyn TcxDb + Send + Sync + 'tcx>) -> Self {
        let log = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let log_clone = log.clone();
        // SAFETY: caller guarantees Database won't outlive 'tcx.
        let erased: Box<dyn TcxDb + Send + Sync + 'static> = unsafe { std::mem::transmute(tcx) };
        Self {
            storage: salsa::Storage::new(Some(Box::new(move |event| {
                if let salsa::EventKind::WillExecute { database_key } = event.kind {
                    log_clone.lock().unwrap().push(format!("{database_key:?}"));
                }
            }))),
            tcx: std::sync::Arc::from(erased),
            query_log: log,
        }
    }

    /// Drain the query log and return it as a newline-separated string.
    pub fn take_query_log(&self) -> String {
        let mut log = self.query_log.lock().unwrap();
        let out = log.join("\n");
        log.clear();
        out
    }
}

impl Default for Database {
    fn default() -> Self {
        Self::new(NoopTcxDb)
    }
}

#[salsa::db]
impl crate::Db for Database {
    fn tcx(&self) -> &dyn TcxDb {
        &*self.tcx
    }
}

#[salsa::db]
impl salsa::Database for Database {}
