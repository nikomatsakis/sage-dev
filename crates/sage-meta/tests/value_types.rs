#![feature(register_tool)]
#![register_tool(sage)]

use sage_meta::{Declaration, Diagnostic, Document, TypeExprKind, TypeShape, VariantFields};

fn parse_body(body: &str) -> Result<Document, Diagnostic> {
    sage_meta::parse(
        "demo/crate.sage",
        &format!(r#"crate demo {{ """Value types.""" }} {body}"#),
    )
}

fn check(body: &str) -> Vec<Diagnostic> {
    sage_meta::validate(&[parse_body(body).unwrap()])
}

#[test]
#[sage::test(meta.language.types.struct_fields)]
#[sage::test(meta.language.types.enum_variants)]
#[sage::test(meta.language.types.declaration_prose)]
fn empty_structs_and_variant_payloads_preserve_their_shapes() {
    let document = parse_body("struct Empty {} enum Shapes { Unit, Tuple(), Named {} }").unwrap();
    assert!(sage_meta::validate(std::slice::from_ref(&document)).is_empty());
    let Declaration::Type(empty) = &document.declarations[0] else {
        panic!("expected a value declaration")
    };
    assert_eq!(empty.description, None);
    assert_eq!(empty.shape, TypeShape::Struct(vec![]));
    let Declaration::Type(shapes) = &document.declarations[1] else {
        panic!("expected a value declaration")
    };
    assert_eq!(shapes.description, None);
    let TypeShape::Enum(variants) = &shapes.shape else {
        panic!("expected an enum")
    };
    assert_eq!(variants[0].fields, VariantFields::Unit);
    assert_eq!(variants[1].fields, VariantFields::Tuple(vec![]));
    assert_eq!(variants[2].fields, VariantFields::Named(vec![]));
}

#[test]
#[sage::test(meta.language.types.generic_parameters)]
fn parameters_are_local_to_struct_enum_and_opaque_declarations() {
    for declaration in [
        "struct Pair<T, U> { first: T, second: U }",
        "enum Pair<T, U> { First(T), Second { value: U } }",
        r#"type Pair<T, U> { """Opaque pair.""" }"#,
    ] {
        let document = parse_body(declaration).unwrap();
        let Declaration::Type(pair) = &document.declarations[0] else {
            panic!("expected a value declaration")
        };
        assert_eq!(pair.parameters, ["T", "U"]);
        assert!(
            check(&format!(
                "{declaration} struct Use {{ pair: Pair<bool, u32> }}"
            ))
            .is_empty()
        );
        let errors = check(&format!("{declaration} struct Leaked {{ value: T }}"));
        assert_eq!(errors.len(), 1, "{errors:?}");
        assert!(errors[0].message.contains("unknown type T"));
    }
}

#[test]
#[sage::test(meta.language.types.opaque_representation)]
fn opaque_types_require_prose_and_have_no_payload() {
    for declaration in [
        r#"type Hidden<T> """Hidden representation.""";"#,
        r#"type Hidden<T> { """Hidden representation.""" }"#,
    ] {
        let document = parse_body(declaration).unwrap();
        let Declaration::Type(hidden) = &document.declarations[0] else {
            panic!("expected a value declaration")
        };
        assert_eq!(hidden.shape, TypeShape::Opaque);
        assert_eq!(
            hidden.description.as_deref(),
            Some("Hidden representation.")
        );
        assert_eq!(hidden.parameters, ["T"]);
    }
    for declaration in [
        "type Hidden;",
        "type Hidden {}",
        r#"type Hidden { """Opaque.""" field: bool }"#,
    ] {
        assert!(parse_body(declaration).is_err(), "accepted {declaration}");
    }
}

#[test]
#[sage::test(meta.language.types.field_prose)]
fn named_variant_fields_keep_their_own_prose() {
    let document = parse_body(
        r#"enum Value {
            Record { """Variant.""" """First field.""" first: bool,
                """Second field.""" second: u32 }
        }"#,
    )
    .unwrap();
    let Declaration::Type(value) = &document.declarations[0] else {
        panic!("expected a value declaration")
    };
    let TypeShape::Enum(variants) = &value.shape else {
        panic!("expected an enum")
    };
    assert_eq!(variants[0].description.as_deref(), Some("Variant."));
    let VariantFields::Named(fields) = &variants[0].fields else {
        panic!("expected named fields")
    };
    assert_eq!(fields[0].description.as_deref(), Some("First field."));
    assert_eq!(fields[1].description.as_deref(), Some("Second field."));
}

#[test]
#[sage::test(meta.language.types.member_commas)]
fn member_lists_require_separators_and_allow_trailing_commas() {
    for trailing in ["", ","] {
        let source = format!(
            "struct Pair {{ a: bool, b: u32{trailing} }}
             enum Choice {{ Unit, Tuple(bool, u32{trailing}),
                 Named {{ a: bool, b: u32{trailing} }}{trailing} }}"
        );
        assert!(check(&source).is_empty(), "rejected {source}");
    }
    for declaration in [
        "struct Pair { a: bool b: u32 }",
        "enum Choice { First Second }",
        "enum Choice { Tuple(bool u32) }",
        "enum Choice { Named { a: bool b: u32 } }",
    ] {
        assert!(parse_body(declaration).is_err(), "accepted {declaration}");
    }
}

#[test]
#[sage::test(meta.language.types.type_expressions)]
fn nested_type_expressions_retain_named_arguments_and_tuple_elements() {
    let document = parse_body("struct Value { data: Vec<(bool, Option<u32>)> }").unwrap();
    assert!(sage_meta::validate(std::slice::from_ref(&document)).is_empty());
    let Declaration::Type(value) = &document.declarations[0] else {
        panic!("expected a value declaration")
    };
    let TypeShape::Struct(fields) = &value.shape else {
        panic!("expected a struct")
    };
    let TypeExprKind::Named { path, arguments } = &fields[0].ty.kind else {
        panic!("expected a named type")
    };
    assert_eq!(path.root, "Vec");
    assert_eq!(arguments.len(), 1);
    let TypeExprKind::Tuple(elements) = &arguments[0].kind else {
        panic!("expected a tuple argument")
    };
    assert_eq!(elements.len(), 2);
    assert!(
        matches!(&elements[0].kind, TypeExprKind::Named { path, arguments }
        if path.root == "bool" && arguments.is_empty())
    );
    let TypeExprKind::Named { path, arguments } = &elements[1].kind else {
        panic!("expected a named tuple element")
    };
    assert_eq!(path.root, "Option");
    assert_eq!(arguments.len(), 1);
    assert!(
        matches!(&arguments[0].kind, TypeExprKind::Named { path, arguments }
        if path.root == "u32" && arguments.is_empty())
    );
}

#[test]
#[sage::test(meta.language.types.name_resolution)]
fn parameters_shadow_local_types_and_local_declarations_shadow_the_prelude() {
    let errors = check(
        r#"type T<U> """A generic local type.""";
        struct Value<T> { parameter: T }
        struct Vec<T, U> { first: T, second: U }
        struct Uses { local: Vec<bool, u32> }
        class String { """A local class shadows the builtin.""" }
        context Option { """A local context shadows the builtin.""" }
        class Base { """Base."""
            fn value() -> (String, Option) { """Get.""" envelope {} }
        }
        class Child extends crate::Base { """Child."""
            override fn value() -> (crate::String, crate::Option) { """Get.""" envelope {} }
        }"#,
    );
    assert!(errors.is_empty(), "{errors:?}");
}

#[test]
#[sage::test(meta.language.types.prelude)]
fn builtin_value_types_need_no_declarations() {
    let errors =
        check("struct Prelude { values: Vec<Option<bool>>, count: u32, size: u64, text: String }");
    assert!(errors.is_empty(), "{errors:?}");
}

#[test]
#[sage::test(meta.language.types.qualified_names)]
fn module_lookup_requires_qualification_for_root_and_sibling_types() {
    let root = sage_meta::parse(
        "demo/crate.sage",
        r#"crate demo { """Root.""" module one; module two; } struct Root {}"#,
    )
    .unwrap();
    let sibling =
        sage_meta::parse("demo/one.sage", "module crate::one; struct Sibling {}").unwrap();
    let module = |body: &str| {
        sage_meta::parse("demo/two.sage", &format!("module crate::two; {body}")).unwrap()
    };
    let documents = [
        root.clone(),
        sibling.clone(),
        module("struct Uses { root: crate::Root, sibling: crate::one::Sibling }"),
    ];
    assert!(sage_meta::validate(&documents).is_empty());
    for (ty, diagnostic) in [
        ("Root", "unknown type Root"),
        ("Sibling", "unknown type Sibling"),
        (
            "one::Sibling",
            "type names must be local names or crate-qualified paths",
        ),
    ] {
        let errors = sage_meta::validate(&[
            root.clone(),
            sibling.clone(),
            module(&format!("struct Uses {{ value: {ty} }}")),
        ]);
        assert_eq!(errors.len(), 1, "{errors:?}");
        assert!(errors[0].message.contains(diagnostic), "{errors:?}");
    }
}

#[test]
#[sage::test(meta.language.types.tuples)]
fn parentheses_always_denote_tuples() {
    let document =
        parse_body("struct Tuples { zero: (), one: (bool), comma: (bool,), two: (bool, u32) }")
            .unwrap();
    let Declaration::Type(tuples) = &document.declarations[0] else {
        panic!("expected a value declaration")
    };
    let TypeShape::Struct(fields) = &tuples.shape else {
        panic!("expected a struct")
    };
    for (field, arity) in fields.iter().zip([0, 1, 1, 2]) {
        assert!(matches!(&field.ty.kind, TypeExprKind::Tuple(elements) if elements.len() == arity));
    }
    let errors = check(
        r#"class Base { """Base.""" fn value() -> bool { """Get.""" envelope {} } }
        class Child extends crate::Base { """Child."""
            override fn value() -> (bool) { """Get.""" envelope {} }
        }"#,
    );
    assert_eq!(errors.len(), 1, "{errors:?}");
    assert!(
        errors[0]
            .message
            .contains("preserve method kind and result type")
    );
}

#[test]
#[sage::test(meta.language.types.argument_arity)]
fn arity_checks_reach_nested_named_types() {
    for ty in [
        "Vec",
        "Option<bool, u32>",
        "bool<u32>",
        "Vec<Option>",
        "(bool, Vec<Option>)",
    ] {
        let errors = check(&format!("struct Bad {{ value: {ty} }}"));
        assert_eq!(errors.len(), 1, "{errors:?}");
        assert!(
            errors[0].message.contains("type arguments, found"),
            "{errors:?}"
        );
    }
}

#[test]
#[sage::test(meta.language.types.unknown_names)]
fn unknown_nested_names_are_rejected_at_each_type_use_site() {
    for declaration in [
        "struct Bad { value: Option<(bool, Missing)> }",
        "enum Bad { Tuple(Vec<Missing>) }",
        "enum Bad { Named { value: (Missing, bool) } }",
        r#"class Bad { """Bad.""" input value: Vec<Missing>; }"#,
        r#"context Bad { """Bad.""" state value: Vec<Missing>; }"#,
        r#"class Bad { """Bad.""" fn value() -> Option<Missing> { """Bad.""" envelope {} } }"#,
    ] {
        let errors = check(declaration);
        assert_eq!(errors.len(), 1, "{errors:?}");
        assert!(
            errors[0].message.contains("unknown type Missing"),
            "{errors:?}"
        );
    }
}

#[test]
#[sage::test(meta.language.types.unique_fields)]
fn field_names_are_unique_only_within_their_own_payload() {
    for declaration in [
        "struct Bad { value: bool, value: u32 }",
        "enum Bad { Named { value: bool, value: u32 } }",
    ] {
        let errors = check(declaration);
        assert_eq!(errors.len(), 1, "{errors:?}");
        assert!(errors[0].message.contains("duplicate field value"));
    }
    assert!(
        check(
            "struct First { value: bool } struct Second { value: u32 }
        enum Choices { First { value: bool }, Second { value: u32 } }"
        )
        .is_empty()
    );
}

#[test]
#[sage::test(meta.language.types.unique_variants)]
fn variant_names_are_unique_only_within_their_own_enum() {
    let errors = check("enum Bad { Duplicate, Duplicate(bool) }");
    assert_eq!(errors.len(), 1, "{errors:?}");
    assert!(errors[0].message.contains("duplicate variant Duplicate"));
    assert!(check("enum First { Same } enum Second { Same(bool) }").is_empty());
}

#[test]
#[sage::test(meta.language.types.recursive_values)]
fn direct_and_mutual_value_recursion_needs_no_storage_indirection() {
    let errors = check(
        "struct Loop { again: Loop }
         enum Tree { Leaf, Pair(Tree, Tree) }
         struct First { next: Second }
         enum Second { Next(First) }",
    );
    assert!(errors.is_empty(), "{errors:?}");
}
