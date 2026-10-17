#![feature(rustc_private)]
#![feature(register_tool)]
#![register_tool(sage)]

use expect_test::expect;
use sage_test_harness::compile_test;

#[test]
#[sage::test(crates.sage_test_harness.compile_tests.fixtures)]
fn source_selection_and_duplicate_fields_are_rejected() {
    let cases: &[(&str, fn())] = &[
        ("missing source or files", || {
            compile_test! {};
        }),
        ("provide exactly one source or files field", || {
            compile_test! { source: "", source: "" };
        }),
        ("provide exactly one source or files field", || {
            compile_test! { source: "", files: { "src/lib.rs": "" } };
        }),
        ("provide exactly one source or files field", || {
            compile_test! { files: { "src/lib.rs": "" }, source: "" };
        }),
        ("provide exactly one source or files field", || {
            compile_test! { files: { "src/lib.rs": "" }, files: { "src/lib.rs": "" } };
        }),
        ("duplicate edition", || {
            compile_test! { source: "", edition: "2024", edition: "2021" };
        }),
        ("duplicate field: observations", || {
            compile_test! { source: "", observations: expect![""], observations: expect![""] };
        }),
        ("duplicate field: diagnostics", || {
            compile_test! { source: "", diagnostics: expect![""], diagnostics: expect![""] };
        }),
        ("duplicate field: queries", || {
            compile_test! { source: "", queries: expect![""], queries: expect![""] };
        }),
    ];
    for (expected, run) in cases {
        let panic = std::panic::catch_unwind(*run).expect_err("invalid fields must fail");
        let message = panic
            .downcast_ref::<String>()
            .map(String::as_str)
            .or_else(|| panic.downcast_ref::<&str>().copied())
            .unwrap();
        assert!(message.contains(expected), "{message}");
    }
}

#[test]
#[sage::test(crates.sage_test_harness.compile_tests.fixtures)]
fn source_expressions_field_order_and_optional_trailing_commas() {
    compile_test! {
        observations: expect![["src/lib.rs: literal `22`\n  type: u16\n"]],
        edition: "2024",
        diagnostics: expect![""],
        source: indoc::indoc! {r#"
            fn value() -> u16 {
                22 //# ty
            }
        "#}
    };
    compile_test! {
        diagnostics: expect![""],
        files: {
            "src/model/detail.rs": "pub fn value() -> u16 { 22 }",
            "src/model.rs": "pub mod detail;",
            "src/lib.rs": r#"mod model;"#
        },
        edition: "2024",
    };
    compile_test! { source: String::from("fn value() {}") };
}

#[test]
#[sage::test(crates.sage_test_harness.compile_tests.fixtures)]
fn unknown_field_and_root_selection_fail_to_compile() {
    let project = tempfile::tempdir().unwrap();
    let harness = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    std::fs::create_dir(project.path().join("src")).unwrap();
    std::fs::write(project.path().join("Cargo.toml"), format!(
        "[package]\nname = \"fixture_grammar_probe\"\nversion = \"0.0.0\"\nedition = \"2024\"\n[workspace]\n[dependencies]\nsage-test-harness = {{ path = {:?} }}\n[patch.crates-io]\nsalsa = {{ path = {:?} }}\n",
        harness, harness.join("../../vendor/salsa")
    )).unwrap();
    std::fs::copy(
        harness.join("../../Cargo.lock"),
        project.path().join("Cargo.lock"),
    )
    .unwrap();
    for field in ["unknown_fixture_field", "root"] {
        std::fs::write(project.path().join("src/lib.rs"), format!(
            "#![feature(rustc_private)]\npub fn probe() {{ sage_test_harness::compile_test! {{ source: \"\", {field}: \"src/other.rs\" }} }}"
        )).unwrap();
        let output = std::process::Command::new("cargo")
            .args(["check", "--offline", "--lib", "--quiet"])
            .current_dir(project.path())
            .env("CARGO_TARGET_DIR", harness.join("../../target"))
            .env("RUSTC", sage_host::metadata::our_rustc())
            .output()
            .unwrap();
        let errors = String::from_utf8_lossy(&output.stderr);
        assert!(!output.status.success(), "unknown field must be rejected");
        assert!(
            errors.contains(&format!("no method named `{field}`")),
            "{errors}"
        );
    }
}
