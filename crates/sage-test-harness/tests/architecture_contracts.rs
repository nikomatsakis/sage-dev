#![feature(rustc_private)]
//! Production query evidence for the source-correspondence and contract checker.
//! The checked scope is explicit: setup queries run before the observer is armed.

use std::path::Path;
use std::sync::{Arc, Mutex, OnceLock};

use sage_ir::db::Database;
use sage_ir::local_syms::fns::LocalFnSym;
use sage_ir::symbol::{FnSymbol, ModSymbol, SymbolData};
use sage_meta::contracts::{ContractModel, ViolationKind};
use sage_meta::correspondence::CorrespondenceIndex;
use sage_meta::salsa_contracts::{QueryEventKind, QueryOutcome, SalsaObserver, SalsaReport};
use salsa::Database as _;

const SIGNATURE: &str = "sage_ir::symbols::LocalFnSym::signature";
const BODY: &str = "sage_ir::symbols::LocalFnSym::body";
const FUNCTION_SOURCE: &str = include_str!("../../sage-ir/src/local_syms/fns.rs");
const MODULE_SOURCE: &str = include_str!("../../sage-ir/src/local_syms/mods.rs");

fn observer(roots: &[&str]) -> SalsaObserver {
    static CONTRACTS: OnceLock<(Arc<ContractModel>, Arc<CorrespondenceIndex>)> = OnceLock::new();
    let (model, index) = CONTRACTS.get_or_init(|| {
        let workspace = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let model_root = sage_meta::workspace::default_model_root(&workspace).unwrap();
        let documents = sage_meta::workspace::load_documents(&model_root).unwrap();
        let index = CorrespondenceIndex::load(&workspace).unwrap();
        assert!(index.validate(&documents).is_empty());
        (
            Arc::new(ContractModel::new(&documents).unwrap()),
            Arc::new(index),
        )
    });
    SalsaObserver::new(
        model.clone(),
        index.clone(),
        roots.iter().map(|root| root.to_string()),
        &[
            ("crates/sage-ir/src/local_syms/fns.rs", FUNCTION_SOURCE),
            ("crates/sage-ir/src/local_syms/mods.rs", MODULE_SOURCE),
        ],
    )
    .unwrap()
}

fn database() -> (Database, Arc<Mutex<Option<SalsaObserver>>>) {
    let state = Arc::new(Mutex::new(None::<SalsaObserver>));
    let sink = state.clone();
    let database = Database::with_event_observer(sage_ir::tcx::NoopTcxDb, move |event| {
        if let Some(observer) = sink.lock().unwrap().as_mut() {
            observer.event(event);
        }
    });
    (database, state)
}

fn report(state: &Mutex<Option<SalsaObserver>>) -> SalsaReport {
    state.lock().unwrap().take().unwrap().finish()
}

fn local_function<'db>(
    db: &'db dyn sage_ir::Db,
    module: ModSymbol<'db>,
    name: &str,
) -> LocalFnSym<'db> {
    module
        .expanded_module_items(db)
        .iter()
        .find_map(|symbol| match symbol.data(db) {
            SymbolData::FnSymbol(FnSymbol::Local(function)) => {
                (function.name(db).text(db) == name).then_some(function)
            }
            SymbolData::FnSymbol(FnSymbol::Ext(_))
            | SymbolData::StructSymbol(_)
            | SymbolData::EnumSymbol(_)
            | SymbolData::VariantSymbol(_)
            | SymbolData::VariantCtorSymbol(_)
            | SymbolData::TraitSymbol(_)
            | SymbolData::TypeAliasSymbol(_)
            | SymbolData::ConstSymbol(_)
            | SymbolData::StaticSymbol(_)
            | SymbolData::ImplSymbol(_)
            | SymbolData::ModSymbol(_)
            | SymbolData::MacroDefSymbol(_)
            | SymbolData::UseSymbol(_)
            | SymbolData::IntrinsicTypeSymbol(_)
            | SymbolData::MacroInvocationSymbol(_) => None,
        })
        .expect("fixture function")
}

#[test]
fn real_signature_maps_to_its_original_method_on_cold_and_reused_requests() {
    let (mut database, state) = database();
    let file = database.add_source_file(
        "lib.rs".into(),
        include_str!("fixtures/contracts/lib.rs").into(),
    );
    database.attach(|db| {
        let (_, module) = sage_test_harness::setup_root_module(db, file);
        let function = local_function(db, module, "leaf");
        for (outcome, kinds) in [
            (
                QueryOutcome::Executed,
                vec![
                    QueryEventKind::Request,
                    QueryEventKind::Execute,
                    QueryEventKind::Executed,
                    QueryEventKind::Return,
                ],
            ),
            (
                QueryOutcome::Reused,
                vec![QueryEventKind::Request, QueryEventKind::Return],
            ),
        ] {
            *state.lock().unwrap() = Some(observer(&[SIGNATURE]));
            let signature = function.sig(db);
            let (stash, binder) = signature.open();
            assert!(stash[binder.value.params].is_empty());
            let report = report(&state);
            report.contracts.assert_calls_conform();
            assert_eq!(
                report
                    .events
                    .iter()
                    .map(|event| event.kind.clone())
                    .collect::<Vec<_>>(),
                kinds,
                "{:#?}",
                report.events
            );
            for event in &report.events {
                assert_eq!(event.operation.as_deref(), Some(SIGNATURE));
                assert_eq!(
                    event.site.as_ref().unwrap().file,
                    "crates/sage-ir/src/local_syms/fns.rs"
                );
                assert_eq!(
                    event.site.as_ref().unwrap().rust_path,
                    "sage_ir::local_syms::fns::LocalFnSym<'db>::sig"
                );
            }
            assert_eq!(report.events.last().unwrap().outcome, Some(outcome));
            assert_eq!(report.contracts.coverage.checked_requests, 1);
        }
    });
}

#[test]
fn real_body_query_reports_missing_coverage_instead_of_claiming_conformance() {
    let (mut database, state) = database();
    let file = database.add_source_file(
        "lib.rs".into(),
        include_str!("fixtures/contracts/lib.rs").into(),
    );
    database.attach(|db| {
        let (_, module) = sage_test_harness::setup_root_module(db, file);
        let function = local_function(db, module, "caller");
        *state.lock().unwrap() = Some(observer(&[BODY]));
        let checked = function.body(db);
        assert!(checked.diagnostics.is_empty(), "{:?}", checked.diagnostics);
        let report = report(&state);
        // Expected independently of source-index output: caller body, its own
        // interface, module lookup, then the callee interface. Ordinary context
        // frames remain absent until separately instrumented.
        let requests = report
            .events
            .iter()
            .filter(|event| event.kind == QueryEventKind::Request)
            .collect::<Vec<_>>();
        assert_eq!(
            requests
                .iter()
                .map(|event| (
                    event.operation.as_deref(),
                    event.site.as_ref().map(|site| site.rust_path.as_str()),
                ))
                .collect::<Vec<_>>(),
            [
                (
                    Some(BODY),
                    Some("sage_ir::local_syms::fns::LocalFnSym<'db>::body")
                ),
                (
                    Some(SIGNATURE),
                    Some("sage_ir::local_syms::fns::LocalFnSym<'db>::sig")
                ),
                (
                    Some("sage_ir::modules::LocalModSym::expanded_items"),
                    Some("sage_ir::local_syms::mods::local_expanded_module_items")
                ),
                (
                    Some(SIGNATURE),
                    Some("sage_ir::local_syms::fns::LocalFnSym<'db>::sig")
                ),
            ]
        );
        assert_ne!(
            requests[1].key, requests[3].key,
            "caller and callee have distinct query keys"
        );
        assert_eq!(report.contracts.violations.len(), 1);
        assert_eq!(
            report.contracts.violations[0].kind,
            ViolationKind::UndeclaredCall
        );
        assert!(
            report.contracts.violations[0]
                .message
                .contains("LocalFnSym::body -> sage_ir::modules::LocalModSym::expanded_items")
        );
        assert!(!report.contracts.calls_conform);
        assert!(
            report
                .contracts
                .coverage
                .gaps
                .iter()
                .any(|gap| gap.contains("receiver identity required")),
            "{report:#?}"
        );
        assert!(
            report
                .events
                .iter()
                .any(|event| event.operation.as_deref() == Some(SIGNATURE))
        );
        assert_eq!(
            report
                .events
                .iter()
                .filter(|event| event.kind == QueryEventKind::Request
                    && event.operation.as_deref() == Some(BODY))
                .count(),
            1,
            "callee bodies must not be requested"
        );
    });
}
