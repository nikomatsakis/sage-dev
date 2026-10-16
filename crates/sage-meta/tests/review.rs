#![feature(register_tool)]
#![register_tool(sage)]

use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};

use sage_meta::review::{self, ReviewInput, Selector, TestStatus, ValidationOutput};

static NEXT: AtomicUsize = AtomicUsize::new(0);
const CLAIM: &str = "{anchor}`demo.rule`\nThe result is four.\n";
const TEST: &str = "#[test]\n#[sage::test(demo.rule)]\nfn verifies_result() {\n    // Check the concrete result.\n    assert_eq!(2 + 2, 4);\n}";

struct Fixture(PathBuf);

impl Fixture {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!(
            "sage-review-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        for path in ["src", "md/design/crates/demo"] {
            std::fs::create_dir_all(root.join(path)).unwrap();
        }
        let fixture = Self(root);
        for (path, content) in [
            (
                "Cargo.toml",
                "[package]\nname = 'demo'\nversion = '0.0.0'\nedition = '2024'\n[workspace]\n",
            ),
            ("book.toml", "[book]\nsrc = 'md'\n"),
            (
                "md/SUMMARY.md",
                "- [Design](design/README.md)\n  - [Crates](design/crates/README.md)\n",
            ),
            ("md/design/crates/README.md", "# Crates\n"),
            (
                "md/design/crates/demo/crate.sage",
                "crate demo { \"\"\"Review fixture.\"\"\" }",
            ),
            ("md/design/README.md", CLAIM),
            (
                "src/lib.rs",
                "#[sage::item(crate)] mod model {}\nmod behavior;\n#[cfg(test)] mod tests;\n",
            ),
            (
                "src/behavior.rs",
                "// {anchor}`demo.rule`\npub fn result() -> u32 { 4 }\n",
            ),
            ("src/tests.rs", TEST),
        ] {
            fixture.write(path, content);
        }
        fixture
    }

    fn write(&self, path: &str, content: &str) {
        let path = self.0.join(path);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, content).unwrap();
    }

    fn start(&self) -> ReviewInput {
        review::start(&self.0, "demo.rule", vec![]).unwrap()
    }

    fn report() -> ValidationOutput {
        ValidationOutput { reviewer: "independent fixture reviewer".into(), report: "The implementation returns four and the test asserts the concrete addition result. Source inspection is separate from test execution.".into(), requested_edits: vec![] }
    }

    fn approve(&self) {
        self.start();
        review::record(&self.0, "demo.rule", &Self::report()).unwrap();
    }

    fn input(&self) -> ReviewInput {
        serde_json::from_slice(
            &std::fs::read(self.0.join("validations/demo.rule/input.json")).unwrap(),
        )
        .unwrap()
    }

    fn summary(&self) -> serde_json::Value {
        serde_json::from_slice(
            &std::fs::read(self.0.join("validations/demo.rule/summary.json")).unwrap(),
        )
        .unwrap()
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
#[sage::test(meta.reviews.validation_layout)]
#[sage::test(meta.reviews.summary_format)]
#[sage::test(meta.reviews.preparation)]
#[sage::test(meta.reviews.record_required)]
#[sage::test(meta.reviews.recorded_reads)]
#[sage::test(meta.reviews.recording)]
fn start_invalidates_previous_success_and_record_is_required() {
    let fixture = Fixture::new();
    assert!(!sage_meta::check(&fixture.0).is_valid());
    fixture.approve();
    assert!(sage_meta::check(&fixture.0).is_valid());
    let input = fixture.start();
    assert_eq!(fixture.input(), input);
    let summary = fixture.summary();
    assert_eq!(summary["anchor"], "demo.rule");
    assert!(summary["started_at_unix_seconds"].as_u64().unwrap() > 0);
    assert!(summary["recorded_at_unix_seconds"].is_null());
    assert_eq!(summary["schema_version"], review::SCHEMA_VERSION);
    assert_eq!(summary["policy_version"], review::POLICY_VERSION);
    assert!(summary.get("input_sha256").is_none());
    assert!(!fixture.0.join("validations/demo.rule/output.json").exists());
    assert!(!sage_meta::check(&fixture.0).is_valid());
    let saved = review::load_saved(&fixture.0.join("validations")).unwrap();
    assert_eq!(saved.reviews.len(), 1);
    assert!(saved.reviews[0].output.is_none());
    fixture.write("helper.txt", "context\n");
    review::capture_read(&fixture.0, "demo.rule", "helper.txt", Selector::WholeFile).unwrap();
    assert!(!sage_meta::check(&fixture.0).is_valid());
    let bytes = std::fs::read(fixture.0.join("validations/demo.rule/input.json")).unwrap();
    review::record(&fixture.0, "demo.rule", &Fixture::report()).unwrap();
    assert_eq!(
        bytes,
        std::fs::read(fixture.0.join("validations/demo.rule/input.json")).unwrap()
    );
    assert!(
        fixture.summary()["recorded_at_unix_seconds"]
            .as_u64()
            .unwrap()
            >= summary["started_at_unix_seconds"].as_u64().unwrap()
    );
    assert!(sage_meta::check(&fixture.0).is_valid());
    assert!(review::record(&fixture.0, "demo.rule", &Fixture::report()).is_err());
    assert!(
        review::capture_read(&fixture.0, "demo.rule", "helper.txt", Selector::WholeFile).is_err()
    );
}

#[test]
#[sage::test(meta.reviews.input_format)]
#[sage::test(meta.reviews.input_fragments)]
#[sage::test(meta.reviews.fragment_format)]
#[sage::test(meta.reviews.review_instructions)]
#[sage::test(meta.reviews.review_scope)]
#[sage::test(meta.reviews.source_representation)]
#[sage::test(meta.reviews.preparation)]
fn initial_context_preserves_instructions_and_exact_uniform_fragments() {
    let fixture = Fixture::new();
    let instructions = vec![
        "Check all scenarios.\nKeep this newline.".into(),
        "Account for TODO rationale.".into(),
    ];
    let input = review::start(&fixture.0, "demo.rule", instructions.clone()).unwrap();
    assert_eq!(input.coordinator_instructions, instructions);
    assert_eq!(input.prompt, review::PROMPT);
    assert_eq!(input, fixture.input());
    let claim = input
        .fragments
        .iter()
        .find(|f| matches!(&f.selector, Selector::Claim { anchor_id } if anchor_id == "demo.rule"))
        .unwrap();
    assert_eq!(claim.path, "md/design/README.md");
    assert_eq!(claim.contents.join("\n"), "The result is four.");
    let implementation = input
        .fragments
        .iter()
        .find(|f| f.path == "src/behavior.rs" && matches!(f.selector, Selector::WholeFile))
        .unwrap();
    assert_eq!(
        implementation.contents.join("\n"),
        std::fs::read_to_string(fixture.0.join("src/behavior.rs")).unwrap()
    );
    let test = input
        .fragments
        .iter()
        .find(|f| matches!(f.selector, Selector::Test { .. }))
        .unwrap();
    assert_eq!(test.path, "src/tests.rs");
    assert_eq!(test.contents.join("\n"), TEST);
    assert!(
        !input
            .fragments
            .iter()
            .any(|f| f.path == "src/tests.rs" && matches!(f.selector, Selector::WholeFile))
    );
    assert_eq!(input.implementations.len(), 1);
    assert_eq!(input.implementations[0].file, "src/behavior.rs");
    assert_eq!(input.implementations[0].status, TestStatus::Ordinary);
    assert_eq!(input.tests.len(), 1);
}

#[test]
#[sage::test(meta.reviews.claim_text)]
#[sage::test(meta.reviews.selector_claim)]
#[sage::test(meta.reviews.staleness)]
#[sage::test(meta.reviews.source_representation)]
fn complete_claim_blocks_are_selected_but_unrelated_movement_is_not_stale() {
    let fixture = Fixture::new();
    let body = "First **paragraph**.\n\nSecond paragraph.\n\n- list item\n\n| A | B |\n|---|---|\n| 1 | 2 |\n\n```rust\nlet x = 4;\n```";
    let claim =
        format!("# Heading\n\n{{anchor}}`demo.rule`\n{body}\n\n<!-- end anchor -->\nOutside\n");
    fixture.write("md/design/README.md", &claim);
    let input = fixture.start();
    let fragment = input
        .fragments
        .iter()
        .find(|f| matches!(f.selector, Selector::Claim { .. }))
        .unwrap();
    assert_eq!(fragment.contents.join("\n"), body);
    review::record(&fixture.0, "demo.rule", &Fixture::report()).unwrap();
    fixture.write(
        "md/design/README.md",
        &format!("\n\n{}", claim.replace("Outside", "Changed outside")),
    );
    assert!(sage_meta::check(&fixture.0).is_valid());
    for text in ["Second paragraph", "list item", "| 1 | 2 |", "let x = 4"] {
        fixture.write("md/design/README.md", &claim.replace(text, "changed"));
        assert!(!sage_meta::check(&fixture.0).is_valid(), "{text}");
    }
}

#[test]
#[sage::test(meta.reviews.test_source)]
#[sage::test(meta.reviews.selector_test)]
#[sage::test(meta.reviews.staleness)]
#[sage::test(meta.reviews.fragment_associations)]
#[sage::test(meta.reviews.input_format)]
#[sage::test(meta.reviews.fragment_freshness)]
fn test_selector_preserves_metadata_and_ignores_unrelated_line_movement() {
    let fixture = Fixture::new();
    fixture.write(
        "src/tests.rs",
        &format!("#[cfg(unix)]\nmod nested {{\n{TEST}\n}}\n"),
    );
    let input = fixture.start();
    let test = &input.tests[0];
    assert_eq!(test.crate_name, "demo");
    assert_eq!(test.target, "lib:demo");
    assert_eq!(test.rust_path, "demo::tests::nested::verifies_result");
    assert_eq!(test.file, "src/tests.rs");
    assert!(test.cfg.iter().any(|cfg| cfg.contains("unix")));
    assert!(test.cfg.iter().any(|cfg| cfg.contains("test")));
    assert_eq!(test.status, TestStatus::Ordinary);
    let selector = Selector::Test {
        crate_name: test.crate_name.clone(),
        target: test.target.clone(),
        rust_path: test.rust_path.clone(),
    };
    assert_eq!(
        review::capture_read(&fixture.0, "demo.rule", "src/tests.rs", selector).unwrap(),
        TEST
    );
    review::record(&fixture.0, "demo.rule", &Fixture::report()).unwrap();
    fixture.write(
        "src/tests.rs",
        &format!("// Unrelated\n\n#[cfg(unix)]\nmod nested {{\n{TEST}\n}}\n"),
    );
    assert!(sage_meta::check(&fixture.0).is_valid());
    fixture.write(
        "src/tests.rs",
        &format!("#[cfg(windows)]\nmod nested {{\n{TEST}\n}}\n"),
    );
    assert!(!sage_meta::check(&fixture.0).is_valid());
}

#[test]
#[sage::test(meta.reviews.complete_test_set)]
#[sage::test(meta.reviews.fragment_associations)]
#[sage::test(meta.reviews.input_format)]
#[sage::test(meta.reviews.review_diagnostics)]
fn added_or_removed_associations_invalidate_unchanged_saved_fragments() {
    let fixture = Fixture::new();
    fixture.approve();
    let original = fixture.input();
    let second = "#[test] #[sage::test(demo.rule)] fn second() { assert_eq!(1 + 3, 4); }";
    fixture.write("src/tests.rs", &format!("{TEST}\n{second}"));
    let error = sage_meta::check(&fixture.0)
        .ensure_valid()
        .unwrap_err()
        .to_string();
    assert!(error.contains("demo.rule"), "{error}");
    assert!(error.contains("test associations"), "{error}");
    assert!(error.contains("sage-review-checklist/SKILL.md"), "{error}");
    let current = fixture.start();
    assert_eq!(current.tests.len(), 2);
    assert_eq!(current.tests, fixture.start().tests);
    assert_eq!(
        original.tests[0],
        *current
            .tests
            .iter()
            .find(|t| t.rust_path.ends_with("verifies_result"))
            .unwrap()
    );
    review::record(&fixture.0, "demo.rule", &Fixture::report()).unwrap();
    fixture.write("src/tests.rs", TEST);
    let error = sage_meta::check(&fixture.0)
        .ensure_valid()
        .unwrap_err()
        .to_string();
    assert!(error.contains("demo.rule"), "{error}");
    assert!(error.contains("test associations"), "{error}");
    assert!(error.contains("sage-review-checklist/SKILL.md"), "{error}");
    fixture.approve();
    fixture.write("src/lib.rs", "#[sage::item(crate)] mod model {}\nmod behavior;\n#[cfg(test)] mod tests;\n// {anchor}`demo.rule`\nfn another_implementation() {}\n");
    let error = sage_meta::check(&fixture.0)
        .ensure_valid()
        .unwrap_err()
        .to_string();
    assert!(error.contains("demo.rule"), "{error}");
    assert!(error.contains("implementation associations"), "{error}");
    assert!(error.contains("sage-review-checklist/SKILL.md"), "{error}");
    assert_eq!(fixture.start().implementations.len(), 2);
}

#[test]
#[sage::test(meta.reviews.selector_whole_file)]
#[sage::test(meta.reviews.selector_line_range)]
#[sage::test(meta.reviews.recorded_reads)]
#[sage::test(meta.reviews.fragment_freshness)]
#[sage::test(meta.reviews.input_format)]
#[sage::test(meta.reviews.input_fragments)]
#[sage::test(meta.reviews.source_representation)]
fn whole_files_and_ranges_preserve_exact_line_endings_and_detect_selected_edits() {
    let fixture = Fixture::new();
    fixture.start();
    fixture.write("helper.txt", "first\r\n\r\nthird\nlast");
    assert_eq!(
        review::capture_read(
            &fixture.0,
            "demo.rule",
            "helper.txt",
            Selector::LineRange { start: 1, end: 2 }
        )
        .unwrap(),
        "first\r\n\r\n"
    );
    assert_eq!(
        review::capture_read(
            &fixture.0,
            "demo.rule",
            "helper.txt",
            Selector::LineRange { start: 4, end: 4 }
        )
        .unwrap(),
        "last"
    );
    for (start, end) in [(0, 1), (3, 2), (1, 5), (5, 5)] {
        assert!(
            review::capture_read(
                &fixture.0,
                "demo.rule",
                "helper.txt",
                Selector::LineRange { start, end }
            )
            .is_err()
        );
    }
    review::record(&fixture.0, "demo.rule", &Fixture::report()).unwrap();
    fixture.write("helper.txt", "first\r\n\r\nUNSELECTED\nlast");
    assert!(sage_meta::check(&fixture.0).is_valid());
    fixture.write("helper.txt", "changed\r\n\r\nthird\nlast");
    assert!(!sage_meta::check(&fixture.0).is_valid());
    fixture.start();
    for _ in 0..2 {
        assert_eq!(
            review::capture_read(&fixture.0, "demo.rule", "helper.txt", Selector::WholeFile)
                .unwrap(),
            "changed\r\n\r\nthird\nlast"
        );
    }
    let input = fixture.input();
    let tail = &input.fragments[input.fragments.len() - 2..];
    assert_eq!(tail[0], tail[1]);
    assert_eq!(tail[0].contents.join("\n"), "changed\r\n\r\nthird\nlast");
    review::record(&fixture.0, "demo.rule", &Fixture::report()).unwrap();
    fixture.write("helper.txt", "changed\r\n\r\nthird\nlast\n");
    assert!(!sage_meta::check(&fixture.0).is_valid());
}

#[test]
#[sage::test(meta.reviews.recording)]
#[sage::test(meta.reviews.output_format)]
#[sage::test(meta.reviews.decisions)]
#[sage::test(meta.reviews.input_fragments)]
#[sage::test(meta.reviews.fragment_freshness)]
#[sage::test(meta.reviews.record_required)]
fn invalid_reports_and_stale_inputs_never_complete_an_active_review() {
    let fixture = Fixture::new();
    assert!(review::record(&fixture.0, "demo.rule", &Fixture::report()).is_err());
    fixture.start();
    for field in ["reviewer", "report", "edit"] {
        let mut output = Fixture::report();
        match field {
            "reviewer" => output.reviewer = "  ".into(),
            "report" => output.report.clear(),
            _ => output.requested_edits = vec!["\n".into()],
        }
        assert!(review::record(&fixture.0, "demo.rule", &output).is_err());
        assert!(!fixture.0.join("validations/demo.rule/output.json").exists());
    }
    let before = std::fs::read(fixture.0.join("validations/demo.rule/input.json")).unwrap();
    fixture.write(
        "src/behavior.rs",
        "// {anchor}`demo.rule`\npub fn result() -> u32 { 5 }\n",
    );
    assert!(review::record(&fixture.0, "demo.rule", &Fixture::report()).is_err());
    assert_eq!(
        before,
        std::fs::read(fixture.0.join("validations/demo.rule/input.json")).unwrap()
    );
    assert!(!sage_meta::check(&fixture.0).is_valid());
    fixture.start();
    let mut output = Fixture::report();
    output.requested_edits =
        vec!["Return four and add an assertion against the implementation.".into()];
    review::record(&fixture.0, "demo.rule", &output).unwrap();
    assert_eq!(
        review::load_saved(&fixture.0.join("validations"))
            .unwrap()
            .reviews[0]
            .output
            .as_ref(),
        Some(&output)
    );
    assert!(!sage_meta::check(&fixture.0).is_valid());
    let json: serde_json::Value = serde_json::from_slice(
        &std::fs::read(fixture.0.join("validations/demo.rule/output.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(json.as_object().unwrap().len(), 3);
    assert!(json.get("decision").is_none());
    assert!(json.get("outcome").is_none());
}

#[test]
#[sage::test(meta.reviews.decisions)]
#[sage::test(meta.reviews.review_scope)]
#[sage::test(meta.reviews.input_format)]
fn accepted_placeholder_reports_preserve_todo_and_na_evidence() {
    for (module, expected) in [
        ("todo", TestStatus::Todo),
        ("not_applicable", TestStatus::NotApplicable),
    ] {
        let fixture = Fixture::new();
        fixture.write("src/behavior.rs", &format!("mod {module} {{\n// {{anchor}}`demo.rule`\n// Deferred until the concrete backend exists; editorial for N/A.\nfn placeholder() {{}}\n}}"));
        fixture.write("src/tests.rs", &format!("mod {module} {{\n// Deferred until the concrete backend exists; editorial for N/A.\n#[test] #[sage::test(demo.rule)] fn placeholder() {{}}\n}}"));
        let input = fixture.start();
        assert_eq!(input.tests[0].status, expected);
        assert_eq!(input.implementations[0].status, expected);
        assert!(
            input.implementations[0]
                .explanation
                .as_ref()
                .unwrap()
                .contains("backend")
        );
        let mut output = Fixture::report();
        output.report = "The explicit placeholder explains why concrete evidence is unavailable and names its prerequisite. This accepts the stated gap, not implemented behavior.".into();
        review::record(&fixture.0, "demo.rule", &output).unwrap();
        let check = sage_meta::check(&fixture.0);
        assert!(check.is_valid());
        let stage = check.review.unwrap();
        assert_eq!(stage.approved, 0);
        assert_eq!(
            (stage.todo, stage.not_applicable),
            if module == "todo" { (1, 0) } else { (0, 1) }
        );
    }
}

#[test]
#[sage::test(meta.reviews.preparation)]
#[sage::test(meta.reviews.recorded_reads)]
#[sage::test(meta.reviews.gate)]
fn invalid_ids_and_paths_are_rejected_without_destroying_existing_reports() {
    let fixture = Fixture::new();
    fixture.approve();
    let output = std::fs::read(fixture.0.join("validations/demo.rule/output.json")).unwrap();
    for anchor in [
        "../escape",
        "/absolute",
        "demo/rule",
        "demo..rule",
        "demo.missing",
    ] {
        assert!(review::start(&fixture.0, anchor, vec![]).is_err());
        assert_eq!(
            output,
            std::fs::read(fixture.0.join("validations/demo.rule/output.json")).unwrap()
        );
    }
    assert!(
        review::capture_read(
            &fixture.0,
            "demo.missing",
            "src/tests.rs",
            Selector::WholeFile
        )
        .is_err()
    );
    fixture.start();
    let before = fixture.input();
    for path in ["../outside", "/etc/passwd", "missing.txt"] {
        assert!(review::capture_read(&fixture.0, "demo.rule", path, Selector::WholeFile).is_err());
        assert_eq!(fixture.input(), before);
    }
    assert!(
        review::capture_read(
            &fixture.0,
            "demo.rule",
            "src/tests.rs",
            Selector::Claim {
                anchor_id: "demo.rule".into()
            }
        )
        .is_err()
    );
    assert!(
        review::capture_read(
            &fixture.0,
            "demo.rule",
            "src/tests.rs",
            Selector::Test {
                crate_name: "demo".into(),
                target: "test:missing".into(),
                rust_path: "demo::tests::verifies_result".into()
            }
        )
        .is_err()
    );
    assert_eq!(fixture.input(), before);
}

#[test]
#[sage::test(meta.reviews.gate)]
#[sage::test(meta.reviews.review_diagnostics)]
#[sage::test(meta.reviews.record_required)]
fn diagnostics_identify_missing_unfinished_stale_and_requested_edit_reviews() {
    let fixture = Fixture::new();
    for state in ["missing", "unfinished", "stale", "requested edits"] {
        match state {
            "unfinished" => {
                fixture.start();
            }
            "stale" => {
                fixture.approve();
                fixture.write(
                    "src/behavior.rs",
                    "// {anchor}`demo.rule`\npub fn result() -> u32 { 5 }\n",
                );
            }
            "requested edits" => {
                fixture.start();
                let mut output = Fixture::report();
                output
                    .requested_edits
                    .push("Fix the implementation.".into());
                review::record(&fixture.0, "demo.rule", &output).unwrap();
            }
            _ => {}
        }
        let output = fixture.run(&["check"]);
        assert!(!output.status.success(), "{state}");
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(stderr.contains("demo.rule"), "{stderr}");
        assert!(
            stderr.contains("sage-review-checklist/SKILL.md"),
            "{stderr}"
        );
        if matches!(state, "missing" | "unfinished") {
            assert!(stderr.contains(state), "{stderr}");
        }
        if state == "stale" {
            assert!(stderr.contains("src/behavior.rs"), "{stderr}");
        }
        if state == "requested edits" {
            assert!(stderr.contains("edit"), "{stderr}");
        }
    }
}

#[test]
#[sage::test(meta.reviews.gate)]
#[sage::test(meta.reviews.summary_format)]
#[sage::test(meta.reviews.review_diagnostics)]
#[sage::test(meta.reviews.staleness)]
fn unsupported_versions_and_mismatched_identity_cannot_pass() {
    for field in ["schema_version", "policy_version", "anchor"] {
        let fixture = Fixture::new();
        fixture.approve();
        let mut summary = fixture.summary();
        summary[field] = if field == "anchor" {
            "demo.other".into()
        } else {
            999.into()
        };
        fixture.write(
            "validations/demo.rule/summary.json",
            &serde_json::to_string(&summary).unwrap(),
        );
        let report = sage_meta::check(&fixture.0);
        let error = report.ensure_valid().unwrap_err().to_string();
        assert!(error.contains("demo.rule"), "{error}");
        assert!(error.contains("sage-review-checklist/SKILL.md"), "{error}");
        let reason = if field == "anchor" {
            "does not match"
        } else {
            "unsupported schema/policy"
        };
        assert!(error.contains(reason), "{error}");
    }
}

#[test]
#[sage::test(meta.reviews.gate)]
#[sage::test(meta.reviews.review_diagnostics)]
fn retired_records_fail_even_when_every_remaining_anchor_passes() {
    let fixture = Fixture::new();
    fixture.approve();
    fixture.write("md/design/README.md", "# Retired\n");
    fixture.write("src/behavior.rs", "pub fn result() -> u32 { 4 }\n");
    fixture.write("src/tests.rs", "");
    let output = fixture.run(&["check"]);
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("demo.rule"), "{stderr}");
    assert!(stderr.contains("remove"), "{stderr}");
    assert!(fixture.0.join("validations/demo.rule/output.json").exists());
    std::fs::remove_dir_all(fixture.0.join("validations/demo.rule")).unwrap();
    assert!(sage_meta::check(&fixture.0).is_valid());
}

#[test]
#[sage::test(meta.reviews.preparation)]
#[sage::test(meta.reviews.recording)]
#[sage::test(meta.reviews.review_instructions)]
fn cli_start_and_record_roundtrip_exact_saved_input() {
    let fixture = Fixture::new();
    fixture.write(
        "instructions.txt",
        "Inspect the edge cases.\nPreserve these instructions.",
    );
    let output = fixture.run(&[
        "review",
        "start",
        "demo.rule",
        "--instructions",
        "instructions.txt",
    ]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let input: ReviewInput = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(input, fixture.input());
    assert_eq!(
        input.coordinator_instructions,
        ["Inspect the edge cases.\nPreserve these instructions."]
    );
    fixture.write(
        "report.json",
        &serde_json::to_string(&Fixture::report()).unwrap(),
    );
    let output = fixture.run(&["review", "record", "demo.rule", "report.json"]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(fixture.run(&["check"]).status.success());
    assert!(
        !fixture
            .run(&["review", "record", "demo.rule"])
            .status
            .success()
    );
}

#[test]
#[sage::test(meta.reviews.review_delegation)]
fn review_skill_assigns_one_owner_and_requires_start_read_record() {
    let skill = include_str!("../../../.agents/skills/sage-review-checklist/SKILL.md");
    assert!(skill.contains("one") && skill.contains("subagent") && skill.contains("anchor"));
    assert!(skill.contains("cargo meta review start"));
    assert!(skill.contains("cargo meta review read"));
    assert!(skill.contains("cargo meta review record"));
    assert!(skill.contains("instructions"));
    assert!(skill.contains("TODO") && skill.contains("N/A"));
}

#[test]
#[sage::test(meta.reviews.selector_test)]
#[sage::test(meta.reviews.recorded_reads)]
fn test_selectors_disambiguate_identical_paths_by_cargo_target() {
    let fixture = Fixture::new();
    // The same physical source is compiled as two independently named targets.
    fixture.write("Cargo.toml", "[package]\nname = 'demo'\nversion = '0.0.0'\nedition = '2024'\n[workspace]\n[[test]]\nname = 'first'\npath = 'tests/shared.rs'\n[[test]]\nname = 'second'\npath = 'tests/shared.rs'\n");
    fixture.write(
        "tests/shared.rs",
        "#[test] #[sage::test(demo.rule)] fn shared() { assert_eq!(4, 4); }",
    );
    let input = fixture.start();
    let tests: Vec<_> = input
        .tests
        .iter()
        .filter(|test| test.file == "tests/shared.rs")
        .collect();
    assert_eq!(tests.len(), 2);
    assert_ne!(tests[0].target, tests[1].target);
    for test in tests {
        let result = review::capture_read(
            &fixture.0,
            "demo.rule",
            &test.file,
            Selector::Test {
                crate_name: test.crate_name.clone(),
                target: test.target.clone(),
                rust_path: test.rust_path.clone(),
            },
        )
        .unwrap();
        assert_eq!(
            result,
            "#[test] #[sage::test(demo.rule)] fn shared() { assert_eq!(4, 4); }"
        );
    }
    assert!(
        review::capture_read(
            &fixture.0,
            "demo.rule",
            "tests/shared.rs",
            Selector::Test {
                crate_name: "other".into(),
                target: "test:first".into(),
                rust_path: "demo::shared".into()
            }
        )
        .is_err()
    );
}

#[test]
#[sage::test(meta.reviews.preparation)]
#[sage::test(meta.reviews.recording)]
#[sage::test(meta.reviews.record_required)]
fn restarting_one_anchor_leaves_other_anchor_record_unchanged() {
    let fixture = Fixture::new();
    fixture.write(
        "md/design/README.md",
        &format!("{CLAIM}\n{{anchor}}`demo.other`\nAnother claim.\n"),
    );
    fixture.write("src/behavior.rs", "// {anchor}`demo.rule`\npub fn result() -> u32 { 4 }\n// {anchor}`demo.other`\npub fn other() -> u32 { 4 }\n");
    fixture.write(
        "src/tests.rs",
        &format!("{TEST}\n#[test] #[sage::test(demo.other)] fn other() {{ assert_eq!(4, 4); }}"),
    );
    fixture.approve();
    review::start(&fixture.0, "demo.other", vec![]).unwrap();
    review::record(&fixture.0, "demo.other", &Fixture::report()).unwrap();
    assert!(sage_meta::check(&fixture.0).is_valid());
    let before: Vec<_> = ["summary.json", "input.json", "output.json"]
        .map(|file| std::fs::read(fixture.0.join("validations/demo.other").join(file)).unwrap())
        .into();
    fixture.start();
    assert!(!sage_meta::check(&fixture.0).is_valid());
    review::record(&fixture.0, "demo.rule", &Fixture::report()).unwrap();
    for (file, bytes) in ["summary.json", "input.json", "output.json"]
        .into_iter()
        .zip(before)
    {
        assert_eq!(
            bytes,
            std::fs::read(fixture.0.join("validations/demo.other").join(file)).unwrap()
        );
    }
    assert!(sage_meta::check(&fixture.0).is_valid());
}

#[test]
#[sage::test(meta.reviews.recording)]
#[sage::test(meta.reviews.record_required)]
#[sage::test(meta.reviews.gate)]
fn incomplete_output_publication_never_satisfies_the_gate() {
    let fixture = Fixture::new();
    fixture.start();
    // An output written before the summary records completion is incomplete.
    fixture.write(
        "validations/demo.rule/output.json",
        &serde_json::to_string(&Fixture::report()).unwrap(),
    );
    assert!(!sage_meta::check(&fixture.0).is_valid());
    fixture.approve();
    std::fs::remove_file(fixture.0.join("validations/demo.rule/output.json")).unwrap();
    assert!(!sage_meta::check(&fixture.0).is_valid());
    fixture.start();
    fixture.write("validations/demo.rule/output.json", "{bad json");
    assert!(!sage_meta::check(&fixture.0).is_valid());
}

#[cfg(unix)]
#[test]
#[sage::test(meta.reviews.recorded_reads)]
fn recorded_reads_reject_symlink_escape_without_capturing_context() {
    let fixture = Fixture::new();
    fixture.start();
    let before = fixture.input();
    std::os::unix::fs::symlink("/etc/passwd", fixture.0.join("escape")).unwrap();
    assert!(review::capture_read(&fixture.0, "demo.rule", "escape", Selector::WholeFile).is_err());
    assert_eq!(before, fixture.input());
}

#[test]
#[sage::test(meta.reviews.selector_claim)]
#[sage::test(meta.reviews.fragment_freshness)]
#[sage::test(meta.reviews.recorded_reads)]
fn declared_claim_read_tracks_selected_text_without_capturing_the_document() {
    let fixture = Fixture::new();
    fixture.start();
    let text = review::capture_read(
        &fixture.0,
        "demo.rule",
        "md/design/README.md",
        Selector::Claim {
            anchor_id: "demo.rule".into(),
        },
    )
    .unwrap();
    assert_eq!(text, "The result is four.");
    let input = fixture.input();
    assert_eq!(input.fragments.last().unwrap().contents.join("\n"), text);
    review::record(&fixture.0, "demo.rule", &Fixture::report()).unwrap();
    fixture.write(
        "md/design/README.md",
        &format!("# An unrelated heading\n\n{CLAIM}"),
    );
    assert!(sage_meta::check(&fixture.0).is_valid());
    fixture.write("md/design/README.md", &CLAIM.replace("four", "five"));
    assert!(!sage_meta::check(&fixture.0).is_valid());
}

#[test]
#[sage::test(meta.reviews.selector_test)]
#[sage::test(meta.reviews.fragment_freshness)]
fn ambiguous_initial_test_selector_cannot_be_recorded_as_complete_context() {
    let fixture = Fixture::new();
    fixture.write(
        "src/tests.rs",
        &format!(
            "#[cfg(unix)]\n{TEST}\n#[cfg(windows)]\n{}",
            TEST.replace("2 + 2", "1 + 3")
        ),
    );
    // One target/path identity resolves to two distinct annotated function spans.
    // Initial context must reject the ambiguity just as an explicit read does.
    match review::start(&fixture.0, "demo.rule", vec![]) {
        Err(_) => {}
        Ok(input) => {
            assert_eq!(input.tests.len(), 2);
            assert!(
                review::record(&fixture.0, "demo.rule", &Fixture::report()).is_err(),
                "recording must reject an ambiguous initial selector, not silently keep one function body"
            );
        }
    }
    assert!(!sage_meta::check(&fixture.0).is_valid());
}

#[test]
#[sage::test(meta.reviews.selector_test)]
#[sage::test(meta.reviews.fragment_freshness)]
fn ambiguous_test_identity_across_anchors_is_rejected() {
    let fixture = Fixture::new();
    fixture.write(
        "md/design/README.md",
        &format!("{CLAIM}\n{{anchor}}`demo.other`\nAnother claim.\n"),
    );
    fixture.write(
        "src/behavior.rs",
        "// {anchor}`demo.rule`\nfn result() {}\n// {anchor}`demo.other`\nfn other() {}\n",
    );
    fixture.write(
        "src/tests.rs",
        &format!(
            "#[cfg(unix)]\n{TEST}\n#[cfg(windows)]\n{}",
            TEST.replace("demo.rule", "demo.other")
        ),
    );
    assert!(
        review::start(&fixture.0, "demo.rule", vec![]).is_err(),
        "a selector is ambiguous even when its conflicting function is associated with a different anchor"
    );
    assert!(!sage_meta::check(&fixture.0).is_valid());
}

#[test]
#[sage::test(meta.reviews.fragment_freshness)]
#[sage::test(meta.reviews.recording)]
fn disappeared_or_invalidated_additional_fragments_reject_recording() {
    for range in [false, true] {
        let fixture = Fixture::new();
        fixture.start();
        fixture.write("dependency.txt", "first\nsecond\n");
        let selector = if range {
            Selector::LineRange { start: 2, end: 2 }
        } else {
            Selector::WholeFile
        };
        review::capture_read(&fixture.0, "demo.rule", "dependency.txt", selector).unwrap();
        let saved = std::fs::read(fixture.0.join("validations/demo.rule/input.json")).unwrap();
        if range {
            fixture.write("dependency.txt", "first\n");
        } else {
            std::fs::remove_file(fixture.0.join("dependency.txt")).unwrap();
        }
        let error = review::record(&fixture.0, "demo.rule", &Fixture::report())
            .unwrap_err()
            .to_string();
        assert!(error.contains("dependency.txt"), "{error}");
        assert!(!fixture.0.join("validations/demo.rule/output.json").exists());
        assert_eq!(
            saved,
            std::fs::read(fixture.0.join("validations/demo.rule/input.json")).unwrap()
        );
        assert!(!sage_meta::check(&fixture.0).is_valid());
    }
}

#[test]
#[sage::test(meta.reviews.fragment_freshness)]
#[sage::test(meta.reviews.selector_test)]
#[sage::test(meta.reviews.recorded_reads)]
fn another_anchors_test_read_tracks_body_and_identity() {
    for change in ["body", "missing", "ambiguous"] {
        let fixture = Fixture::new();
        fixture.write(
            "md/design/README.md",
            &format!("{CLAIM}\n{{anchor}}`demo.other`\nOther scenario.\n"),
        );
        fixture.write("src/behavior.rs", "// {anchor}`demo.rule`\npub fn result() -> u32 { 4 }\n// {anchor}`demo.other`\npub fn other() -> u32 { 5 }\n");
        let other = "#[test] #[sage::test(demo.other)] fn other() { assert_eq!(2 + 3, 5); }";
        fixture.write("src/tests.rs", &format!("{TEST}\n{other}"));
        let initial = fixture.start();
        let selector = Selector::Test {
            crate_name: "demo".into(),
            target: "lib:demo".into(),
            rust_path: "demo::tests::other".into(),
        };
        assert_eq!(
            review::capture_read(&fixture.0, "demo.rule", "src/tests.rs", selector).unwrap(),
            other
        );
        let changed = match change {
            "body" => other.replace("2 + 3", "1 + 4"),
            "missing" => other.replace("fn other()", "fn renamed()"),
            _ => format!("#[cfg(unix)] {other}\n#[cfg(windows)] {other}"),
        };
        fixture.write(
            "src/tests.rs",
            &format!("// unrelated line\n{TEST}\n{changed}"),
        );
        if change != "ambiguous" {
            let spec = sage_meta::spec::SpecIndex::load_workspace(&fixture.0).unwrap();
            let rust = sage_meta::correspondence::CorrespondenceIndex::load(&fixture.0).unwrap();
            let current = review::inventory(&spec, &rust).unwrap();
            assert_eq!(initial.fragments, current["demo.rule"].fragments);
            assert_eq!(initial.tests, current["demo.rule"].tests);
        }
        assert!(
            review::record(&fixture.0, "demo.rule", &Fixture::report()).is_err(),
            "{change}"
        );
        assert!(!fixture.0.join("validations/demo.rule/output.json").exists());
    }
}

#[test]
#[sage::test(meta.reviews.gate)]
#[sage::test(meta.reviews.validation_layout)]
#[sage::test(meta.reviews.output_format)]
#[sage::test(meta.reviews.review_diagnostics)]
fn completed_corrupt_records_and_invalid_layout_cannot_pass() {
    for fault in [
        "malformed",
        "blank reviewer",
        "blank report",
        "blank edit",
        "extra field",
        "missing summary",
        "missing input",
        "missing output",
        "non-directory",
    ] {
        let fixture = Fixture::new();
        fixture.approve();
        let directory = fixture.0.join("validations/demo.rule");
        match fault {
            "malformed" => fixture.write("validations/demo.rule/output.json", "{"),
            "missing summary" => std::fs::remove_file(directory.join("summary.json")).unwrap(),
            "missing input" => std::fs::remove_file(directory.join("input.json")).unwrap(),
            "missing output" => std::fs::remove_file(directory.join("output.json")).unwrap(),
            "non-directory" => {
                fixture.write("validations/unexpected", "not a validation directory")
            }
            _ => {
                let mut output = serde_json::to_value(Fixture::report()).unwrap();
                match fault {
                    "blank reviewer" => output["reviewer"] = " ".into(),
                    "blank report" => output["report"] = "\n".into(),
                    "blank edit" => output["requested_edits"] = serde_json::json!([""]),
                    _ => output["outcome"] = true.into(),
                }
                fixture.write(
                    "validations/demo.rule/output.json",
                    &serde_json::to_string(&output).unwrap(),
                );
            }
        }
        assert!(!sage_meta::check(&fixture.0).is_valid(), "{fault}");
        let output = fixture.run(&["check"]);
        assert!(!output.status.success(), "{fault}");
        let error = String::from_utf8_lossy(&output.stderr);
        let anchor = if fault == "non-directory" {
            "unexpected"
        } else {
            "demo.rule"
        };
        assert!(error.contains(anchor), "{error}");
        assert!(error.contains("invalid validation"), "{error}");
        assert!(error.contains("sage-review-checklist/SKILL.md"), "{error}");
        let reason = match fault {
            "malformed" => "EOF",
            "blank reviewer" | "blank report" => "nonblank reviewer and report",
            "blank edit" => "nonblank strings",
            "extra field" => "unknown field",
            "missing summary" | "missing input" | "missing output" => "No such file",
            "non-directory" => "not a real directory",
            _ => unreachable!(),
        };
        assert!(error.contains(reason), "{fault}: {error}");
    }
}

#[test]
#[sage::test(meta.reviews.gate)]
#[sage::test(meta.reviews.recording)]
#[sage::test(meta.reviews.review_diagnostics)]
fn checking_is_read_only_and_recording_preserves_obsolete_records() {
    let fixture = Fixture::new();
    assert!(!fixture.run(&["check"]).status.success());
    assert!(!fixture.0.join("validations").exists());
    fixture.approve();
    let snapshot = |name: &str| {
        ["input.json", "output.json", "summary.json"]
            .map(|file| std::fs::read(fixture.0.join("validations").join(name).join(file)).unwrap())
    };
    let original = snapshot("demo.rule");
    assert!(fixture.run(&["check"]).status.success());
    assert!(sage_meta::check(&fixture.0).is_valid());
    assert_eq!(snapshot("demo.rule"), original);
    let obsolete = fixture.0.join("validations/demo.retired");
    std::fs::create_dir(&obsolete).unwrap();
    for (index, file) in ["input.json", "output.json", "summary.json"]
        .iter()
        .enumerate()
    {
        std::fs::write(obsolete.join(file), &original[index]).unwrap();
    }
    let mut summary = fixture.summary();
    summary["anchor"] = "demo.retired".into();
    std::fs::write(
        obsolete.join("summary.json"),
        serde_json::to_vec(&summary).unwrap(),
    )
    .unwrap();
    let retired = snapshot("demo.retired");
    assert!(!sage_meta::check(&fixture.0).is_valid());
    assert_eq!(snapshot("demo.rule"), original);
    assert_eq!(snapshot("demo.retired"), retired);
    fixture.start();
    review::record(&fixture.0, "demo.rule", &Fixture::report()).unwrap();
    assert_eq!(snapshot("demo.retired"), retired);
    assert!(!sage_meta::check(&fixture.0).is_valid());
}

#[test]
#[sage::test(meta.reviews.gate)]
fn accepted_report_cannot_replace_missing_test_or_implementation_evidence() {
    for missing in ["test", "implementation"] {
        let fixture = Fixture::new();
        fixture.approve();
        if missing == "test" {
            fixture.write("src/tests.rs", "");
        } else {
            fixture.write("src/behavior.rs", "pub fn result() -> u32 { 4 }\n");
        }
        assert!(!sage_meta::check(&fixture.0).is_valid(), "{missing}");
        let cli = fixture.run(&["check"]);
        assert!(!cli.status.success());
        assert!(String::from_utf8_lossy(&cli.stderr).contains("demo.rule"));
    }
}

#[test]
#[sage::test(meta.reviews.input_format)]
#[sage::test(meta.reviews.review_scope)]
#[sage::test(meta.reviews.input_fragments)]
#[sage::test(meta.reviews.preparation)]
fn input_keeps_complete_association_metadata_and_appends_context_in_one_collection() {
    let fixture = Fixture::new();
    fixture.write(
        "src/lib.rs",
        "#[sage::item(crate)] mod model {}\n#[cfg(unix)] mod behavior;\n#[cfg(test)] mod tests;\n",
    );
    let initial = fixture.start();
    let implementation = &initial.implementations[0];
    assert_eq!(implementation.crate_name, "demo");
    assert_eq!(implementation.target, "lib:demo");
    assert_eq!(implementation.module, "demo::behavior");
    assert_eq!(implementation.file, "src/behavior.rs");
    assert!(implementation.cfg.iter().any(|cfg| cfg.contains("unix")));
    assert!(
        implementation
            .excerpt
            .join("\n")
            .contains("pub fn result() -> u32 { 4 }")
    );
    assert!(!implementation.region);
    assert_eq!(implementation.status, TestStatus::Ordinary);
    assert_eq!(implementation.explanation, None);
    let test = &initial.tests[0];
    assert_eq!(test.crate_name, "demo");
    assert_eq!(test.target, "lib:demo");
    assert_eq!(test.rust_path, "demo::tests::verifies_result");
    assert_eq!(test.file, "src/tests.rs");
    assert!(test.cfg.iter().any(|cfg| cfg.contains("test")));
    assert_eq!(test.status, TestStatus::Ordinary);
    fixture.write("helper.txt", "extra context\n");
    assert_eq!(
        review::capture_read(&fixture.0, "demo.rule", "helper.txt", Selector::WholeFile).unwrap(),
        "extra context\n"
    );
    let captured = fixture.input();
    assert_eq!(captured.implementations, initial.implementations);
    assert_eq!(captured.tests, initial.tests);
    assert_eq!(
        &captured.fragments[..initial.fragments.len()],
        initial.fragments
    );
    assert_eq!(captured.fragments.len(), initial.fragments.len() + 1);
    assert_eq!(
        captured.fragments.last().unwrap().contents.join("\n"),
        "extra context\n"
    );
    let json = serde_json::to_value(&captured).unwrap();
    let mut keys: Vec<_> = json
        .as_object()
        .unwrap()
        .keys()
        .map(String::as_str)
        .collect();
    keys.sort();
    assert_eq!(
        keys,
        [
            "coordinator_instructions",
            "fragments",
            "implementations",
            "prompt",
            "tests"
        ]
    );
}

#[test]
#[sage::test(meta.reviews.input_format)]
#[sage::test(meta.reviews.review_scope)]
fn region_association_preserves_placeholder_explanation_and_excerpt() {
    let fixture = Fixture::new();
    fixture.write("src/behavior.rs", "mod todo {\n// {anchor}`demo.rule`<<\n// TODO: Implement arithmetic after the numeric type model is available.\npub fn result() -> u32 { 4 }\n// >>\n}\n");
    let input = fixture.start();
    let site = &input.implementations[0];
    assert!(site.region);
    assert_eq!(site.module, "demo::behavior::todo");
    assert_eq!(site.status, TestStatus::Todo);
    assert!(
        site.explanation
            .as_ref()
            .unwrap()
            .contains("numeric type model")
    );
    let excerpt = site.excerpt.join("\n");
    assert!(excerpt.contains("pub fn result() -> u32 { 4 }"));
    assert!(excerpt.contains("TODO: Implement arithmetic"));
    let saved = fixture.input();
    assert_eq!(saved.implementations, input.implementations);
}

#[test]
#[sage::test(meta.reviews.preparation)]
#[sage::test(meta.reviews.record_required)]
fn interrupted_start_publication_states_cannot_reuse_an_old_approval() {
    let fixture = Fixture::new();
    fixture.approve();
    let directory = fixture.0.join("validations/demo.rule");
    let input = fixture.input();
    // Start removes the old directory before creating any part of the new record.
    std::fs::remove_dir_all(&directory).unwrap();
    assert!(!sage_meta::check(&fixture.0).is_valid());
    assert!(!fixture.run(&["check"]).status.success());
    std::fs::create_dir(&directory).unwrap();
    assert!(!sage_meta::check(&fixture.0).is_valid());
    assert!(!fixture.run(&["check"]).status.success());
    std::fs::write(
        directory.join("input.json"),
        serde_json::to_vec(&input).unwrap(),
    )
    .unwrap();
    assert!(!sage_meta::check(&fixture.0).is_valid());
    assert!(!fixture.run(&["check"]).status.success());
    assert!(!directory.join("output.json").exists());
    // A successful restart also stays failing until a fresh report is recorded.
    fixture.start();
    assert!(!sage_meta::check(&fixture.0).is_valid());
    assert!(!fixture.run(&["check"]).status.success());
    review::record(&fixture.0, "demo.rule", &Fixture::report()).unwrap();
    assert!(sage_meta::check(&fixture.0).is_valid());
}

#[test]
#[sage::test(meta.reviews.recorded_reads)]
fn cli_read_defaults_to_whole_file_from_workspace_subdirectory() {
    let fixture = Fixture::new();
    let before = fixture.start();
    let text = "first\r\nsecond\n";
    fixture.write("context.txt", text);
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_sage-meta"))
        .current_dir(fixture.0.join("src"))
        .args(["review", "read", "demo.rule", "context.txt"])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(output.stdout, text.as_bytes());
    let after = fixture.input();
    assert_eq!(after.fragments.len(), before.fragments.len() + 1);
    assert_eq!(
        &after.fragments[..before.fragments.len()],
        before.fragments.as_slice()
    );
    let captured = after.fragments.last().unwrap();
    assert_eq!(captured.path, "context.txt");
    assert_eq!(captured.selector, Selector::WholeFile);
    assert_eq!(captured.contents.join("\n"), text);
    assert!(fixture.summary()["recorded_at_unix_seconds"].is_null());
}

#[test]
#[sage::test(meta.reviews.recording)]
fn record_rejects_invalid_active_identity_and_changed_associations() {
    for fault in [
        "unknown anchor",
        "missing input",
        "anchor",
        "schema_version",
        "policy_version",
        "test cfg",
    ] {
        let fixture = Fixture::new();
        let initial = fixture.start();
        let directory = fixture.0.join("validations/demo.rule");
        let anchor = match fault {
            "unknown anchor" => "demo.unknown",
            "missing input" => {
                std::fs::remove_file(directory.join("input.json")).unwrap();
                "demo.rule"
            }
            "test cfg" => {
                fixture.write(
                    "src/lib.rs",
                    "#[sage::item(crate)] mod model {}\nmod behavior;\n#[cfg(unix)] mod tests;\n",
                );
                // The test text stays identical; only its enclosing cfg changes.
                let spec = sage_meta::check(&fixture.0);
                assert!(!spec.is_valid());
                assert_eq!(fixture.input(), initial);
                "demo.rule"
            }
            field => {
                let mut summary = fixture.summary();
                summary[field] = if field == "anchor" {
                    "demo.other".into()
                } else {
                    0.into()
                };
                std::fs::write(
                    directory.join("summary.json"),
                    serde_json::to_vec(&summary).unwrap(),
                )
                .unwrap();
                "demo.rule"
            }
        };
        assert!(
            review::record(&fixture.0, anchor, &Fixture::report()).is_err(),
            "{fault}"
        );
        assert!(!directory.join("output.json").exists(), "{fault}");
        assert!(
            fixture.summary()["recorded_at_unix_seconds"].is_null(),
            "{fault}"
        );
        assert!(!sage_meta::check(&fixture.0).is_valid(), "{fault}");
    }
}
