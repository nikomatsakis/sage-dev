use std::path::Path;
use std::sync::{Arc, Mutex};

use sage_meta::contracts::{ContractModel, ContractReport, ViolationKind};
use sage_meta::correspondence::{CorrespondenceIndex, LookupError, RustSource};
use sage_meta::salsa_contracts::{QueryEventKind, SalsaObserver};
use salsa::Setter;

use crate::queries::*;

const ROOT: &str = "sage_contract_fixture::nodes::Root::run";
const LEAF: &str = "sage_contract_fixture::nodes::Leaf::read";
const SOURCE: &str = include_str!("queries.rs");

#[salsa::db]
#[derive(Clone)]
struct Db {
    storage: salsa::Storage<Self>,
}
#[salsa::db]
impl salsa::Database for Db {}

struct Observer {
    index: Arc<CorrespondenceIndex>,
    compiled_source: String,
    session: Option<SalsaObserver>,
    lines: Vec<String>,
}

impl Observer {
    fn event(&mut self, event: salsa::Event) {
        let session = self.session.as_mut().expect("active fixture session");
        let count = session.events().len();
        session.event(&event);
        if session.events().len() == count {
            return;
        }
        let event = session.events().last().unwrap();
        let (Some(operation), Some(site)) = (&event.operation, &event.site) else {
            return;
        };
        let short = operation
            .strip_prefix("sage_contract_fixture::nodes::")
            .unwrap();
        let site = site
            .rust_path
            .strip_prefix("sage_contract_fixture::")
            .unwrap();
        let action = match event.kind {
            QueryEventKind::Request => "request",
            QueryEventKind::Execute => "execute",
            QueryEventKind::Executed => "executed",
            QueryEventKind::Return => "return",
        };
        let suffix = if let Some(outcome) = &event.outcome {
            format!(" [{outcome:?}]")
        } else if event.accumulator_traversal {
            " [accumulator]".into()
        } else {
            String::new()
        };
        self.lines
            .push(format!("{action} {short} @ {site}{suffix}"));
    }

    fn reset(&mut self) {
        self.session = Some(session(self.index.clone(), &self.compiled_source));
        self.lines.clear();
    }

    fn report(&mut self) -> ContractReport {
        self.session.take().unwrap().finish().contracts
    }
}

fn documents() -> Vec<sage_meta::Document> {
    sage_meta::workspace::load_documents(&Path::new(env!("CARGO_MANIFEST_DIR")).join("sage-src"))
        .unwrap()
}

fn session(index: Arc<CorrespondenceIndex>, compiled_source: &str) -> SalsaObserver {
    SalsaObserver::new(
        Arc::new(ContractModel::new(&documents()).unwrap()),
        index,
        [ROOT.into(), LEAF.into()],
        &[("src/queries.rs", compiled_source)],
    )
    .unwrap()
}

fn index() -> CorrespondenceIndex {
    let index = CorrespondenceIndex::load(Path::new(env!("CARGO_MANIFEST_DIR"))).unwrap();
    index.verify_source("src/queries.rs", SOURCE).unwrap();
    assert!(
        index.validate(&documents()).is_empty(),
        "{:?}",
        index.validate(&documents())
    );
    index
}

fn database(index: CorrespondenceIndex) -> (Db, Arc<Mutex<Observer>>) {
    database_with_snapshot(index, SOURCE)
}

fn database_with_snapshot(
    index: CorrespondenceIndex,
    compiled_source: &str,
) -> (Db, Arc<Mutex<Observer>>) {
    let index = Arc::new(index);
    let observer = Arc::new(Mutex::new(Observer {
        session: Some(session(index.clone(), compiled_source)),
        index,
        compiled_source: compiled_source.into(),
        lines: vec![],
    }));
    let sink = observer.clone();
    let db = Db {
        storage: salsa::Storage::new(Some(Box::new(move |event| {
            sink.lock().unwrap().event(event)
        }))),
    };
    (db, observer)
}

#[test]
fn cold_warm_and_internal_reexecution_have_reviewed_traversals() {
    let (mut db, observer) = database(index());
    let input = Input::new(&db, 10, false, false);
    assert_eq!(start(&db, input), 5);
    let cold = observer.lock().unwrap().lines.join("\n");
    assert_eq!(cold, include_str!("../expected/cold.txt").trim());
    observer.lock().unwrap().report().assert_calls_conform();
    observer.lock().unwrap().reset();
    assert_eq!(start(&db, input), 5);
    assert_eq!(
        observer.lock().unwrap().lines.join("\n"),
        include_str!("../expected/warm.txt").trim()
    );
    observer.lock().unwrap().report().assert_calls_conform();

    // Helper reexecutes while validating Root; equal result lets Root remain cached.
    observer.lock().unwrap().reset();
    input.set_value(&mut db).to(11);
    assert_eq!(start(&db, input), 5);
    assert_eq!(
        observer.lock().unwrap().lines.join("\n"),
        include_str!("../expected/validated.txt").trim()
    );
    let report = observer.lock().unwrap().report();
    report.assert_calls_conform();
    assert_eq!(report.coverage.validation_contexts, 1);
}

#[test]
fn forbidden_requests_fail_even_when_leaf_is_cached() {
    for prewarm in [false, true] {
        let (db, observer) = database(index());
        let input = Input::new(&db, 10, true, false);
        if prewarm {
            assert_eq!(forbidden(&db, input), 10);
            observer.lock().unwrap().report().assert_calls_conform();
            observer.lock().unwrap().reset();
        }
        assert_eq!(start(&db, input), 10);
        let report = observer.lock().unwrap().report();
        assert!(!report.calls_conform);
        let violations: Vec<_> = report
            .violations
            .iter()
            .map(|violation| {
                let path = violation
                    .path
                    .iter()
                    .map(|frame| {
                        frame
                            .operation
                            .strip_prefix("sage_contract_fixture::nodes::")
                            .unwrap()
                    })
                    .collect::<Vec<_>>()
                    .join(" -> ");
                format!("{:?}: {path}", violation.kind)
            })
            .collect();
        assert_eq!(
            violations.join("\n"),
            include_str!("../expected/forbidden.txt").trim()
        );
        assert!(
            report
                .violations
                .iter()
                .any(|v| v.kind == ViolationKind::UndeclaredCall)
        );
        assert!(
            report
                .violations
                .iter()
                .any(|v| v.kind == ViolationKind::ForbiddenTransitive)
        );
        assert!(report.coverage.gaps.is_empty());
        if prewarm {
            let lines = &observer.lock().unwrap().lines;
            assert!(
                lines
                    .iter()
                    .any(|line| line.contains("return Leaf::read") && line.contains("[Reused]"))
            );
            assert!(
                !lines
                    .iter()
                    .any(|line| line.starts_with("execute Leaf::read"))
            );
        }
    }
}

#[test]
fn panic_balances_scopes_and_next_session_is_clean() {
    let (mut db, observer) = database(index());
    let input = Input::new(&db, 10, false, true);
    assert!(std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| start(&db, input))).is_err());
    let report = observer.lock().unwrap().report();
    assert!(
        !report
            .violations
            .iter()
            .any(|v| v.kind == ViolationKind::UnbalancedExit)
    );
    assert!(
        observer
            .lock()
            .unwrap()
            .lines
            .iter()
            .any(|line| line.contains("[Cancelled]"))
    );
    observer.lock().unwrap().reset();
    input.set_fail(&mut db).to(false);
    assert_eq!(start(&db, input), 5);
    observer.lock().unwrap().report().assert_calls_conform();
}

#[test]
fn generated_accumulator_api_remains_observable_without_invented_source_calls() {
    let (db, observer) = database(index());
    let input = Input::new(&db, 10, false, false);
    start(&db, input);
    observer.lock().unwrap().report().assert_calls_conform();
    observer.lock().unwrap().reset();
    assert_eq!(start::accumulated::<Note>(&db, input)[0].0, 10);
    let lines = &observer.lock().unwrap().lines.clone();
    assert!(lines.iter().any(|line| line.contains("[accumulator]")));
    observer.lock().unwrap().report().assert_calls_conform();
}

fn modified_index(source: String) -> CorrespondenceIndex {
    CorrespondenceIndex::from_sources(&[RustSource {
        crate_name: "sage_contract_fixture".into(),
        target: "lib:sage_contract_fixture".into(),
        file: "src/queries.rs".into(),
        source,
    }])
    .unwrap()
}

#[test]
fn missing_and_swapped_annotations_cannot_produce_a_false_pass() {
    // Preserve source line numbers while deliberately testing the matcher with
    // a faulty annotation inventory; normal session setup rejects stale source.
    let missing = SOURCE.replace("#[sage::item(crate::nodes::Helper::work)]", "");
    let (db, observer) = database_with_snapshot(modified_index(missing.clone()), &missing);
    assert_eq!(start(&db, Input::new(&db, 10, false, false)), 5);
    let report = observer.lock().unwrap().report();
    assert!(!report.calls_conform);
    assert!(
        report
            .coverage
            .gaps
            .iter()
            .any(|gap| gap.contains("unmapped"))
    );

    let swapped = SOURCE.replace(
        "#[sage::item(crate::nodes::Helper::work)]",
        "#[sage::item(crate::nodes::Leaf::read)]",
    );
    let (db, observer) = database_with_snapshot(modified_index(swapped.clone()), &swapped);
    assert_eq!(start(&db, Input::new(&db, 10, false, false)), 5);
    let report = observer.lock().unwrap().report();
    assert!(!report.calls_conform);
    assert!(
        report
            .violations
            .iter()
            .any(|v| v.kind == ViolationKind::ForbiddenTransitive)
    );
}

#[test]
fn ambiguous_sites_and_stale_sources_are_rejected() {
    let source = "#[sage::item(crate::nodes::Helper::work)] fn a() {} #[sage::item(crate::nodes::Leaf::read)] fn b() {}";
    let index = modified_index(source.into());
    assert!(matches!(
        index.lookup("src/queries.rs", 1),
        Err(LookupError::Ambiguous { .. })
    ));
    assert!(matches!(
        index.verify_source("src/queries.rs", SOURCE),
        Err(LookupError::SourceMismatch { .. })
    ));
}

#[test]
fn methods_in_one_impl_keep_distinct_source_locations() {
    let (db, observer) = database(index());
    let input = Input::new(&db, 10, false, false);
    assert_eq!(input.another(&db), 10);
    assert_eq!(
        observer.lock().unwrap().lines.join("\n"),
        "request Leaf::read @ queries::Input::another\nexecute Leaf::read @ queries::Input::another\nexecuted Leaf::read @ queries::Input::another\nreturn Leaf::read @ queries::Input::another [Executed]"
    );
    observer.lock().unwrap().report().assert_calls_conform();
}

#[test]
fn unsupported_macro_generated_queries_are_visible_coverage_gaps() {
    let (db, observer) = database(index());
    let input = Input::new(&db, 10, false, false);
    assert_eq!(generated(&db, input), 10);
    let report = observer.lock().unwrap().report();
    assert!(!report.calls_conform);
    assert!(
        report
            .coverage
            .gaps
            .iter()
            .any(|gap| gap.contains("unmapped"))
    );
}

#[test]
fn unpaired_source_cannot_produce_a_conformance_success() {
    let observer = SalsaObserver::new(
        Arc::new(ContractModel::new(&documents()).unwrap()),
        Arc::new(index()),
        [ROOT.into()],
        &[],
    )
    .unwrap();
    let observer = Arc::new(Mutex::new(Some(observer)));
    let sink = observer.clone();
    let db = Db {
        storage: salsa::Storage::new(Some(Box::new(move |event| {
            sink.lock().unwrap().as_mut().unwrap().event(&event);
        }))),
    };
    assert_eq!(start(&db, Input::new(&db, 10, false, false)), 5);
    let report = observer.lock().unwrap().take().unwrap().finish();
    assert!(!report.contracts.calls_conform);
    assert!(
        report
            .contracts
            .coverage
            .gaps
            .iter()
            .any(|gap| gap.contains("compiled snapshot"))
    );
    assert!(report.events.iter().all(|event| event.operation.is_none()));
}

#[test]
fn moving_execution_between_threads_is_an_explicit_gap() {
    let (db, observer) = database(index());
    let input = Input::new(&db, 10, false, false);
    assert_eq!(start(&db, input), 5);
    assert_eq!(
        std::thread::spawn(move || start(&db, input))
            .join()
            .unwrap(),
        5
    );
    let report = observer.lock().unwrap().report();
    assert!(!report.calls_conform);
    assert!(
        report
            .coverage
            .gaps
            .iter()
            .any(|gap| gap.contains("another thread"))
    );
}

#[test]
fn malformed_lifecycle_replays_cannot_report_conformance() {
    // Capture the actual leaf query events instead of inventing database keys
    // or declaration locations; replay only changes their ordering/completeness.
    let events = Arc::new(Mutex::new(Vec::new()));
    let sink = events.clone();
    let db = Db {
        storage: salsa::Storage::new(Some(Box::new(move |event| {
            if event.query_source.is_some() {
                sink.lock().unwrap().push(event);
            }
        }))),
    };
    assert_eq!(forbidden(&db, Input::new(&db, 10, false, false)), 10);
    let captured = events.lock().unwrap();
    assert_eq!(captured.len(), 4);
    let model_index = Arc::new(index());
    let replay = |order: &[usize]| {
        let mut observer = session(model_index.clone(), SOURCE);
        for &position in order {
            observer.event(&captured[position]);
        }
        observer.finish().contracts
    };
    replay(&[0, 1, 2, 3]).assert_calls_conform();
    for (order, diagnostic) in [
        (&[0, 1, 3, 2][..], "nesting order"),
        (&[0, 3][..], "Executed without execution events"),
        (&[0, 1, 3][..], "nesting order"),
        (&[0, 2, 3][..], "nesting order"),
    ] {
        let report = replay(order);
        assert!(!report.calls_conform, "malformed order {order:?} passed");
        assert!(
            report
                .coverage
                .gaps
                .iter()
                .any(|gap| gap.contains(diagnostic)),
            "{report:?}"
        );
    }
    let salsa::EventKind::DidRequest { database_key, .. } = captured[3].kind else {
        panic!("expected the real query's return event");
    };
    for (disposition, cause, diagnostic) in [
        (
            salsa::QueryDisposition::Reused,
            salsa::RequestCause::Fetch,
            "cached disposition after executing",
        ),
        (
            salsa::QueryDisposition::Executed,
            salsa::RequestCause::AccumulatorTraversal,
            "cause changed",
        ),
    ] {
        let mut observer = session(model_index.clone(), SOURCE);
        for event in &captured[..3] {
            observer.event(event);
        }
        let mut return_event = salsa::Event::new(salsa::EventKind::DidRequest {
            database_key,
            disposition,
        });
        return_event.query_source = captured[3].query_source;
        return_event.request_cause = Some(cause);
        observer.event(&return_event);
        let report = observer.finish().contracts;
        assert!(!report.calls_conform);
        assert!(
            report
                .coverage
                .gaps
                .iter()
                .any(|gap| gap.contains(diagnostic)),
            "{report:?}"
        );
    }
}
