use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};

static NEXT: AtomicUsize = AtomicUsize::new(0);
struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!(
            "sage-cli-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(root.join("sage-src/demo")).unwrap();
        std::fs::create_dir_all(root.join("src")).unwrap();
        std::fs::create_dir_all(root.join("md")).unwrap();
        std::fs::write(root.join("md/README.md"), "# Fixture\n").unwrap();
        std::fs::write(
            root.join("Cargo.toml"),
            "[package]\nname = \"demo\"\nversion = \"0.0.0\"\nedition = \"2024\"\n[workspace]\n",
        )
        .unwrap();
        std::fs::write(
            root.join("sage-src/demo/crate.sage"),
            "crate demo { \"\"\"Fixture.\"\"\" } type Value { \"\"\"A value.\"\"\" }",
        )
        .unwrap();
        std::fs::write(root.join("src/lib.rs"), "#[sage::item(crate)] mod r#abstract {} #[sage::item(crate::Value)] struct Implementation;").unwrap();
        Self(root)
    }
    fn run(&self, args: &[&str]) -> std::process::Output {
        std::process::Command::new(env!("CARGO_BIN_EXE_sage-meta"))
            .current_dir(&self.0)
            .args(args)
            .output()
            .unwrap()
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.0).unwrap();
    }
}

#[test]
fn correspondence_command_reports_source_and_executable_coverage_separately() {
    let fixture = Fixture::new();
    let output = fixture.run(&["check", "--json"]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["model"]["valid"], true);
    assert_eq!(report["correspondence"]["valid"], true);
    assert_eq!(report["correspondence"]["mapped_items"], 2);
    assert_eq!(report["correspondence"]["executable_declarations"], 0);
    assert_eq!(report["specification"]["valid"], true);
    assert_eq!(report["specification"]["tests_executed"], false);
    assert_eq!(
        report["correspondence"]["runtime_conformance_checked"],
        false
    );
}

#[test]
fn invalid_correspondence_returns_machine_readable_diagnostics_and_fails() {
    let fixture = Fixture::new();
    std::fs::write(fixture.0.join("src/lib.rs"), "#[sage::item(crate)] mod r#abstract {} #[sage::item(crate::Missing)] struct Implementation;").unwrap();
    let output = fixture.run(&["check", "--correspondence", "sage-src", "--json"]);
    assert!(!output.status.success());
    let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["model"]["valid"], true);
    assert_eq!(report["correspondence"]["valid"], false);
    assert_eq!(
        report["correspondence"]["diagnostics"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
    let output = fixture.run(&["check"]);
    assert!(!output.status.success());
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(stderr.contains("src/lib.rs:1:"), "{stderr}");
    assert!(stderr.contains("sage-src/demo/crate.sage:1:"), "{stderr}");
    assert!(
        fixture.run(&["sage-src"]).status.success(),
        "legacy command remains supported"
    );
}

#[test]
fn specification_failures_fail_the_cli_and_library() {
    let fixture = Fixture::new();
    std::fs::write(
        fixture.0.join("md/README.md"),
        "{anchor}`demo.documented`\nA specification paragraph requires a test.\n",
    )
    .unwrap();
    let library = sage_meta::check(&fixture.0);
    assert!(library.ensure_valid().is_err());
    assert!(library.model.valid);
    assert!(library.correspondence.as_ref().unwrap().valid);
    assert!(!library.specification.as_ref().unwrap().valid);
    let output = fixture.run(&["check", "--json"]);
    assert!(!output.status.success());
    let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["specification"]["valid"], false);
    assert_eq!(report["specification"]["anchors"], 1);

    let source = fixture.0.join("src/lib.rs");
    let mut rust = std::fs::read_to_string(&source).unwrap();
    rust.push_str(
        "\n#[test] #[sage::test(demo.documented)] fn verifies_documented_behavior() {}\n",
    );
    std::fs::write(&source, rust).unwrap();
    sage_meta::check(&fixture.0).ensure_valid().unwrap();
    assert!(fixture.run(&["check"]).status.success());

    std::fs::write(fixture.0.join("md/README.md"), "# No anchor\n").unwrap();
    let report = sage_meta::check(&fixture.0);
    assert!(
        report.ensure_valid().is_err(),
        "removing the anchor leaves a dangling test reference"
    );
    assert!(!report.specification.unwrap().diagnostics.is_empty());
}

#[test]
fn check_failures_show_source_excerpts_in_tests_and_cli() {
    let fixture = Fixture::new();
    std::fs::write(
        fixture.0.join("md/README.md"),
        "# Règles\n\n{anchor}`demo.documentedx`\nThis paragraph requires a test.\n",
    )
    .unwrap();
    let source = fixture.0.join("src/lib.rs");
    let mut rust = std::fs::read_to_string(&source).unwrap();
    rust.push_str("\n#[test]\n#[sage::test(demo.documented)]\nfn verifies_rule() {}\n");
    std::fs::write(&source, rust).unwrap();

    let report = sage_meta::check(&fixture.0);
    let error = report.ensure_valid().unwrap_err();
    let rendered = error.to_string();
    // libtest and main's Result handler both use Debug, not Display.
    assert_eq!(format!("{error:?}"), rendered);
    assert!(rendered.starts_with("sage-meta check failed (2 errors)\n"));
    assert!(rendered.contains("md/README.md:3:1"), "{rendered}");
    assert!(rendered.contains("src/lib.rs:3:1"), "{rendered}");
    assert!(rendered.contains("3 | {anchor}`demo.documentedx`"));
    assert!(rendered.contains("3 | #[sage::test(demo.documented)]"));
    assert!(rendered.contains("^^^"));
    for unwanted in ["CheckError(", "\\n", "Validated", "Mapped"] {
        assert!(!rendered.contains(unwanted), "{rendered}");
    }

    let output = fixture.run(&["check"]);
    assert!(!output.status.success());
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(stderr.contains(&rendered), "{stderr}");

    let output = fixture.run(&["check", "--json"]);
    assert!(!output.status.success());
    let json: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert!(json.get("rendered_errors").is_none());
    let diagnostics = json["specification"]["diagnostics"].as_array().unwrap();
    assert_eq!(diagnostics.len(), 2);
    assert_eq!(diagnostics[0]["file"], "README.md");
    assert_eq!(diagnostics[0]["span"]["start"], "# Règles\n\n".len());
    assert_eq!(diagnostics[1]["file"], "src/lib.rs");

    // Reports retain the source used for validation, even after a file changes.
    std::fs::write(source, "").unwrap();
    std::fs::write(fixture.0.join("md/README.md"), "").unwrap();
    assert_eq!(report.ensure_valid().unwrap_err().to_string(), rendered);
}

#[test]
fn missing_documentation_cannot_silently_skip_specification_checks() {
    let fixture = Fixture::new();
    std::fs::remove_dir_all(fixture.0.join("md")).unwrap();
    let report = sage_meta::check(&fixture.0);
    assert!(report.ensure_valid().is_err());
    assert!(report.specification.unwrap().load_error.is_some());
}

#[test]
fn selected_model_in_a_member_uses_cargos_workspace_documentation() {
    let fixture = Fixture::new();
    let member = fixture.0.join("member");
    std::fs::create_dir(&member).unwrap();
    std::fs::rename(fixture.0.join("src"), member.join("src")).unwrap();
    std::fs::rename(fixture.0.join("sage-src"), member.join("sage-src")).unwrap();
    std::fs::write(
        member.join("Cargo.toml"),
        "[package]\nname = \"demo\"\nversion = \"0.0.0\"\nedition = \"2024\"\n",
    )
    .unwrap();
    std::fs::write(
        fixture.0.join("Cargo.toml"),
        "[workspace]\nmembers = [\"member\"]\nresolver = \"3\"\n",
    )
    .unwrap();
    sage_meta::check::check_model(&member.join("sage-src"))
        .ensure_valid()
        .unwrap();
    let output = fixture.run(&["check", "member/sage-src"]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn mdbook_preprocessor_renders_nested_chapters_without_changing_examples() {
    use std::io::Write;
    use std::process::{Command, Stdio};
    for items_key in ["sections", "items"] {
        let input = serde_json::json!([
            {},
            {(items_key): [{"Chapter": {
                "content": "# Parent\n",
                "source_path": "parent.md",
                "sub_items": [{"Chapter": {
                    "content": "{anchor}`demo.rule`\nA rule.\n\n```markdown\n{anchor}`example.only`\n```\n",
                    "source_path": "child.md", "sub_items": []
                }}]
            }}]}
        ]);
        let mut child = Command::new(env!("CARGO_BIN_EXE_sage-meta"))
            .arg("preprocess")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        child
            .stdin
            .take()
            .unwrap()
            .write_all(input.to_string().as_bytes())
            .unwrap();
        let output = child.wait_with_output().unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let book: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
        let content = book[items_key][0]["Chapter"]["sub_items"][0]["Chapter"]["content"]
            .as_str()
            .unwrap();
        assert!(content.contains("id=\"demo.rule\""));
        assert!(content.contains("{anchor}`example.only`"));
        assert!(content.contains("A rule."));
    }
}

#[test]
fn invalid_model_and_parse_failures_produce_json_without_running_correspondence() {
    let fixture = Fixture::new();
    for (source, key) in [
        (
            "crate demo { \"\"\"Fixture.\"\"\" } struct Invalid { field: Missing }",
            "diagnostics",
        ),
        ("this is not a model", "load_error"),
    ] {
        std::fs::write(fixture.0.join("sage-src/demo/crate.sage"), source).unwrap();
        let output = fixture.run(&["check", "--correspondence", "--json"]);
        assert!(!output.status.success());
        let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(report["model"]["valid"], false);
        assert!(!report["model"][key].is_null());
        assert!(report["correspondence"].is_null());
        let output = fixture.run(&["check"]);
        assert!(!output.status.success());
        let stderr = String::from_utf8(output.stderr).unwrap();
        assert!(stderr.contains("sage-src/demo/crate.sage:1:"), "{stderr}");
        assert!(stderr.contains(&format!("1 | {source}")), "{stderr}");
    }
}

#[test]
fn mdbook_preprocessor_supports_rendering_and_link_validation() {
    for renderer in ["html", "linkcheck2"] {
        assert!(
            std::process::Command::new(env!("CARGO_BIN_EXE_sage-meta"))
                .args(["preprocess", "supports", renderer])
                .status()
                .unwrap()
                .success()
        );
    }
}
