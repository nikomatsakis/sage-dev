use std::sync::Arc;

use crate::tcx::{NoopTcxDb, TcxDb};

#[salsa::db]
#[derive(Clone)]
pub struct Database {
    storage: salsa::Storage<Self>,
    tcx: Arc<dyn TcxDb + Send + Sync>,
    query_log: Arc<std::sync::Mutex<Vec<String>>>,
}

impl Database {
    pub fn new(tcx: impl TcxDb + Send + Sync + 'static) -> Self {
        let log = Arc::new(std::sync::Mutex::new(Vec::new()));
        let log_clone = log.clone();
        Self {
            storage: salsa::Storage::new(Some(Box::new(move |event| {
                if let salsa::EventKind::WillExecute { database_key } = event.kind {
                    log_clone.lock().unwrap().push(format!("{database_key:?}"));
                }
            }))),
            tcx: Arc::new(tcx),
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
