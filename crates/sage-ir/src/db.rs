use std::collections::HashMap;

use crate::source::SourceFile;
use crate::tcx::{NoopTcxDb, TcxDb};
use salsa::Setter as _;

/// Salsa database for sage-ir.
#[salsa::db]
#[derive(Clone)]
pub struct Database {
    storage: salsa::Storage<Self>,
    tcx: std::sync::Arc<dyn TcxDb>,
    query_log: std::sync::Arc<std::sync::Mutex<Vec<String>>>,
    inspection_log: std::sync::Arc<std::sync::Mutex<Vec<InspectionEvent>>>,
    files: HashMap<String, SourceFile>,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum InspectionSource {
    Sage,
    Solver,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum InspectionChildOrder {
    Sequential,
    Unordered,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum InspectionEvent {
    PhaseEnter {
        phase: &'static str,
    },
    PhaseExit {
        phase: &'static str,
    },
    SpanEnter {
        operation: &'static str,
        source: InspectionSource,
        child_order: InspectionChildOrder,
    },
    SpanExit {
        operation: &'static str,
    },
    QueryEnter {
        key: String,
    },
    QueryExit {
        key: String,
        disposition: salsa::QueryDisposition,
    },
    QueryLeaf {
        key: String,
        disposition: salsa::QueryDisposition,
        observations: u64,
    },
    ExternalMetadata {
        operation: String,
    },
}

/// A balanced producer-authored semantic span in the inspection trace.
///
/// The exit is emitted from `Drop` so cancellation and unwinding cannot leave
/// the dynamic trace stack open.
pub struct InspectionSpan<'db> {
    db: &'db dyn crate::Db,
    operation: &'static str,
    source: InspectionSource,
    child_order: InspectionChildOrder,
}

impl<'db> InspectionSpan<'db> {
    pub fn new(
        db: &'db dyn crate::Db,
        operation: &'static str,
        source: InspectionSource,
        child_order: InspectionChildOrder,
    ) -> Self {
        db.log_inspection_span(operation, source, child_order, true);
        Self {
            db,
            operation,
            source,
            child_order,
        }
    }
}

impl Drop for InspectionSpan<'_> {
    fn drop(&mut self) {
        self.db
            .log_inspection_span(self.operation, self.source, self.child_order, false);
    }
}

/// A semantic span which is active only while its inner future is being
/// polled.
///
/// Poll scoping preserves dynamic parentage when multiple solver futures are
/// interleaved on one thread: a pending future closes its span before a
/// sibling is polled.
pub struct InspectionFuture<'db, F> {
    db: &'db dyn crate::Db,
    operation: &'static str,
    source: InspectionSource,
    child_order: InspectionChildOrder,
    future: std::pin::Pin<Box<F>>,
}

impl<'db, F> InspectionFuture<'db, F> {
    pub fn new(
        db: &'db dyn crate::Db,
        operation: &'static str,
        source: InspectionSource,
        child_order: InspectionChildOrder,
        future: F,
    ) -> Self {
        Self {
            db,
            operation,
            source,
            child_order,
            future: Box::pin(future),
        }
    }
}

impl<F: std::future::Future> std::future::Future for InspectionFuture<'_, F> {
    type Output = F::Output;

    fn poll(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Self::Output> {
        let this = self.get_mut();
        let _span = InspectionSpan::new(this.db, this.operation, this.source, this.child_order);
        this.future.as_mut().poll(cx)
    }
}

impl Database {
    fn storage_and_logs() -> (
        salsa::Storage<Self>,
        std::sync::Arc<std::sync::Mutex<Vec<String>>>,
        std::sync::Arc<std::sync::Mutex<Vec<InspectionEvent>>>,
    ) {
        let query_log = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let query_log_for_events = query_log.clone();
        let inspection_log = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let inspection_log_for_events = inspection_log.clone();
        let storage = salsa::Storage::new(Some(Box::new(move |event| match event.kind {
            salsa::EventKind::WillRequest { database_key } => {
                inspection_log_for_events
                    .lock()
                    .unwrap()
                    .push(InspectionEvent::QueryEnter {
                        key: format!("{database_key:?}"),
                    });
            }
            salsa::EventKind::DidRequest {
                database_key,
                disposition,
            } => {
                let key = format!("{database_key:?}");
                let mut events = inspection_log_for_events.lock().unwrap();
                let len = events.len();
                let current_is_leaf = matches!(
                    events.last(),
                    Some(InspectionEvent::QueryEnter { key: entered }) if entered == &key
                );
                if current_is_leaf && len >= 2 {
                    let repeated_leaf = matches!(
                        &events[len - 2],
                        InspectionEvent::QueryLeaf {
                            key: previous,
                            disposition: previous_disposition,
                            ..
                        } if previous == &key && *previous_disposition == disposition
                    );
                    if repeated_leaf {
                        events.pop();
                        let Some(InspectionEvent::QueryLeaf { observations, .. }) =
                            events.last_mut()
                        else {
                            unreachable!()
                        };
                        *observations += 1;
                        return;
                    }
                }
                if current_is_leaf && len >= 3 {
                    let repeated_pair = matches!(
                        (&events[len - 3], &events[len - 2]),
                        (
                            InspectionEvent::QueryEnter { key: previous_enter },
                            InspectionEvent::QueryExit {
                                key: previous_exit,
                                disposition: previous_disposition,
                            },
                        ) if previous_enter == &key
                            && previous_exit == &key
                            && *previous_disposition == disposition
                    );
                    if repeated_pair {
                        events.truncate(len - 3);
                        events.push(InspectionEvent::QueryLeaf {
                            key,
                            disposition,
                            observations: 2,
                        });
                        return;
                    }
                }
                events.push(InspectionEvent::QueryExit { key, disposition });
            }
            salsa::EventKind::WillExecute { database_key } => {
                query_log_for_events
                    .lock()
                    .unwrap()
                    .push(format!("  salsa: {database_key:?}"));
            }
            _ => {}
        })));
        (storage, query_log, inspection_log)
    }

    // ANCHOR: architecture_query_execution_log
    pub fn new(tcx: impl TcxDb + 'static) -> Self {
        let (storage, query_log, inspection_log) = Self::storage_and_logs();
        Self {
            storage,
            tcx: std::sync::Arc::new(tcx),
            query_log,
            inspection_log,
            files: HashMap::new(),
        }
    }
    // ANCHOR_END: architecture_query_execution_log

    /// Create a database with a `ProxyTcxDb`, sharing the query log.
    pub fn with_proxy(req_tx: std::sync::mpsc::Sender<crate::tcx::TcxRequest>) -> Self {
        let (storage, query_log, inspection_log) = Self::storage_and_logs();
        let proxy = crate::tcx::ProxyTcxDb::new(req_tx, query_log.clone(), inspection_log.clone());
        Self {
            storage,
            tcx: std::sync::Arc::new(proxy),
            query_log,
            inspection_log,
            files: HashMap::new(),
        }
    }

    pub fn add_source_file(&mut self, path: String, text: String) -> SourceFile {
        let file = SourceFile::new(self, path.clone(), text);
        self.files.insert(path, file);
        file
    }

    /// Update one existing source input without reconstructing the database.
    pub fn set_source_text(&mut self, path: &str, text: String) -> Result<String, String> {
        let file = self
            .files
            .get(path)
            .copied()
            .ok_or_else(|| format!("source file `{path}` is not registered"))?;
        Ok(file.set_text(self).to(text))
    }

    pub fn source_files(&self) -> impl Iterator<Item = (&str, SourceFile)> + '_ {
        self.files.iter().map(|(path, file)| (path.as_str(), *file))
    }

    /// Drain the query log and return it as a newline-separated string.
    pub fn take_query_log(&self) -> String {
        let mut log = self.query_log.lock().unwrap();
        let out = log.join("\n");
        log.clear();
        out
    }

    pub fn take_inspection_log(&self) -> Vec<InspectionEvent> {
        std::mem::take(&mut *self.inspection_log.lock().unwrap())
    }

    pub fn salsa_revision(&self) -> u64 {
        salsa::plumbing::current_revision(self).as_u64()
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

    fn log_inspection_phase(&self, phase: &'static str, entering: bool) {
        self.inspection_log.lock().unwrap().push(if entering {
            InspectionEvent::PhaseEnter { phase }
        } else {
            InspectionEvent::PhaseExit { phase }
        });
    }

    fn log_inspection_span(
        &self,
        operation: &'static str,
        source: InspectionSource,
        child_order: InspectionChildOrder,
        entering: bool,
    ) {
        self.inspection_log.lock().unwrap().push(if entering {
            InspectionEvent::SpanEnter {
                operation,
                source,
                child_order,
            }
        } else {
            InspectionEvent::SpanExit { operation }
        });
    }

    fn source_file(&self, path: &str) -> Option<SourceFile> {
        self.files.get(path).copied()
    }
}

#[salsa::db]
impl salsa::Database for Database {}

#[cfg(test)]
mod tests {
    use super::*;
    use salsa::Accumulator as _;

    #[salsa::accumulator]
    struct TracedAccumulatedValue(#[allow(dead_code)] usize);

    #[salsa::tracked]
    fn traced_accumulated_leaf(db: &dyn crate::Db, source: SourceFile) {
        TracedAccumulatedValue(source.text(db).len()).accumulate(db);
    }

    #[salsa::tracked]
    fn traced_accumulated_root(db: &dyn crate::Db, source: SourceFile) {
        traced_accumulated_leaf(db, source);
    }

    fn dispositions_for(
        events: &[InspectionEvent],
        query_name: &str,
    ) -> Vec<salsa::QueryDisposition> {
        let mut dispositions = Vec::new();
        for event in events {
            match event {
                InspectionEvent::QueryExit { key, disposition } if key.contains(query_name) => {
                    dispositions.push(*disposition);
                }
                InspectionEvent::QueryLeaf {
                    key,
                    disposition,
                    observations,
                } if key.contains(query_name) => {
                    dispositions.extend(std::iter::repeat_n(*disposition, *observations as usize));
                }
                InspectionEvent::PhaseEnter { .. }
                | InspectionEvent::PhaseExit { .. }
                | InspectionEvent::SpanEnter { .. }
                | InspectionEvent::SpanExit { .. }
                | InspectionEvent::QueryEnter { .. }
                | InspectionEvent::QueryExit { .. }
                | InspectionEvent::QueryLeaf { .. }
                | InspectionEvent::ExternalMetadata { .. } => {}
            }
        }
        dispositions
    }

    #[test]
    fn pending_inspection_futures_close_before_a_sibling_poll() {
        let database = Database::default();
        let _ = database.take_inspection_log();
        let mut first = Box::pin(InspectionFuture::new(
            &database,
            "first",
            InspectionSource::Solver,
            InspectionChildOrder::Unordered,
            std::future::pending::<()>(),
        ));
        let mut second = Box::pin(InspectionFuture::new(
            &database,
            "second",
            InspectionSource::Solver,
            InspectionChildOrder::Unordered,
            std::future::pending::<()>(),
        ));
        let mut cx = std::task::Context::from_waker(std::task::Waker::noop());

        assert!(matches!(
            std::future::Future::poll(first.as_mut(), &mut cx),
            std::task::Poll::Pending
        ));
        assert!(matches!(
            std::future::Future::poll(second.as_mut(), &mut cx),
            std::task::Poll::Pending
        ));

        assert!(matches!(
            database.take_inspection_log().as_slice(),
            [
                InspectionEvent::SpanEnter {
                    operation: "first",
                    ..
                },
                InspectionEvent::SpanExit { operation: "first" },
                InspectionEvent::SpanEnter {
                    operation: "second",
                    ..
                },
                InspectionEvent::SpanExit {
                    operation: "second"
                },
            ]
        ));
    }

    #[test]
    fn accumulated_value_refreshes_have_complete_request_lifecycles() {
        use salsa::Database as _;

        let mut database = Database::default();
        let source = database.add_source_file("lib.rs".to_owned(), "fn item() {}".to_owned());
        let unrelated = database.add_source_file("other.rs".to_owned(), "fn other() {}".to_owned());

        database.attach(|db| {
            let values = traced_accumulated_root::accumulated::<TracedAccumulatedValue>(db, source);
            assert_eq!(values.len(), 1);
        });
        let cold = database.take_inspection_log();
        assert!(
            dispositions_for(&cold, "traced_accumulated_leaf")
                .contains(&salsa::QueryDisposition::Executed)
        );

        database.attach(|db| {
            let values = traced_accumulated_root::accumulated::<TracedAccumulatedValue>(db, source);
            assert_eq!(values.len(), 1);
        });
        let warm = database.take_inspection_log();
        assert_eq!(
            dispositions_for(&warm, "traced_accumulated_leaf"),
            [salsa::QueryDisposition::Reused],
            "accumulator graph traversal must expose its leaf refresh"
        );

        unrelated
            .set_text(&mut database)
            .to("fn other() { let _ = 1; }".to_owned());
        database.attach(|db| {
            let _ = traced_accumulated_root::accumulated::<TracedAccumulatedValue>(db, source);
        });
        let validated = database.take_inspection_log();
        assert!(
            dispositions_for(&validated, "traced_accumulated_root")
                .contains(&salsa::QueryDisposition::Validated)
        );

        database.cancellation_token().cancel();
        let cancelled = salsa::Cancelled::catch(std::panic::AssertUnwindSafe(|| {
            database.attach(|db| {
                let _ = traced_accumulated_root::accumulated::<TracedAccumulatedValue>(db, source);
            });
        }));
        assert!(matches!(cancelled, Err(salsa::Cancelled::Local)));
        assert_eq!(
            dispositions_for(&database.take_inspection_log(), "traced_accumulated_root"),
            [salsa::QueryDisposition::Cancelled]
        );
    }
}
