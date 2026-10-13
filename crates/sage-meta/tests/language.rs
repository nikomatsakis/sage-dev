#![feature(register_tool)]
#![register_tool(sage)]

use sage_meta::{Declaration, Effect, Header, parse, validate};

fn check(body: &str) -> Vec<sage_meta::Diagnostic> {
    let source =
        format!("crate demo {{ \"\"\"Demo.\"\"\" }} type Value \"\"\"Result.\"\"\"; {body}");
    validate(&[parse("demo/crate.sage", &source).unwrap()])
}

#[test]
fn parses_prose_paths_and_contexts_into_ast() {
    let document = parse(
        "demo/crate.sage",
        r#"
        crate demo { """Unicode λ and punctuation {;}.""" depends $other; module symbols; }
        type Value """Result.""";
        context Work { """Local state.""" state value: crate::Value; }
        class Item { """Identity."""
            input value: crate::Value;
            query run() -> crate::Value { """Compute.""" envelope {
                reads self.value;
                creates work: crate::Work;
                invokes $other::symbols::Item::signature();
                forbid transitive crate::Item::body;
            } }
        }
    "#,
    )
    .unwrap();
    assert!(
        matches!(document.header, Header::Crate { ref dependencies, .. } if dependencies == &["other"])
    );
    let Declaration::Class(class) = &document.declarations[2] else {
        panic!()
    };
    let effects = class.methods[0].envelope.as_ref().unwrap();
    assert!(
        matches!(&effects[2], Effect::Invokes(path) if path.root == "$other" && path.segments == ["symbols", "Item", "signature"])
    );
}

#[test]
#[sage::test(meta.language.model.forbidden_paths)]
#[sage::test(meta.language.model.virtual_dispatch)]
fn detects_virtual_dispatch_path_to_forbidden_body() {
    let errors = check(
        r#"
        abstract class Function { """Interface."""
            query signature() -> crate::Value { """Interface.""" abstract; }
        }
        class Local extends crate::Function { """Local."""
            override query signature() -> crate::Value { """Bad dependency.""" envelope { invokes self.body(); } }
            query body() -> crate::Value { """Body.""" envelope {} }
        }
        class Other extends crate::Function { """Another implementation."""
            override query signature() -> crate::Value { """Also forbidden.""" envelope { invokes self.body(); } }
            query body() -> crate::Value { """Other body.""" envelope {} }
        }
        class Caller { """Consumer."""
            query run() -> crate::Value { """Read interfaces.""" envelope {
                invokes crate::Function::signature();
                forbid transitive crate::Local::body;
                forbid transitive crate::Other::body;
            } }
        }
    "#,
    );
    assert_eq!(errors.len(), 2, "{errors:?}");
    for implementation in ["Local", "Other"] {
        let path = format!(
            "demo::Caller::run -> demo::{implementation}::signature -> demo::{implementation}::body"
        );
        assert!(
            errors.iter().any(|error| error.message.contains(&path)),
            "{errors:?}"
        );
    }
}

#[test]
#[sage::test(meta.language.model.forbidden_paths)]
fn permits_recursive_graph_without_forbidden_reachability() {
    assert!(
        check(
            r#"
        class Item { """Recursive."""
            fn walk() -> crate::Value { """Walk.""" envelope {
                invokes self.walk(); forbid transitive crate::Item::body;
            } }
            query body() -> crate::Value { """Body.""" envelope {} }
        }
    "#
        )
        .is_empty()
    );
}

#[test]
fn rejects_unknown_calls_and_missing_dependency() {
    let errors = check(
        r#"
        class Item { """Item."""
            fn run() -> crate::Value { """Run.""" envelope {
                invokes self.missing(); invokes $other::Thing::run();
            } }
        }
    "#,
    );
    assert!(errors.iter().any(|e| e.message.contains("unknown method")));
    assert!(
        errors
            .iter()
            .any(|e| e.message.contains("undeclared dependency"))
    );
}

#[test]
#[sage::test(meta.language.classes.abstract_complete)]
fn rejects_cycles_and_incomplete_concrete_classes() {
    let errors = check(
        r#"
        abstract class Base { """Base.""" fn run() -> crate::Value { """Run.""" abstract; } }
        class Concrete extends crate::Base { """Missing implementation.""" }
        class A extends crate::B { """Cycle.""" }
        class B extends crate::A { """Cycle.""" }
    "#,
    );
    assert!(
        errors
            .iter()
            .any(|e| e.message.contains("does not implement"))
    );
    assert!(
        errors
            .iter()
            .any(|e| e.message.contains("inheritance cycle"))
    );
}

#[test]
fn resolves_cross_crate_and_inherited_methods() {
    let a = parse("a/crate.sage", r#"crate a { """A.""" depends $b; }
        class Caller { """Call.""" fn run() -> $b::Value { """Run.""" envelope { invokes $b::Child::run(); } } }
    "#).unwrap();
    let b = parse(
        "b/crate.sage",
        r#"crate b { """B.""" }
        type Value """Value.""";
        class Base { """Base.""" fn run() -> crate::Value { """Run.""" envelope {} } }
        class Child extends crate::Base { """Child.""" }
    "#,
    )
    .unwrap();
    assert!(validate(&[a, b]).is_empty());
}

#[test]
fn checks_manifest_modules_and_source_spans() {
    let root = parse("a/crate.sage", "crate a { \"\"\"A.\"\"\" module missing; }").unwrap();
    assert!(validate(&[root])[0].message.contains("missing module"));
    let source = "crate a { \"\"\"unfinished λ";
    let error = parse("a/crate.sage", source).unwrap_err();
    assert_eq!(error.span.end, source.len());
    assert!(source.is_char_boundary(error.span.start));
    assert!(parse("a/crate.sage", "crate a { \"\"\"A.\"\"\" } garbage").is_err());
}

#[test]
fn repository_model_is_consistent() {
    fn collect(path: &std::path::Path, documents: &mut Vec<sage_meta::Document>) {
        let mut entries = std::fs::read_dir(path)
            .unwrap()
            .map(|e| e.unwrap().path())
            .collect::<Vec<_>>();
        entries.sort();
        for path in entries {
            if path.is_dir() {
                collect(&path, documents);
            } else if path.extension().is_some_and(|e| e == "sage") {
                documents.push(
                    parse(
                        &path.to_string_lossy(),
                        &std::fs::read_to_string(&path).unwrap(),
                    )
                    .unwrap(),
                );
            }
        }
    }
    let workspace = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let root = sage_meta::workspace::default_model_root(&workspace).unwrap();
    let mut documents = Vec::new();
    collect(&root, &mut documents);
    assert!(!documents.is_empty());
    let errors = validate(&documents);
    assert!(
        errors.is_empty(),
        "{}",
        errors
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join("\n")
    );
}

#[test]
#[sage::test(meta.language.model.inherited_prohibitions)]
fn inherited_prohibitions_constrain_overrides() {
    let errors = check(
        r#"
        class Base { """Contract."""
            fn run() -> crate::Value { """No bodies.""" envelope { forbid transitive crate::Body::read; } }
        }
        class Child extends crate::Base { """Implementation."""
            override fn run() -> crate::Value { """Bad override.""" envelope { invokes crate::Body::read(); } }
        }
        class Body { """Body.""" fn read() -> crate::Value { """Read.""" envelope {} } }
    "#,
    );
    assert!(
        errors
            .iter()
            .any(|e| e.message.contains("demo::Child::run -> demo::Body::read"))
    );
}

#[test]
#[sage::test(meta.language.types.struct_fields)]
#[sage::test(meta.language.types.enum_variants)]
#[sage::test(meta.language.types.opaque_representation)]
#[sage::test(meta.language.types.declaration_prose)]
#[sage::test(meta.language.types.field_prose)]
#[sage::test(meta.language.types.variant_prose)]
fn parses_structs_enums_and_opaque_generics() {
    use sage_meta::{TypeExprKind, TypeShape, VariantFields};
    let source = r#"crate demo { """Types.""" }
        type Binder<T> { """Bound value; representation intentionally opaque.""" }
        struct Pair<T> {
            """Two related values."""
            """The first value.""" first: T,
            rest: Vec<Option<T>>,
        }
        enum Ty {
            """Semantic types without storage wrappers."""
            Never """No values.""",
            Tuple("""Ordered components.""" Vec<Ty>, bool),
            Ref { """Reference.""" referent: Ty, mutable: bool },
        }
        struct Signature { value: Binder<Pair<Ty>>, pair: (Ty, Ty), unit: () }
    "#;
    let doc = parse("demo/crate.sage", source).unwrap();
    assert!(validate(std::slice::from_ref(&doc)).is_empty());
    let Declaration::Type(binder) = &doc.declarations[0] else {
        panic!()
    };
    assert_eq!(binder.parameters, ["T"]);
    assert_eq!(binder.shape, TypeShape::Opaque);
    assert_eq!(
        binder.description.as_deref(),
        Some("Bound value; representation intentionally opaque.")
    );
    let Declaration::Type(pair) = &doc.declarations[1] else {
        panic!()
    };
    let TypeShape::Struct(fields) = &pair.shape else {
        panic!()
    };
    assert_eq!(pair.description.as_deref(), Some("Two related values."));
    assert_eq!(pair.parameters, ["T"]);
    assert_eq!(
        fields
            .iter()
            .map(|field| field.name.as_str())
            .collect::<Vec<_>>(),
        ["first", "rest"]
    );
    assert!(
        matches!(&fields[0].ty.kind, TypeExprKind::Named { path, arguments }
        if path.root == "T" && arguments.is_empty())
    );
    assert_eq!(fields[0].description.as_deref(), Some("The first value."));
    let Declaration::Type(ty) = &doc.declarations[2] else {
        panic!()
    };
    let TypeShape::Enum(variants) = &ty.shape else {
        panic!()
    };
    assert_eq!(
        ty.description.as_deref(),
        Some("Semantic types without storage wrappers.")
    );
    assert_eq!(
        variants
            .iter()
            .map(|variant| variant.name.as_str())
            .collect::<Vec<_>>(),
        ["Never", "Tuple", "Ref"]
    );
    assert_eq!(variants[0].fields, VariantFields::Unit);
    assert_eq!(variants[0].description.as_deref(), Some("No values."));
    let VariantFields::Tuple(types) = &variants[1].fields else {
        panic!("expected tuple payload")
    };
    assert_eq!(types.len(), 2);
    assert!(
        matches!(&types[0].kind, TypeExprKind::Named { path, arguments }
        if path.root == "Vec" && arguments.len() == 1
        && matches!(&arguments[0].kind, TypeExprKind::Named { path, arguments }
            if path.root == "Ty" && arguments.is_empty()))
    );
    assert!(
        matches!(&types[1].kind, TypeExprKind::Named { path, arguments }
        if path.root == "bool" && arguments.is_empty())
    );
    assert_eq!(
        variants[1].description.as_deref(),
        Some("Ordered components.")
    );
    let VariantFields::Named(fields) = &variants[2].fields else {
        panic!("expected named payload")
    };
    assert_eq!(fields.len(), 2);
    assert_eq!(fields[0].name, "referent");
    assert!(
        matches!(&fields[0].ty.kind, TypeExprKind::Named { path, arguments }
        if path.root == "Ty" && arguments.is_empty())
    );
    assert_eq!(fields[1].name, "mutable");
    assert!(
        matches!(&fields[1].ty.kind, TypeExprKind::Named { path, arguments }
        if path.root == "bool" && arguments.is_empty())
    );
    assert_eq!(variants[2].description.as_deref(), Some("Reference."));
    let Declaration::Type(signature) = &doc.declarations[3] else {
        panic!()
    };
    let TypeShape::Struct(fields) = &signature.shape else {
        panic!()
    };
    assert!(matches!(&fields[1].ty.kind, TypeExprKind::Tuple(types) if types.len() == 2));
    assert_eq!(
        &source[fields[0].ty.span.start..fields[0].ty.span.end],
        "Binder<Pair<Ty>>"
    );
}

#[test]
#[sage::test(meta.language.types.argument_arity)]
#[sage::test(meta.language.types.unique_parameters)]
fn checks_arity_scope_and_duplicate_members_in_value_types() {
    let errors = check(
        r#"
        type Binder<T> { """Bound value.""" }
        struct Bad<T, T> { x: Binder, x: Vec<Missing>, y: T<Value>, z: Option<Value, Value> }
        enum Choices { A, A, B { x: Value, x: Value } }
        struct Other { leaked: T }
    "#,
    );
    for expected in [
        "duplicate type parameter T",
        "duplicate field x",
        "duplicate variant A",
        "unknown type Missing",
        "unknown type T",
        "expects 1 type arguments, found 0",
        "expects 0 type arguments, found 1",
        "expects 1 type arguments, found 2",
    ] {
        assert!(
            errors.iter().any(|e| e.message.contains(expected)),
            "missing {expected}: {errors:?}"
        );
    }
}

#[test]
#[sage::test(meta.language.classes.override_signature)]
fn checks_nested_arguments_in_fields_results_and_overrides() {
    let errors = check(
        r#"
        class Base { """Base."""
            fn value() -> Vec<Value> { """Get.""" envelope {} }
        }
        class Child extends crate::Base { """Child."""
            input broken: Option<Missing>;
            override fn value() -> Vec<Option<Value>> { """Get.""" envelope {} }
            fn missing() -> Option<Unknown> { """Get.""" envelope {} }
        }
    "#,
    );
    assert!(
        errors
            .iter()
            .any(|e| e.message.contains("unknown type Missing"))
    );
    assert!(
        errors
            .iter()
            .any(|e| e.message.contains("unknown type Unknown"))
    );
    assert!(
        errors
            .iter()
            .any(|e| e.message.contains("preserve method kind and result type"))
    );
    assert!(
        check(
            r#"
        class Base { """Base.""" fn value() -> Vec<Value> { """Get.""" envelope {} } }
        class Child extends crate::Base { """Child."""
            override fn value() -> Vec<crate::Value> { """Get.""" envelope {} }
        }
    "#
        )
        .is_empty()
    );
    let errors = check(
        r#"
        class Base { """Base.""" fn value() -> Value { """Get.""" envelope {} } }
        class Child extends crate::Base { """Child."""
            override query value() -> Value { """Get.""" envelope {} }
        }
    "#,
    );
    assert!(errors.iter().any(|error| {
        error
            .message
            .contains("preserve method kind and result type")
    }));
}

#[test]
#[sage::test(meta.language.types.qualified_names)]
fn generic_types_resolve_across_crates_with_dependency_checks() {
    let library = parse(
        "lib/crate.sage",
        r#"crate lib { """Library.""" }
        struct Wrapper<T> { value: T }
    "#,
    )
    .unwrap();
    let consumer = |dependency: &str| {
        parse(
            "app/crate.sage",
            &format!(
                r#"
        crate app {{ """App.""" {dependency} }}
        struct Uses {{ data: $lib::Wrapper<Vec<bool>> }}
    "#
            ),
        )
        .unwrap()
    };
    assert!(validate(&[library.clone(), consumer("depends $lib;")]).is_empty());
    assert!(
        validate(&[library, consumer("")])
            .iter()
            .any(|e| e.message.contains("undeclared dependency $lib"))
    );
}

#[test]
#[sage::test(meta.language.types.opaque_representation)]
#[sage::test(meta.language.types.generic_parameters)]
#[sage::test(meta.language.types.member_commas)]
fn rejects_malformed_value_declarations() {
    for body in [
        "struct Bad { x: Vec<Value }",
        "enum Bad { V(Value Value) }",
        "struct Bad { x: Value y: Value }",
        "type Bad<T> {}",
        "struct Bad<> {}",
    ] {
        assert!(
            parse(
                "demo/crate.sage",
                &format!("crate demo {{ \"\"\"Demo.\"\"\" }} {body}")
            )
            .is_err(),
            "accepted {body}"
        );
    }
}

#[test]
fn correspondence_annotations_live_in_rust_only() {
    assert!(
        parse(
            "demo/crate.sage",
            r#"crate demo { """Demo.""" }
        #[rust(crate::Item)] type Item """Value.""";"#
        )
        .is_err()
    );
}
