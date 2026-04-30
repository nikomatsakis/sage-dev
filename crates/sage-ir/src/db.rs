use crate::tcx::{NoopTcxDb, TcxDb};

/// Salsa database for sage-ir.
#[salsa::db]
#[derive(Clone)]
pub struct Database {
    storage: salsa::Storage<Self>,
    tcx: std::sync::Arc<dyn TcxDb>,
    query_log: std::sync::Arc<std::sync::Mutex<Vec<String>>>,
}

impl Database {
    pub fn new(tcx: impl TcxDb + 'static) -> Self {
        let log = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let log_clone = log.clone();
        Self {
            storage: salsa::Storage::new(Some(Box::new(move |event| {
                if let salsa::EventKind::WillExecute { database_key } = event.kind {
                    log_clone
                        .lock()
                        .unwrap()
                        .push(format!("  salsa: {database_key:?}"));
                }
            }))),
            tcx: std::sync::Arc::new(tcx),
            query_log: log,
        }
    }

    /// Create a database with a `ProxyTcxDb`, sharing the query log.
    pub fn with_proxy(req_tx: std::sync::mpsc::Sender<crate::tcx::TcxRequest>) -> Self {
        let log = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let log_clone = log.clone();
        let proxy = crate::tcx::ProxyTcxDb::new(req_tx, log.clone());
        Self {
            storage: salsa::Storage::new(Some(Box::new(move |event| {
                if let salsa::EventKind::WillExecute { database_key } = event.kind {
                    log_clone
                        .lock()
                        .unwrap()
                        .push(format!("  salsa: {database_key:?}"));
                }
            }))),
            tcx: std::sync::Arc::new(proxy),
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

    fn log_query(&self, entry: String) {
        self.query_log.lock().unwrap().push(entry);
    }
}

#[salsa::db]
impl salsa::Database for Database {}
