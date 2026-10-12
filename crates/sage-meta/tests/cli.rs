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
    let output = fixture.run(&["check", "--json", "--correspondence"]);
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
    assert!(
        fixture.run(&["check"]).status.success(),
        "model-only checking remains independent"
    );
    assert!(
        fixture.run(&["sage-src"]).status.success(),
        "legacy command remains supported"
    );
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
    }
}
