#![feature(register_tool)]
#![register_tool(sage)]

use std::sync::Arc;

use sage_meta::contracts::{ContractModel, ContractSession, ViolationKind};
use sage_meta::{Class, ClassKind, Declaration, Document, Effect, FieldKind, MethodKind};

fn parse(body: &str) -> Document {
    sage_meta::parse(
        "demo/crate.sage",
        &format!("crate demo {{ \"\"\"Envelope tests.\"\"\" }} {body}"),
    )
    .unwrap()
}

fn valid(body: &str) -> Document {
    let document = parse(body);
    let errors = sage_meta::validate(std::slice::from_ref(&document));
    assert!(errors.is_empty(), "{errors:?}");
    document
}

fn class<'a>(document: &'a Document, name: &str) -> &'a Class {
    document
        .declarations
        .iter()
        .find_map(|declaration| match declaration {
            Declaration::Class(class) if class.name == name => Some(class),
            Declaration::Class(_) | Declaration::Type(_) => None,
        })
        .unwrap()
}

#[test]
#[sage::test(meta.classes.single_inheritance)]
fn declarations_accept_one_base_and_reject_multiple_bases() {
    for kind in ["class", "context"] {
        let document = valid(&format!(
            r#"{kind} Base {{ """Base.""" }}
            {kind} Child extends crate::Base {{ """Child.""" }}"#
        ));
        assert_eq!(
            class(&document, "Child").base.as_ref().unwrap().segments,
            ["Base"]
        );
        for bases in [
            "crate::Base, crate::Other",
            "crate::Base extends crate::Other",
        ] {
            assert!(
                sage_meta::parse(
                    "demo/crate.sage",
                    &format!(r#"crate demo {{ """Demo.""" }} {kind} Child extends {bases} {{ """Child.""" }}"#),
                )
                .is_err(),
                "accepted multiple bases: {kind} {bases}",
            );
        }
    }
}

#[test]
#[sage::test(meta.contexts.inheritance)]
fn context_inheritance_stays_within_the_context_kind() {
    let document = valid(
        r#"context Base { """Working state.""" }
        context Child extends crate::Base { """Extended state.""" }"#,
    );
    assert_eq!(class(&document, "Child").kind, ClassKind::Context);
    for (base, child) in [
        ("class", "context"),
        ("abstract class", "context"),
        ("context", "class"),
        ("context", "abstract class"),
    ] {
        let document = parse(&format!(
            r#"{base} Base {{ """Base.""" }}
            {child} Child extends crate::Base {{ """Child.""" }}"#
        ));
        assert!(sage_meta::validate(&[document]).iter().any(|error| {
            error
                .message
                .contains("contexts and symbol classes cannot inherit from each other")
        }));
    }
}

#[test]
#[sage::test(meta.fields.kinds)]
fn fields_preserve_identity_input_and_state_kinds() {
    let document = valid(
        r#"class Item { """Identity and input.""" identity key: u32; input value: u32; }
        context Work { """Local state.""" state pending: u32; }"#,
    );
    assert_eq!(class(&document, "Item").fields[0].kind, FieldKind::Identity);
    assert_eq!(class(&document, "Item").fields[1].kind, FieldKind::Input);
    assert_eq!(class(&document, "Work").fields[0].kind, FieldKind::State);
}

#[test]
#[sage::test(meta.fields.context_state)]
fn working_state_fields_require_a_context() {
    valid(r#"context Work { """Work.""" state pending: u32; }"#);
    for kind in ["class", "abstract class"] {
        let document = parse(&format!(
            r#"{kind} Item {{ """Item.""" state pending: u32; }}"#
        ));
        assert!(
            sage_meta::validate(&[document])
                .iter()
                .any(|error| error.message.contains("state fields require a context"))
        );
    }
}

#[test]
#[sage::test(meta.methods.kinds)]
fn query_and_function_kinds_survive_contract_resolution() {
    let document = valid(
        r#"class Item { """Item."""
            query signature() -> u32 { """A product.""" envelope {} }
            fn work() -> u32 { """A computation.""" envelope {} }
        }"#,
    );
    assert_eq!(class(&document, "Item").methods[0].kind, MethodKind::Query);
    assert_eq!(
        class(&document, "Item").methods[1].kind,
        MethodKind::Function
    );
    let model = ContractModel::new(&[document]).unwrap();
    assert_eq!(
        model.operations()["demo::Item::signature"].kind,
        MethodKind::Query
    );
    assert_eq!(
        model.operations()["demo::Item::work"].kind,
        MethodKind::Function
    );
}

#[test]
#[sage::test(meta.methods.prose)]
fn method_prose_is_required_first_and_does_not_grant_calls() {
    let document = valid(
        r#"class Item { """Item."""
            fn work() -> u32 {
                """invokes $missing::Other::run(); { prose is not an envelope }"""
                envelope {}
            }
        }"#,
    );
    assert_eq!(
        class(&document, "Item").methods[0].description,
        "invokes $missing::Other::run(); { prose is not an envelope }"
    );
    assert!(
        ContractModel::new(&[document]).unwrap().operations()["demo::Item::work"]
            .invokes
            .is_empty()
    );
    for body in ["envelope {}", "abstract;", r#"envelope {} """Too late.""""#] {
        let source = format!(
            r#"crate demo {{ """Demo.""" }} abstract class Item {{ """Item."""
            fn work() -> u32 {{ {body} }} }}"#
        );
        assert!(
            sage_meta::parse("demo/crate.sage", &source).is_err(),
            "{body}"
        );
    }
}

#[test]
#[sage::test(meta.envelopes.reads)]
fn reads_accept_inherited_fields_and_reject_unknown_or_foreign_fields() {
    let source = r#"class Base { """Base.""" input value: u32; }
        class Child extends crate::Base { """Child."""
            fn work() -> u32 { """Read input.""" envelope { reads self.value; } }
        }"#;
    valid(source);
    for path in ["self.missing", "crate::Base::value"] {
        let document = parse(&source.replace("self.value", path));
        let errors = sage_meta::validate(&[document]);
        assert!(!errors.is_empty(), "accepted invalid field read: {path}");
    }
}

#[test]
#[sage::test(meta.envelopes.creates)]
fn creates_resolves_a_context_and_retains_its_binding() {
    let source = r#"context Work { """Local work.""" }
        class Item { """Item."""
            fn work() -> u32 { """Create work.""" envelope { creates cx: crate::Work; } }
        }"#;
    let document = valid(source);
    assert!(matches!(
        &class(&document, "Item").methods[0].envelope.as_ref().unwrap()[0],
        Effect::Creates { name, context } if name == "cx" && context.segments == ["Work"]
    ));
    for target in ["crate::Item", "crate::Missing"] {
        let document = parse(&source.replace("crate::Work", target));
        assert!(
            sage_meta::validate(&[document])
                .iter()
                .any(|error| error.message.contains("not a context")),
            "{target}"
        );
    }
}

#[test]
#[sage::test(meta.envelopes.leaf)]
fn empty_envelopes_are_leaves_and_are_distinct_from_abstract_methods() {
    let document = valid(
        r#"abstract class Item { """Item."""
            fn abstract_work() -> u32 { """Abstract.""" abstract; }
            fn leaf() -> u32 { """Leaf.""" envelope {} }
        }"#,
    );
    assert_eq!(class(&document, "Item").methods[0].envelope, None);
    assert_eq!(class(&document, "Item").methods[1].envelope, Some(vec![]));
    let model = ContractModel::new(&[document]).unwrap();
    let leaf = &model.operations()["demo::Item::leaf"];
    assert!(!leaf.is_abstract);
    assert!(leaf.invokes.is_empty());
    assert!(leaf.prohibits.is_empty());
    assert!(leaf.unchecked_effects.is_empty());
    assert!(model.operations()["demo::Item::abstract_work"].is_abstract);
}

#[test]
#[sage::test(meta.envelopes.permissions)]
fn permitted_calls_are_optional_and_have_no_execution_order() {
    let model = Arc::new(
        ContractModel::new(&[valid(
            r#"class Root { """Root.""" fn run() -> u32 { """Run.""" envelope {
            invokes crate::Other::first(); invokes crate::Other::second();
        } } }
        class Other { """Other."""
            fn first() -> u32 { """First.""" envelope {} }
            fn second() -> u32 { """Second.""" envelope {} }
        }"#,
        )])
        .unwrap(),
    );
    for calls in [
        &[][..],
        &["demo::Other::second"][..],
        &["demo::Other::second", "demo::Other::first"][..],
    ] {
        let mut session = ContractSession::new(model.clone(), ["demo::Root::run".into()]);
        let root = session.enter("demo::Root::run", None);
        for operation in calls {
            let call = session.enter(operation, None);
            session.exit(call);
        }
        session.exit(root);
        session.finish().assert_calls_conform();
    }
}

#[test]
#[sage::test(meta.envelopes.direct_calls)]
#[sage::test(meta.model.forbidden_paths)]
fn indirect_reachability_does_not_grant_a_direct_call_permission() {
    let source = r#"class Root { """Root.""" fn run() -> u32 { """Run.""" envelope {
            invokes crate::Helper::work();
        } } }
        class Helper { """Helper.""" fn work() -> u32 { """Work.""" envelope {
            invokes crate::Leaf::read();
        } } }
        class Leaf { """Leaf.""" fn read() -> u32 { """Read.""" envelope {} } }"#;
    let model = Arc::new(ContractModel::new(&[valid(source)]).unwrap());
    assert_eq!(
        model.operations()["demo::Root::run"].invokes[0].targets,
        ["demo::Helper::work".into()].into_iter().collect()
    );
    let mut session = ContractSession::new(model, ["demo::Root::run".into()]);
    let root = session.enter("demo::Root::run", None);
    let indirect = session.enter("demo::Leaf::read", None);
    session.exit(indirect);
    session.exit(root);
    assert_eq!(
        session.finish().violations[0].kind,
        ViolationKind::UndeclaredCall
    );

    let prohibited = source.replace(
        "invokes crate::Helper::work();",
        "invokes crate::Helper::work(); forbid transitive crate::Leaf::read;",
    );
    let errors = sage_meta::validate(&[parse(&prohibited)]);
    assert!(errors.iter().any(|error| {
        error
            .message
            .contains("demo::Root::run -> demo::Helper::work -> demo::Leaf::read")
    }));
    valid(&prohibited.replace("invokes crate::Helper::work();", ""));
}
