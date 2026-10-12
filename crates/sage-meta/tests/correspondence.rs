use sage_meta::correspondence::{CorrespondenceIndex, LookupError, RustItemKind, RustSource};
use sage_meta::{Document, parse};

fn index(source: &str) -> CorrespondenceIndex {
    CorrespondenceIndex::from_sources(&[RustSource {
        crate_name: "demo".into(),
        target: "lib:demo".into(),
        file: "src/lib.rs".into(),
        source: source.into(),
    }])
    .unwrap()
}
fn model() -> Vec<Document> {
    vec![
        parse(
            "demo/crate.sage",
            r#"crate demo { """Fixture.""" }
        class Root {
            """Test root."""
            query run() -> u32 { """Return a number.""" envelope {} }
        }
        type Value { """Test value.""" }
    "#,
        )
        .unwrap(),
    ]
}
const VALID: &str = r#"#![feature(register_tool)]
#![register_tool(sage)]
#[sage::item(crate)]
mod r#abstract {}
#[sage::item(crate::Root)]
struct Implementation;
impl Implementation {
    #[sage::item(crate::Root::run)]
    fn compute() -> u32 { 1 }
}
#[sage::item(crate::Value)]
struct ActualValue;
"#;

#[test]
fn maps_independently_named_rust_declarations_and_checks_both_directions() {
    let index = index(VALID);
    assert!(
        index.validate(&model()).is_empty(),
        "{:?}",
        index.validate(&model())
    );
    let mapped = index.lookup("src/lib.rs", 9).unwrap();
    assert_eq!(mapped.sage_item, "demo::Root::run");
    assert_eq!(mapped.site.rust_path, "demo::Implementation::compute");
    assert_eq!(mapped.site.kind, RustItemKind::Method);
    assert_eq!(mapped.site.line, 8);
    assert!(matches!(
        index.lookup("another/src/lib.rs", 9),
        Err(LookupError::Missing { .. })
    ));
    assert_eq!(index.entries["demo"][0].rust_path, "demo::abstract");
}

#[test]
fn deletion_dangling_targets_wrong_crates_and_wrong_kinds_fail() {
    for (source, expected) in [
        (
            VALID.replace("#[sage::item(crate::Value)]", ""),
            "has no Rust",
        ),
        (
            VALID.replace("crate::Value", "crate::Missing"),
            "unknown Sage item demo::Missing",
        ),
        (
            VALID.replace("crate::Value", "foreign::Value"),
            "different crate",
        ),
        (
            VALID.replace("struct ActualValue;", "fn ActualValue() {}"),
            "incompatible Rust Function",
        ),
        (VALID.replace("crate::Value", "Value"), "start with crate"),
        (
            VALID.replace("crate::Value", "crate::Value, 42"),
            "expects one crate-qualified",
        ),
    ] {
        let diagnostics = index(&source).validate(&model());
        assert!(
            diagnostics
                .iter()
                .any(|diagnostic| diagnostic.message.contains(expected)),
            "{expected}: {diagnostics:?}"
        );
    }
}

#[test]
fn markers_are_source_coverage_not_executable_coverage() {
    let source = r#"
        #[sage::item(crate)]
        #[sage::item(crate::Root)]
        #[sage::item(crate::Value)]
        #[sage::item(crate::Root::run)]
        mod r#abstract {}
    "#;
    let index = index(source);
    assert!(index.validate(&model()).is_empty());
    assert!(
        index.entries["demo::Root::run"]
            .iter()
            .all(|site| !site.executable)
    );
    assert!(matches!(
        index.lookup("src/lib.rs", 6),
        Err(LookupError::Missing { .. })
    ));
}

#[test]
fn conditional_mappings_cannot_silently_pass_reverse_validation() {
    for attribute in [
        "#[cfg_attr(test, sage::item(foreign::Missing))]",
        "#[cfg_attr(test, cfg_attr(feature = \"nested\", sage::item(crate::Missing)))]",
        "#[cfg_attr(test, path = \"other.rs\")]",
    ] {
        let source = format!("{VALID}\n{attribute}\nmod conditional {{}}");
        let diagnostics = index(&source).validate(&model());
        assert!(
            diagnostics
                .iter()
                .any(|diagnostic| diagnostic.message.contains("cfg_attr are unsupported")),
            "{diagnostics:?}"
        );
    }
    let source = format!("{VALID}\n#[cfg_attr(test, allow(dead_code))]\nmod unrelated {{}}");
    assert!(index(&source).validate(&model()).is_empty());
}

#[test]
fn multiple_cfg_sites_preserve_conditions_without_claiming_active_build_coverage() {
    let index = index(
        r#"#[cfg(feature = "a")]
        mod first {
            #[sage::item(crate::Root::run)]
            fn chosen() {}
        }
        #[cfg(not(feature = "a"))]
        #[sage::item(crate::Root::run)]
        fn other() {}
    "#,
    );
    let sites = &index.entries["demo::Root::run"];
    assert_eq!(sites.len(), 2);
    assert!(sites[0].cfg[0].contains("feature = \"a\""));
    assert!(sites[1].cfg[0].contains("not"));
    assert_eq!(
        index.lookup("src/lib.rs", 4).unwrap().site.rust_path,
        "demo::first::chosen"
    );
}

#[test]
fn shared_line_and_multiple_operation_annotations_are_ambiguous() {
    let index = index(
        "#[sage::item(crate::Root::run)] fn first() {} #[sage::item(crate::Root::run)] fn second() {}",
    );
    assert!(matches!(
        index.lookup("src/lib.rs", 1),
        Err(LookupError::Ambiguous { .. })
    ));
    let index = self::index(
        "#[sage::item(crate::Root::run)] #[sage::item(crate::Other::run)] fn both() {}",
    );
    assert!(
        index
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.message.contains("exactly one"))
    );
    assert!(matches!(
        index.lookup("src/lib.rs", 1),
        Err(LookupError::Ambiguous { .. })
    ));
}

#[test]
fn known_symbol_generator_is_parsed_but_arbitrary_macro_tokens_are_not_anchors() {
    let index = index(
        r#"define_kind_symbols! {
        #[sage::item(crate::Root)]
        pub struct Symbol<'db> { data: SymbolDataPriv<'db> }
        pub enum SymbolData<'db> { .. }
        {}
        #[sage::item(crate::Value)]
        pub enum FnSymbol<'db> { Local(LocalFn<'db>), Ext(Kind::Fn) }
        {}
        pub enum Intrinsic<'db> { Local(LocalIntrinsic<'db>) }
    }
    arbitrary! { #[sage::item(crate::Fake)] struct NotProven; }
    "#,
    );
    assert!(index.entries["demo::Root"][0].macro_input);
    assert!(index.entries["demo::Value"][0].macro_input);
    assert!(!index.entries.contains_key("demo::Fake"));
}

#[test]
fn source_snapshot_mismatch_is_rejected() {
    let index = index(VALID);
    assert!(index.verify_source("src/lib.rs", VALID).is_ok());
    assert!(matches!(
        index.verify_source("src/lib.rs", &VALID.replace("compute", "other")),
        Err(LookupError::SourceMismatch { .. })
    ));
    assert!(matches!(
        index.verify_source("unknown.rs", VALID),
        Err(LookupError::SourceMismatch { .. })
    ));
}

#[test]
fn cargo_owner_covers_both_library_and_binary_and_explicit_nested_module_paths() {
    let fixture = std::env::temp_dir().join(format!(
        "sage-correspondence-targets-{}",
        std::process::id()
    ));
    std::fs::create_dir_all(fixture.join("src/inline")).unwrap();
    std::fs::write(
        fixture.join("Cargo.toml"),
        r#"[package]
name = "fixture-package"
version = "0.0.0"
edition = "2024"
[workspace]
[lib]
name = "custom_lib_name"
[[bin]]
name = "custom-bin-name"
path = "src/main.rs"
"#,
    )
    .unwrap();
    std::fs::write(
        fixture.join("src/lib.rs"),
        r#"#[sage::item(crate)] mod r#abstract {}
        mod inline { #[path = "actual.rs"] mod nested; }
    "#,
    )
    .unwrap();
    std::fs::write(
        fixture.join("src/inline/actual.rs"),
        "#[sage::item(crate::Value)] pub struct Thing;",
    )
    .unwrap();
    std::fs::write(
        fixture.join("src/main.rs"),
        "#[sage::item(crate::Root::run)] fn main() {}",
    )
    .unwrap();
    let index = CorrespondenceIndex::load(&fixture).unwrap();
    assert_eq!(
        index.entries["fixture_package::Value"][0].file,
        "src/inline/actual.rs"
    );
    assert_eq!(
        index.entries["fixture_package::Value"][0].target,
        "lib:custom_lib_name"
    );
    assert_eq!(
        index.entries["fixture_package::Root::run"][0].target,
        "bin:custom-bin-name"
    );
    assert_eq!(
        index
            .lookup(&fixture.join("src/main.rs").to_string_lossy(), 1)
            .unwrap()
            .sage_item,
        "fixture_package::Root::run"
    );
    std::fs::remove_dir_all(fixture).unwrap();
}

#[test]
fn nested_unmapped_functions_and_macro_expansions_do_not_borrow_outer_identity() {
    let source = r#"#[sage::item(crate::Root::run)]
fn outer() {
    if true {
        fn unannotated() {}
        #[sage::item(crate::Leaf::read)]
        fn inner() {}
    }
    generate_query!();
}
"#;
    let index = index(source);
    assert_eq!(
        index.lookup("src/lib.rs", 2).unwrap().sage_item,
        "demo::Root::run"
    );
    assert!(matches!(
        index.lookup("src/lib.rs", 4),
        Err(LookupError::Missing { .. })
    ));
    assert_eq!(
        index.lookup("src/lib.rs", 6).unwrap().sage_item,
        "demo::Leaf::read"
    );
    assert!(matches!(
        index.lookup("src/lib.rs", 8),
        Err(LookupError::Missing { .. })
    ));
    assert!(index.diagnostics.is_empty());
}

#[test]
fn annotations_on_unsupported_nodes_cannot_silently_disappear() {
    let index = index(
        r#"struct Container { #[sage::item(crate::Value)] field: u32 }
        #[sage::itm(crate::Root)] struct Misspelled;
    "#,
    );
    assert_eq!(index.diagnostics.len(), 2);
    assert!(index.diagnostics.iter().all(|diagnostic| {
        diagnostic
            .message
            .contains("unsupported Sage attribute or placement")
    }));
}

#[test]
fn unicode_source_offsets_are_valid_utf8_and_retain_the_annotation() {
    let source = "/* ä🦀 */ #[sage::item(crate::Value)] struct Value;";
    let index = index(source);
    let site = &index.entries["demo::Value"][0];
    assert_eq!(
        &source[site.annotation_span.start..site.annotation_span.end],
        "#[sage::item(crate::Value)]"
    );
}

#[test]
fn generated_declarations_reject_hidden_or_misspelled_correspondences() {
    for (attribute, expected) in [
        (
            "#[cfg_attr(feature = \"alternate\", sage::item(crate::Value))]",
            "conditional Sage annotations",
        ),
        (
            "#[cfg_attr(feature = \"outer\", cfg_attr(feature = \"inner\", sage::itm(crate::Value)))]",
            "conditional Sage annotations",
        ),
        (
            "#[sage::itm(crate::Value)]",
            "unsupported Sage attribute or placement",
        ),
        (
            "#[cfg_attr(feature = \"alternate\", path = \"other.rs\")]",
            "conditional Sage annotations",
        ),
    ] {
        for position in 0..4 {
            let mut attributes = ["", "", "", ""];
            attributes[position] = attribute;
            let source = format!(
                r#"define_kind_symbols! {{
                {} pub struct Symbol<'db> {{ data: SymbolDataPriv<'db> }}
                {} pub enum SymbolData<'db> {{ .. }}
                {{}}
                {} pub enum FnSymbol<'db> {{ Local(LocalFn<'db>), Ext(Kind::Fn) }}
                {{}}
                {} pub enum Intrinsic<'db> {{ Local(LocalIntrinsic<'db>) }}
            }}"#,
                attributes[0], attributes[1], attributes[2], attributes[3]
            );
            let index = index(&source);
            assert_eq!(
                index.diagnostics.len(),
                1,
                "{position}: {attribute}: {:?}",
                index.diagnostics
            );
            let diagnostic = &index.diagnostics[0];
            assert!(
                diagnostic.message.contains(expected),
                "{}",
                diagnostic.message
            );
            assert_eq!(
                &source[diagnostic.span.start..diagnostic.span.end],
                attribute
            );
        }
    }
}

#[test]
fn known_generator_accepts_unconditional_mapping_and_unrelated_cfg_attr() {
    let index = index(
        r#"define_kind_symbols! {
        #[cfg_attr(feature = "debug", derive(Debug))]
        #[sage::item(crate::Root)]
        pub struct Symbol<'db> { data: SymbolDataPriv<'db> }
        pub enum SymbolData<'db> { .. }
        {}
        #[cfg(feature = "functions")]
        #[sage::item(crate::Value)]
        pub enum FnSymbol<'db> { Local(LocalFn<'db>), Ext(Kind::Fn) }
        {}
    }"#,
    );
    assert!(index.diagnostics.is_empty(), "{:?}", index.diagnostics);
    assert!(index.entries["demo::Root"][0].macro_input);
    assert_eq!(
        index.entries["demo::Value"][0].cfg,
        ["#[cfg(feature = \"functions\")]"]
    );
}
