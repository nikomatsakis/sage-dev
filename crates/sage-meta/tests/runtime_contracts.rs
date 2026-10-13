#![feature(register_tool)]
#![register_tool(sage)]

use std::sync::Arc;

use sage_meta::contracts::{
    ContractEvent, ContractModel, ContractSession, EntryKind, UncheckedEffect, ViolationKind,
};
use sage_meta::parse;

fn model(body: &str) -> Arc<ContractModel> {
    let document = parse(
        "demo/crate.sage",
        &format!("crate demo {{ \"\"\"Test contracts.\"\"\" }} {body}"),
    )
    .unwrap();
    Arc::new(ContractModel::new(&[document]).unwrap())
}

fn session(model: Arc<ContractModel>, roots: &[&str]) -> ContractSession {
    ContractSession::new(model, roots.iter().map(|root| format!("demo::{root}")))
}

const FORBIDDEN: &str = r#"
class Root { """Start."""
    query run() -> u32 { """Call helper.""" envelope {
        invokes crate::Helper::work();
        forbid transitive crate::Leaf::read;
    } }
}
class Helper { """Intermediate."""
    query work() -> u32 { """No calls allowed.""" envelope {} }
}
class Leaf { """Final."""
    query read() -> u32 { """Read.""" envelope {} }
}
"#;

#[test]
#[sage::test(meta.calls.declared)]
#[sage::test(meta.calls.forbidden)]
fn reports_direct_and_ancestor_violation_with_complete_path_and_rule() {
    let mut session = session(model(FORBIDDEN), &["Root::run"]);
    let root = session.enter("demo::Root::run", Some("root:1"));
    let helper = session.enter("demo::Helper::work", Some("helper:2"));
    let leaf = session.enter("demo::Leaf::read", Some("leaf:3"));
    session.exit(leaf);
    session.exit(helper);
    session.exit(root);
    let report = session.finish();
    assert!(!report.calls_conform);
    assert_eq!(report.coverage.checked_requests, 3);
    assert_eq!(report.violations.len(), 2);
    assert_eq!(report.violations[0].kind, ViolationKind::UndeclaredCall);
    let forbidden = &report.violations[1];
    assert_eq!(forbidden.kind, ViolationKind::ForbiddenTransitive);
    assert_eq!(
        forbidden.rule.as_ref().unwrap().operation,
        "demo::Root::run"
    );
    assert_eq!(
        forbidden
            .path
            .iter()
            .map(|frame| frame.operation.as_str())
            .collect::<Vec<_>>(),
        ["demo::Root::run", "demo::Helper::work", "demo::Leaf::read"]
    );
    assert_eq!(forbidden.path[2].receiver.as_deref(), Some("leaf:3"));
    assert_eq!(report.trace.len(), 6);
}

#[test]
#[sage::test(meta.calls.forbidden)]
fn immediate_permission_does_not_override_an_active_ancestor_prohibition() {
    let model = model(
        r#"
        class Root { """Suspended query."""
            fn run() -> u32 { """Forbids the leaf.""" envelope {
                forbid transitive crate::Leaf::read;
            } }
        }
        class Helper { """Internal reexecution."""
            fn work() -> u32 { """May request the leaf.""" envelope {
                invokes crate::Leaf::read();
            } }
        }
        class Leaf { """Value."""
            fn read() -> u32 { """Read.""" envelope {} }
        }
    "#,
    );
    let mut checked = session(model, &["Root::run"]);
    let root = checked.enter("demo::Root::run", None);
    // Internal reexecution installs Helper beneath the suspended Root without
    // inventing a Root -> Helper source call. Helper's own request is permitted.
    let helper = checked.enter_validation("demo::Helper::work", None);
    let leaf = checked.enter("demo::Leaf::read", None);
    checked.exit(leaf);
    checked.exit(helper);
    checked.exit(root);
    let report = checked.finish();
    assert!(!report.calls_conform);
    assert_eq!(report.violations.len(), 1);
    assert_eq!(
        report.violations[0].kind,
        ViolationKind::ForbiddenTransitive
    );
    assert_eq!(
        report.violations[0].rule.as_ref().unwrap().operation,
        "demo::Root::run"
    );
    assert_eq!(
        report.violations[0]
            .path
            .iter()
            .map(|frame| frame.operation.as_str())
            .collect::<Vec<_>>(),
        ["demo::Root::run", "demo::Helper::work", "demo::Leaf::read"]
    );
}

#[test]
#[sage::test(meta.contracts.validated_session)]
fn repeated_roots_are_independent_and_models_are_shared_immutably() {
    let model = model(FORBIDDEN);
    let mut first = session(model.clone(), &["Root::run", "Leaf::read"]);
    // Prewarming a leaf does not grant future callers permission to request it.
    let leaf = first.enter("demo::Leaf::read", None);
    first.exit(leaf);
    let root = first.enter("demo::Root::run", None);
    let helper = first.enter("demo::Helper::work", None);
    first.exit(helper);
    first.exit(root);
    first.finish().assert_calls_conform();
    let mut second = session(model, &["Root::run"]);
    let root = second.enter("demo::Root::run", None);
    let leaf = second.enter("demo::Leaf::read", None);
    second.exit(leaf);
    second.exit(root);
    assert_eq!(second.finish().violations.len(), 2);
}

#[test]
#[sage::test(meta.calls.declared)]
#[sage::test(meta.contracts.cache_invocations)]
#[sage::test(meta.envelopes.receiver_calls)]
fn same_operation_entry_is_recursion_and_needs_permission() {
    let leaf = r#"class Tree { """Tree.""" fn walk() -> u32 { """Walk.""" envelope {} } }"#;
    let mut denied = session(model(leaf), &["Tree::walk"]);
    let a = denied.enter("demo::Tree::walk", Some("tree:1"));
    let b = denied.enter("demo::Tree::walk", Some("tree:1"));
    denied.exit(b);
    denied.exit(a);
    assert_eq!(
        denied.finish().violations[0].kind,
        ViolationKind::UndeclaredCall
    );

    let recursive = leaf.replace("envelope {}", "envelope { invokes self.walk(); }");
    let mut allowed = session(model(&recursive), &["Tree::walk"]);
    let a = allowed.enter("demo::Tree::walk", Some("tree:1"));
    let b = allowed.enter("demo::Tree::walk", Some("tree:1"));
    allowed.exit(b);
    allowed.exit(a);
    let report = allowed.finish();
    report.assert_calls_conform();
    assert_eq!(report.coverage.checked_requests, 2);
    assert!(matches!(&report.trace[1], ContractEvent::Enter(frame) if frame.parent == Some(a)));
}

#[test]
#[sage::test(meta.contracts.observation_coverage)]
#[sage::test(meta.envelopes.receiver_calls)]
fn self_permission_rejects_different_and_unknown_receivers() {
    let model = model(
        r#"
        class Item { """Item."""
            fn run() -> u32 { """Run.""" envelope { invokes self.read(); } }
            query read() -> u32 { """Read.""" envelope {} }
        }
    "#,
    );
    for receiver in [Some("item:other"), None] {
        let mut checked = session(model.clone(), &["Item::run"]);
        let run = checked.enter("demo::Item::run", Some("item:1"));
        let read = checked.enter("demo::Item::read", receiver);
        checked.exit(read);
        checked.exit(run);
        let report = checked.finish();
        assert!(!report.calls_conform);
        if receiver.is_some() {
            assert_eq!(report.violations[0].kind, ViolationKind::ReceiverMismatch);
        } else {
            assert!(report.coverage.gaps[0].contains("receiver identity required"));
        }
    }
}

#[test]
#[sage::test(meta.envelopes.class_calls)]
fn a_class_wide_permission_does_not_require_receiver_identity() {
    let mut session = session(
        model(
            r#"
        class Item { """Item."""
            fn run() -> u32 { """Run.""" envelope {
                invokes self.read(); invokes crate::Item::read();
            } }
            query read() -> u32 { """Read.""" envelope {} }
        }
    "#,
        ),
        &["Item::run"],
    );
    let run = session.enter("demo::Item::run", None);
    let read = session.enter("demo::Item::read", None);
    session.exit(read);
    session.exit(run);
    session.finish().assert_calls_conform();
}

#[test]
fn interface_dispatch_retains_both_frames_and_receiver_check() {
    let model = model(
        r#"
        abstract class Base { """Interface."""
            query read() -> u32 { """Read.""" abstract; }
        }
        class Child extends crate::Base { """Implementation."""
            override query read() -> u32 { """Read.""" envelope {} }
        }
        class Caller { """Caller."""
            fn run() -> u32 { """Run.""" envelope { invokes crate::Base::read(); } }
        }
    "#,
    );
    for receiver in ["item:1", "item:other"] {
        let mut checked = session(model.clone(), &["Caller::run"]);
        let caller = checked.enter("demo::Caller::run", None);
        let interface = checked.enter("demo::Base::read", Some("item:1"));
        let implementation = checked.enter("demo::Child::read", Some(receiver));
        checked.exit(implementation);
        checked.exit(interface);
        checked.exit(caller);
        let report = checked.finish();
        assert_eq!(report.calls_conform, receiver == "item:1");
        assert_eq!(report.coverage.checked_requests, 3);
    }
}

#[test]
#[sage::test(meta.envelopes.class_calls)]
fn overrides_inherit_prohibitions_and_calls_dispatch_to_known_implementations() {
    let mut session = session(
        model(
            r#"
        class Base { """Base."""
            query run() -> u32 { """Run.""" envelope { forbid transitive crate::Leaf::read; } }
        }
        class Child extends crate::Base { """Child."""
            override query run() -> u32 { """Run.""" envelope {} }
        }
        class Leaf { """Leaf.""" query read() -> u32 { """Read.""" envelope {} } }
        class Caller { """Caller."""
            fn run() -> u32 { """Run.""" envelope { invokes crate::Base::run(); } }
        }
    "#,
        ),
        &["Caller::run"],
    );
    let caller = session.enter("demo::Caller::run", None);
    let child = session.enter("demo::Child::run", None);
    let leaf = session.enter("demo::Leaf::read", None);
    session.exit(leaf);
    session.exit(child);
    session.exit(caller);
    let report = session.finish();
    assert_eq!(report.violations.len(), 2);
    assert_eq!(
        report.violations[1].rule.as_ref().unwrap().operation,
        "demo::Base::run"
    );
}

#[test]
fn inherited_self_prohibition_compares_the_ancestor_receiver() {
    let model = model(
        r#"
        class Item { """Item."""
            fn run() -> u32 { """Run.""" envelope {
                invokes crate::Helper::work(); forbid transitive self.read;
            } }
            query read() -> u32 { """Read.""" envelope {} }
        }
        class Helper { """Helper.""" fn work() -> u32 { """Work.""" envelope {} } }
    "#,
    );
    for receiver in [Some("item:1"), Some("item:2"), None] {
        let mut checked = session(model.clone(), &["Item::run"]);
        let run = checked.enter("demo::Item::run", Some("item:1"));
        let helper = checked.enter("demo::Helper::work", Some("helper:1"));
        let leaf = checked.enter("demo::Item::read", receiver);
        checked.exit(leaf);
        checked.exit(helper);
        checked.exit(run);
        let report = checked.finish();
        assert_eq!(
            report.violations.len(),
            if receiver == Some("item:1") { 2 } else { 1 }
        );
        assert_eq!(report.coverage.gaps.is_empty(), receiver.is_some());
    }
}

#[test]
#[sage::test(meta.contracts.cache_invocations)]
fn internal_reexecution_establishes_context_without_fabricating_a_call() {
    let mut session = session(
        model(
            r#"
        class Root { """Root.""" query run() -> u32 { """Run.""" envelope {} } }
        class Dependency { """Dependency."""
            query run() -> u32 { """Run.""" envelope { invokes crate::Leaf::read(); } }
        }
        class Leaf { """Leaf.""" query read() -> u32 { """Read.""" envelope {} } }
    "#,
        ),
        &["Root::run"],
    );
    let root = session.enter("demo::Root::run", None);
    let validation = session.enter_validation("demo::Dependency::run", None);
    let leaf = session.enter("demo::Leaf::read", None);
    session.exit(leaf);
    session.exit(validation);
    session.exit(root);
    let report = session.finish();
    report.assert_calls_conform();
    assert_eq!(report.coverage.validation_contexts, 1);
    assert!(
        matches!(&report.trace[1], ContractEvent::Enter(frame) if frame.kind == EntryKind::Validation)
    );
}

#[test]
#[sage::test(meta.calls.forbidden)]
fn internal_reexecution_cannot_bypass_an_active_ancestor_prohibition() {
    let mut session = session(model(FORBIDDEN), &["Root::run"]);
    let root = session.enter("demo::Root::run", None);
    // This execution may come from a memo created in an earlier unchecked
    // session. There is no source call to invent, but the restriction applies.
    let validation = session.enter_validation("demo::Leaf::read", None);
    session.exit(validation);
    session.exit(root);
    let report = session.finish();
    assert!(!report.calls_conform);
    assert!(report.coverage.gaps.is_empty());
    assert_eq!(report.coverage.checked_requests, 1);
    assert_eq!(report.coverage.validation_contexts, 1);
    assert_eq!(report.violations.len(), 1);
    assert_eq!(
        report.violations[0].kind,
        ViolationKind::ForbiddenTransitive
    );
    assert_eq!(
        report.violations[0].rule.as_ref().unwrap().operation,
        "demo::Root::run"
    );
    assert_eq!(report.violations[0].path[1].kind, EntryKind::Validation);
    assert!(report.violations[0].message.contains("execution"));
}

#[test]
fn internal_reexecution_checks_inherited_self_prohibitions_conservatively() {
    let model = model(
        r#"
        class Base { """Base."""
            fn run() -> u32 { """Run.""" envelope { forbid transitive self.read; } }
            query read() -> u32 { """Read.""" envelope {} }
        }
        class Child extends crate::Base { """Child."""
            override fn run() -> u32 { """Run.""" envelope {} }
        }
        "#,
    );
    for receiver in [Some("item:1"), Some("item:2"), None] {
        let mut checked = session(model.clone(), &["Child::run"]);
        let root = checked.enter("demo::Child::run", Some("item:1"));
        let validation = checked.enter_validation("demo::Base::read", receiver);
        checked.exit(validation);
        checked.exit(root);
        let report = checked.finish();
        assert_eq!(report.calls_conform, receiver == Some("item:2"));
        assert_eq!(report.coverage.gaps.is_empty(), receiver.is_some());
        if receiver == Some("item:1") {
            assert_eq!(report.violations.len(), 1);
            assert_eq!(
                report.violations[0].kind,
                ViolationKind::ForbiddenTransitive
            );
            assert_eq!(
                report.violations[0].rule.as_ref().unwrap().operation,
                "demo::Base::run"
            );
        } else {
            assert!(report.violations.is_empty());
        }
    }
}

#[test]
#[sage::test(meta.contracts.observation_coverage)]
fn unknown_boundaries_roots_and_unbalanced_exits_cannot_pass() {
    let mut session = session(model(FORBIDDEN), &["Root::run", "Missing::run"]);
    let root = session.enter("demo::Helper::work", None);
    let unknown = session.enter("demo::Missing::run", None);
    session.exit(root);
    session.exit(unknown);
    let report = session.finish();
    assert!(!report.calls_conform);
    assert!(
        report
            .violations
            .iter()
            .any(|v| v.kind == ViolationKind::UnknownOperation)
    );
    assert!(
        report
            .violations
            .iter()
            .any(|v| v.kind == ViolationKind::UnexpectedRoot)
    );
    assert!(
        report
            .violations
            .iter()
            .any(|v| v.kind == ViolationKind::UnbalancedExit)
    );
    assert!(
        report
            .coverage
            .gaps
            .iter()
            .any(|gap| gap.contains("unclosed"))
    );
    assert!(
        report
            .coverage
            .gaps
            .iter()
            .any(|gap| gap.contains("configured root"))
    );
}

#[test]
#[sage::test(meta.contracts.observation_coverage)]
fn observation_gaps_and_effect_coverage_are_explicit() {
    let model = model(
        r#"
        context Context { """Context.""" }
        class Root { """Root.""" input value: u32;
            fn run() -> u32 { """Run.""" envelope {
                reads self.value; creates context: crate::Context;
            } }
        }
    "#,
    );
    let mut checked = session(model.clone(), &["Root::run"]);
    let run = checked.enter("demo::Root::run", None);
    checked.exit(run);
    let report = checked.finish();
    report.assert_calls_conform();
    assert_eq!(
        report
            .coverage
            .unchecked_effects
            .into_iter()
            .collect::<Vec<_>>(),
        [UncheckedEffect::Reads, UncheckedEffect::Creates]
    );

    let mut incomplete = session(model, &["Root::run"]);
    incomplete.record_gap("unmapped Salsa query at input.rs:42");
    assert!(!incomplete.finish().calls_conform);
}

#[test]
#[sage::test(meta.contracts.validated_session)]
fn invalid_static_model_never_becomes_a_runtime_contract() {
    let document = parse(
        "demo/crate.sage",
        r#"
        crate demo { """Demo.""" }
        class Root { """Root."""
            fn run() -> u32 { """Run.""" envelope {
                invokes self.run(); forbid transitive self.run;
            } }
        }
    "#,
    )
    .unwrap();
    let diagnostics = ContractModel::new(&[document]).unwrap_err();
    assert!(
        diagnostics
            .iter()
            .any(|error| error.message.contains("forbidden transitive call"))
    );
}
