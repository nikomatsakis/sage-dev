//! Observe one database on one thread using immutable contracts and source snapshots.
//!
//! Callbacks retain errors instead of panicking inside Salsa. Finish the observer
//! and call `report.contracts.assert_calls_conform()` at the test/session boundary.
//! Ordinary functions, receiver projection, and async propagation require their
//! own adapters; this adapter observes tracked-query requests and execution only.
use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use salsa::{DatabaseKeyIndex, EventKind, IngredientIndex, QueryDisposition, RequestCause};

use crate::contracts::{ContractModel, ContractReport, ContractSession};
use crate::correspondence::{CorrespondenceIndex, LookupError, RustSite};

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
pub enum QueryEventKind {
    Request,
    Execute,
    Executed,
    Return,
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
pub enum QueryOutcome {
    Executed,
    Validated,
    Reused,
    Cancelled,
}

#[derive(Clone, Debug, serde::Serialize)]
pub struct QueryEvent {
    pub kind: QueryEventKind,
    /// A session-local query definition number, independent of debug formatting.
    pub definition: usize,
    pub key: u64,
    pub file: String,
    pub line: usize,
    pub operation: Option<String>,
    pub site: Option<RustSite>,
    pub accumulator_traversal: bool,
    pub outcome: Option<QueryOutcome>,
}

#[derive(Debug, serde::Serialize)]
pub struct SalsaReport {
    pub contracts: ContractReport,
    pub events: Vec<QueryEvent>,
}

#[derive(Clone)]
struct Mapping {
    operation: String,
    site: RustSite,
}

struct Definition {
    id: usize,
    source: salsa::QuerySource,
    mapping: Result<Mapping, LookupError>,
}

enum ScopeKind {
    Request {
        executed: bool,
        cause: Option<RequestCause>,
    },
    Execution,
}

struct Scope {
    key: DatabaseKeyIndex,
    invocation: Option<u64>,
    kind: ScopeKind,
}

/// An observer is bound to one database and one sequential thread. Create a
/// fresh observer for another database or session; no global mutable state is used.
pub struct SalsaObserver {
    index: Arc<CorrespondenceIndex>,
    verified_files: BTreeSet<String>,
    definitions: BTreeMap<IngredientIndex, Definition>,
    session: ContractSession,
    scopes: Vec<Scope>,
    events: Vec<QueryEvent>,
    thread: Option<std::thread::ThreadId>,
}

impl SalsaObserver {
    /// Pair runtime-loaded source with text embedded in this executable, usually
    /// via `include_str!`. Unpaired files are rejected when observed. Two copies
    /// read from disk cannot establish compatibility with the running binary.
    pub fn new(
        model: Arc<ContractModel>,
        index: Arc<CorrespondenceIndex>,
        roots: impl IntoIterator<Item = String>,
        compiled_sources: &[(&str, &str)],
    ) -> Result<Self, LookupError> {
        let mut verified_files = BTreeSet::new();
        for (file, source) in compiled_sources {
            index.verify_source(file, source)?;
            verified_files.insert(index.source_key(file));
        }
        Ok(Self {
            index,
            verified_files,
            definitions: BTreeMap::new(),
            session: ContractSession::new(model, roots),
            scopes: Vec::new(),
            events: Vec::new(),
            thread: None,
        })
    }

    /// Consume the raw callback, including cache hits. Diagnostics accumulate and
    /// prevent a passing report; this method does not panic for malformed events.
    pub fn event(&mut self, event: &salsa::Event) {
        let (key, kind, outcome) = match event.kind {
            EventKind::WillRequest { database_key } => {
                (database_key, QueryEventKind::Request, None)
            }
            EventKind::WillExecute { database_key } => {
                (database_key, QueryEventKind::Execute, None)
            }
            EventKind::DidExecute { database_key } => {
                (database_key, QueryEventKind::Executed, None)
            }
            EventKind::DidRequest {
                database_key,
                disposition,
            } => {
                let outcome = match disposition {
                    QueryDisposition::Executed => QueryOutcome::Executed,
                    QueryDisposition::Validated => QueryOutcome::Validated,
                    QueryDisposition::Reused => QueryOutcome::Reused,
                    QueryDisposition::Cancelled => QueryOutcome::Cancelled,
                };
                (database_key, QueryEventKind::Return, Some(outcome))
            }
            _ => return,
        };
        if self.thread.is_some_and(|thread| thread != event.thread_id) {
            self.session.record_gap("Salsa observer received events on another thread; context propagation is unsupported");
            return;
        }
        self.thread = Some(event.thread_id);
        let Some(source) = event.query_source else {
            self.session
                .record_gap("Salsa lifecycle event has no source provenance");
            return;
        };
        let ingredient = key.ingredient_index();
        let next_id = self.definitions.len();
        let definition = self.definitions.entry(ingredient).or_insert_with(|| {
            let mapping = self
                .index
                .lookup(source.file, source.line as usize)
                .map(|matched| Mapping {
                    operation: matched.sage_item.to_owned(),
                    site: matched.site.clone(),
                })
                .and_then(|mapping| {
                    if self.verified_files.contains(&mapping.site.file) {
                        Ok(mapping)
                    } else {
                        Err(LookupError::SourceMismatch {
                            file: mapping.site.file,
                        })
                    }
                });
            Definition {
                id: next_id,
                source,
                mapping,
            }
        });
        if definition.source != source {
            self.session.record_gap(
                "query definition changed source provenance within one database session",
            );
        }
        let mapping = match &definition.mapping {
            Ok(mapping) => Some(mapping),
            Err(error) => {
                self.session.record_gap(error.to_string());
                None
            }
        };
        let accumulator = event.request_cause == Some(RequestCause::AccumulatorTraversal);
        if matches!(kind, QueryEventKind::Request | QueryEventKind::Return)
            && event.request_cause.is_none()
        {
            self.session
                .record_gap("Salsa request lacks its fetch/traversal cause");
        }
        self.events.push(QueryEvent {
            kind: kind.clone(),
            definition: definition.id,
            key: key.key_index().as_bits(),
            file: self.index.source_key(source.file),
            line: source.line as usize,
            operation: mapping.map(|mapping| mapping.operation.clone()),
            site: mapping.map(|mapping| mapping.site.clone()),
            accumulator_traversal: accumulator,
            outcome: outcome.clone(),
        });
        match kind {
            QueryEventKind::Request => {
                let id = mapping.map(|mapping| {
                    if accumulator {
                        self.session.enter_validation(&mapping.operation, None)
                    } else {
                        self.session.enter(&mapping.operation, None)
                    }
                });
                self.scopes.push(Scope {
                    key,
                    invocation: id,
                    kind: ScopeKind::Request {
                        executed: false,
                        cause: event.request_cause,
                    },
                });
            }
            QueryEventKind::Return => {
                if let Some(Scope {
                    key: active_key,
                    kind: ScopeKind::Request { executed, cause },
                    ..
                }) = self.scopes.last()
                    && *active_key == key
                {
                    if outcome == Some(QueryOutcome::Executed) && !executed {
                        self.session
                            .record_gap("Salsa request returned Executed without execution events");
                    }
                    if matches!(
                        outcome,
                        Some(QueryOutcome::Reused | QueryOutcome::Validated)
                    ) && *executed
                    {
                        self.session.record_gap(
                            "Salsa request returned a cached disposition after executing",
                        );
                    }
                    if *cause != event.request_cause {
                        self.session
                            .record_gap("Salsa request cause changed between entry and return");
                    }
                }
                self.exit(key, false);
            }
            QueryEventKind::Execute => {
                let requested = if let Some(Scope {
                    key: active_key,
                    kind: ScopeKind::Request { executed, .. },
                    ..
                }) = self.scopes.last_mut()
                    && *active_key == key
                {
                    *executed = true;
                    true
                } else {
                    false
                };
                let id = if requested {
                    None
                } else {
                    mapping.map(|mapping| self.session.enter_validation(&mapping.operation, None))
                };
                self.scopes.push(Scope {
                    key,
                    invocation: id,
                    kind: ScopeKind::Execution,
                });
            }
            QueryEventKind::Executed => {
                self.exit(key, true);
            }
        }
    }

    fn exit(&mut self, key: DatabaseKeyIndex, execution: bool) {
        let matches = self.scopes.last().is_some_and(|scope| {
            scope.key == key
                && match scope.kind {
                    ScopeKind::Request { .. } => !execution,
                    ScopeKind::Execution => execution,
                }
        });
        if !matches {
            let kind = if execution { "execution" } else { "request" };
            self.session.record_gap(format!(
                "unbalanced Salsa {kind} event: scopes must close in nesting order"
            ));
            return;
        }
        if let Some(Scope {
            invocation: Some(id),
            ..
        }) = self.scopes.pop()
        {
            self.session.exit(id);
        }
    }

    pub fn events(&self) -> &[QueryEvent] {
        &self.events
    }

    pub fn finish(mut self) -> SalsaReport {
        if !self.scopes.is_empty() {
            self.session
                .record_gap("unclosed Salsa request or execution scopes");
        }
        SalsaReport {
            contracts: self.session.finish(),
            events: self.events,
        }
    }
}
