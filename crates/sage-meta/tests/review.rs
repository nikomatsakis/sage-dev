#![feature(register_tool)]
#![register_tool(sage)]

use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};

use sage_meta::review::{self, Decision, ReviewChecklist, TestStatus};

static NEXT: AtomicUsize = AtomicUsize::new(0);
const CLAIM: &str = "{anchor}`demo.rule`\nThe result is four.\n";
const TEST: &str =
    "#[test]\n#[sage::test(demo.rule)]\nfn verifies_result() {\n    assert_eq!(2 + 2, 4);\n}";

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
        std::fs::write(root.join("book.toml"), "[book]\nsrc = 'md'\n").unwrap();
        std::fs::write(
            root.join("md/SUMMARY.md"),
            "- [Design](design/README.md)\n  - [Crates](design/crates/README.md)\n",
        )
        .unwrap();
        std::fs::write(root.join("md/design/crates/README.md"), "# Crates\n").unwrap();
        std::fs::write(
            root.join("Cargo.toml"),
            "[package]\nname = \"demo\"\nversion = \"0.0.0\"\nedition = \"2024\"\n[workspace]\n",
        )
        .unwrap();
        std::fs::write(
            root.join("md/design/crates/demo/crate.sage"),
            "crate demo { \"\"\"Review fixture.\"\"\" }",
        )
        .unwrap();
        let fixture = Self(root);
        fixture.claim(CLAIM);
        fixture.rust(TEST);
        fixture
    }

    fn claim(&self, text: &str) {
        std::fs::write(self.0.join("md/design/README.md"), text).unwrap();
    }

    fn rust(&self, text: &str) {
        let spec = sage_meta::spec::SpecIndex::load_workspace(&self.0).unwrap();
        let implementation = spec
            .anchors
            .keys()
            .map(|name| format!("// {{anchor}}`{name}`\n"))
            .collect::<String>();
        std::fs::write(
            self.0.join("src/lib.rs"),
            format!("#[sage::item(crate)] mod model {{}}\n{implementation}{text}\n"),
        )
        .unwrap();
    }

    fn approve(&self) -> ReviewChecklist {
        let mut submission = review::prepare(&self.0).unwrap();
        for record in &mut submission.reviews {
            record.decision = Decision::Approved;
            record.reason =
                "The body explicitly asserts that adding two and two yields four.".into();
            record.reviewer = "review test fixture".into();
        }
        review::record(&self.0, &submission).unwrap();
        submission
    }

    fn saved(&self) -> String {
        std::fs::read_to_string(self.0.join(review::CHECKLIST_FILE)).unwrap()
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
#[sage::test(meta.reviews.claim_text)]
#[sage::test(meta.reviews.source_representation)]
fn claim_snapshot_contains_the_complete_content_without_outer_whitespace() {
    let fixture = Fixture::new();
    fixture.claim(
        "# Context\n\n{anchor}`demo.rule`  \nThe **result** is four,  \nincluding this second line.\n\nA qualification of the claim.\n\n- Additional list item.\n\n```rust\n    assert_eq!(2 + 2, 4);\n```\n\n| Input | Result |\n| --- | --- |\n| 2 + 2 | 4 |\n\n<!-- end anchor -->\n\nCommentary outside the claim.\n",
    );
    let pending = review::prepare(&fixture.0).unwrap();
    assert_eq!(pending.reviews.len(), 1);
    assert_eq!(pending.reviews[0].anchor, "demo.rule");
    assert_eq!(
        pending.reviews[0].input.claim,
        [
            "The **result** is four,  ",
            "including this second line.",
            "",
            "A qualification of the claim.",
            "",
            "- Additional list item.",
            "",
            "```rust",
            "    assert_eq!(2 + 2, 4);",
            "```",
            "",
            "| Input | Result |",
            "| --- | --- |",
            "| 2 + 2 | 4 |",
        ]
    );
}

#[test]
#[sage::test(meta.reviews.claim_text)]
#[sage::test(meta.reviews.staleness)]
fn later_blocks_invalidate_reviews_but_content_beyond_a_boundary_does_not() {
    for boundary in [
        "<!-- end anchor -->",
        "## Commentary",
        "Commentary\n----------",
        "{anchor}`demo.other`\nA separate rule.",
    ] {
        let fixture = Fixture::new();
        let claim = format!(
            "{CLAIM}\nA later qualification.\n\n- A list case.\n\n```text\nA worked example.\n```\n\n{boundary}\n\nOutside commentary.\n"
        );
        fixture.claim(&claim);
        let has_other = boundary.starts_with("{anchor}");
        fixture.rust(&if has_other {
            format!("{TEST}\n#[test] #[sage::test(demo.other)] fn other() {{ assert!(true); }}")
        } else {
            TEST.into()
        });
        fixture.approve();
        for original in ["A later qualification", "A list case", "A worked example"] {
            fixture.claim(&claim.replace(original, "Changed content"));
            let pending = review::prepare(&fixture.0).unwrap();
            assert_eq!(pending.reviews.len(), 1, "{boundary}: {original}");
            assert_eq!(pending.reviews[0].anchor, "demo.rule");
            assert!(!sage_meta::check(&fixture.0).is_valid());
        }
        fixture.claim(&claim.replace("Outside commentary", "Edited outside commentary"));
        let pending = review::prepare(&fixture.0).unwrap();
        if has_other {
            assert_eq!(pending.reviews.len(), 1);
            assert_eq!(pending.reviews[0].anchor, "demo.other");
        } else {
            assert!(pending.reviews.is_empty(), "{boundary}");
            assert!(sage_meta::check(&fixture.0).is_valid());
        }
        fixture.claim(&claim);
        assert!(review::prepare(&fixture.0).unwrap().reviews.is_empty());
    }

    let fixture = Fixture::new();
    fixture.approve();
    fixture.claim(&format!("{CLAIM}\nA paragraph appended at EOF.\n"));
    let pending = review::prepare(&fixture.0).unwrap();
    assert_eq!(pending.reviews.len(), 1);
    assert_eq!(pending.reviews[0].anchor, "demo.rule");
    assert_eq!(
        pending.reviews[0].input.claim,
        ["The result is four.", "", "A paragraph appended at EOF."]
    );
}

#[test]
#[sage::test(meta.reviews.source_representation)]
fn persisted_source_line_arrays_reconstruct_claim_implementation_and_test() {
    let fixture = Fixture::new();
    let claim =
        "The **result** is four.  \n\nFor example:\n\n```rust\n    assert_eq!(2 + 2, 4);\n```";
    let function = "#[test]\n#[sage::test(demo.rule)]\nfn verifies_result() {\n    // **Evidence** preserves spacing.\n\n    assert_eq!(2 + 2, 4);\n}";
    fixture.claim(&format!("{{anchor}}`demo.rule`\n{claim}\n"));
    fixture.rust(function);
    fixture.approve();
    let json: serde_json::Value = serde_json::from_str(&fixture.saved()).unwrap();
    let input = &json["reviews"][0]["input"];
    for (lines, original) in [
        (&input["claim"], claim),
        (&input["tests"][0]["source"], function),
    ] {
        let lines = lines.as_array().expect("source is saved as an array");
        let strings = lines
            .iter()
            .map(|line| line.as_str().expect("each line is a JSON string"))
            .collect::<Vec<_>>();
        assert!(strings.contains(&""), "internal blank lines survive");
        assert_eq!(strings.join("\n"), original);
    }
    let loaded = review::load(&fixture.0.join(review::CHECKLIST_FILE)).unwrap();
    assert_eq!(
        loaded.reviews[0].input.implementation_sources["src/lib.rs"].join("\n"),
        std::fs::read_to_string(fixture.0.join("src/lib.rs")).unwrap()
    );
    assert_eq!(loaded.reviews[0].input.claim.join("\n"), claim);
    assert_eq!(loaded.reviews[0].input.tests[0].source.join("\n"), function);
}

#[test]
#[sage::test(meta.reviews.review_scope)]
fn implementation_edits_including_outside_the_preview_invalidate_review() {
    let fixture = Fixture::new();
    let implementation = "// {anchor}`demo.rule`<<\nfn implementation() -> u32 { helper() }\n// >>\nfn helper() -> u32 { 2 + 2 }";
    fixture.rust(&format!("{implementation}\n{TEST}"));
    let reviewed = fixture.approve();
    let saved = fixture.saved();
    let input = &reviewed.reviews[0].input;
    assert!(input.implementations.iter().any(|site| site.region));
    assert!(
        input.implementation_sources["src/lib.rs"]
            .join("\n")
            .contains("fn helper()")
    );

    fixture.rust(&format!("{}\n{TEST}", implementation.replace("2 + 2", "5")));
    let pending = review::prepare(&fixture.0).unwrap();
    assert_eq!(pending.reviews.len(), 1);
    assert_eq!(pending.reviews[0].input.tests, input.tests);
    assert_ne!(
        pending.reviews[0].input.implementation_sources,
        input.implementation_sources
    );
    assert!(!sage_meta::check(&fixture.0).is_valid());
    assert!(review::record(&fixture.0, &reviewed).is_err());
    assert_eq!(fixture.saved(), saved);
}

#[test]
#[sage::test(meta.reviews.test_source)]
fn test_snapshot_preserves_attributes_signature_body_comments_and_inherited_cfg() {
    let fixture = Fixture::new();
    let function = "#[test]\n#[sage::test(demo.rule)]\n#[cfg(unix)]\nfn verifies_result() {\n    // This assertion supplies the evidence.\n    assert_eq!(2 + 2, 4);\n}";
    fixture.rust(&format!("#[cfg(test)]\nmod checks {{\n{function}\n}}"));
    let pending = review::prepare(&fixture.0).unwrap();
    let tests = &pending.reviews[0].input.tests;
    assert_eq!(tests.len(), 1);
    assert_eq!(tests[0].source.join("\n"), function);
    assert_eq!(tests[0].crate_name, "demo");
    assert_eq!(tests[0].target, "lib:demo");
    assert_eq!(tests[0].file, "src/lib.rs");
    assert!(tests[0].rust_path.ends_with("checks::verifies_result"));
    assert!(tests[0].cfg.iter().any(|cfg| cfg.contains("test")));
    assert!(tests[0].cfg.iter().any(|cfg| cfg.contains("unix")));
    assert_eq!(tests[0].status, TestStatus::Ordinary);
}

#[test]
#[sage::test(meta.reviews.complete_test_set)]
#[sage::test(meta.reviews.staleness)]
fn adding_or_removing_an_associated_test_invalidates_the_collective_review() {
    let fixture = Fixture::new();
    fixture.approve();
    let extra =
        "#[test] #[sage::test(demo.rule)] fn checks_another_case() { assert_eq!(1 + 3, 4); }";
    fixture.rust(&format!("{TEST}\n{extra}"));
    assert!(!sage_meta::check(&fixture.0).is_valid());
    let pending = review::prepare(&fixture.0).unwrap();
    assert_eq!(pending.reviews.len(), 1, "the review is grouped by claim");
    assert_eq!(pending.reviews[0].input.tests.len(), 2);
    let approved = fixture.approve();
    fixture.rust(&format!("{extra}\n{TEST}"));
    let reordered = review::prepare(&fixture.0).unwrap();
    assert_eq!(
        reordered.reviews[0].input.tests, approved.reviews[0].input.tests,
        "source order must not change the sorted test set"
    );
    assert!(
        !sage_meta::check(&fixture.0).is_valid(),
        "reordering inside an implementation file changes its snapshot"
    );
    fixture.rust(TEST);
    assert!(!sage_meta::check(&fixture.0).is_valid());
    assert_eq!(
        review::prepare(&fixture.0).unwrap().reviews[0]
            .input
            .tests
            .len(),
        1
    );
}

#[test]
#[sage::test(meta.reviews.staleness)]
fn source_and_cfg_changes_require_review_but_claim_movement_does_not() {
    let fixture = Fixture::new();
    fixture.approve();
    fixture.claim(&format!("# A new heading\n\n{CLAIM}"));
    assert!(sage_meta::check(&fixture.0).is_valid());
    assert!(review::prepare(&fixture.0).unwrap().reviews.is_empty());

    fixture.claim("{anchor}`demo.rule`\nThe result is five.\n");
    assert!(!sage_meta::check(&fixture.0).is_valid());
    fixture.claim(CLAIM);
    fixture.rust(&TEST.replace("2 + 2, 4", "2 + 2, 5"));
    assert!(!sage_meta::check(&fixture.0).is_valid());
    fixture.rust(&format!("#[cfg(test)] mod checks {{ {TEST} }}"));
    assert!(!sage_meta::check(&fixture.0).is_valid());
    fixture.approve();
    fixture.rust(&format!("#[cfg(unix)] mod checks {{ {TEST} }}"));
    assert!(
        !sage_meta::check(&fixture.0).is_valid(),
        "only inherited cfg changed"
    );
}

#[test]
#[sage::test(meta.reviews.preparation)]
fn preparing_changed_claims_is_read_only_and_never_reuses_an_approval() {
    let fixture = Fixture::new();
    fixture.approve();
    let saved = fixture.saved();
    fixture.claim("{anchor}`demo.rule`\nThe result is five.\n");
    let pending = review::prepare(&fixture.0).unwrap();
    assert_eq!(pending.reviews.len(), 1);
    assert_eq!(pending.reviews[0].input.claim, ["The result is five."]);
    assert_eq!(pending.reviews[0].decision, Decision::Pending);
    assert!(pending.reviews[0].reason.is_empty());
    assert!(pending.reviews[0].reviewer.is_empty());
    assert_eq!(
        fixture.saved(),
        saved,
        "old reviewed contents survive preparation"
    );
    assert!(review::record(&fixture.0, &pending).is_err());
    assert_eq!(fixture.saved(), saved);
}

#[test]
#[sage::test(meta.reviews.recording)]
#[sage::test(meta.reviews.decisions)]
fn recording_requires_current_inputs_and_explicit_review_evidence() {
    let fixture = Fixture::new();
    let reviewed = fixture.approve();
    let saved = fixture.saved();
    fixture.claim("{anchor}`demo.rule`\nThe result is five.\n");
    assert!(review::record(&fixture.0, &reviewed).is_err());
    assert_eq!(fixture.saved(), saved);
    fixture.claim(CLAIM);

    let mut unknown = reviewed.clone();
    unknown.reviews[0].anchor = "demo.missing".into();
    assert!(review::record(&fixture.0, &unknown).is_err());
    assert_eq!(fixture.saved(), saved);
    let mut duplicate = reviewed.clone();
    duplicate.reviews.push(duplicate.reviews[0].clone());
    assert!(review::record(&fixture.0, &duplicate).is_err());
    assert_eq!(fixture.saved(), saved);

    for field in ["reason", "reviewer"] {
        let mut invalid = reviewed.clone();
        if field == "reason" {
            invalid.reviews[0].reason.clear();
        } else {
            invalid.reviews[0].reviewer.clear();
        }
        assert!(
            review::record(&fixture.0, &invalid).is_err(),
            "missing {field}"
        );
        assert_eq!(fixture.saved(), saved);
    }
    let mut unresolved = reviewed;
    unresolved.reviews[0].decision = Decision::NeedsWork;
    unresolved.reviews[0].reason = "The asserted scenario does not establish every case.".into();
    review::record(&fixture.0, &unresolved).unwrap();
    assert!(!sage_meta::check(&fixture.0).is_valid());
}

#[test]
#[sage::test(meta.reviews.recording)]
#[sage::test(meta.reviews.preparation)]
fn recording_one_changed_claim_preserves_the_other_saved_review() {
    let fixture = Fixture::new();
    fixture.claim(&format!(
        "{CLAIM}\n{{anchor}}`demo.other`\nA second claim.\n"
    ));
    fixture.rust(&format!(
        "{TEST}\n#[test] #[sage::test(demo.other)] fn second() {{ assert_eq!(2 + 2, 4); }}"
    ));
    let reviewed = fixture.approve();
    let unchanged = reviewed
        .reviews
        .iter()
        .find(|review| review.anchor == "demo.other")
        .unwrap()
        .clone();
    fixture.claim("{anchor}`demo.rule`\nThe addition of two and two yields four.\n\n{anchor}`demo.other`\nA second claim.\n");
    let mut pending = review::prepare(&fixture.0).unwrap();
    assert_eq!(pending.reviews.len(), 1);
    assert_eq!(pending.reviews[0].anchor, "demo.rule");
    pending.reviews[0].decision = Decision::Approved;
    pending.reviews[0].reviewer = "second review fixture".into();
    pending.reviews[0].reason = "The body still explicitly asserts the addition result.".into();
    review::record(&fixture.0, &pending).unwrap();
    let saved = review::load(&fixture.0.join(review::CHECKLIST_FILE)).unwrap();
    assert_eq!(saved.reviews.len(), 2);
    assert_eq!(
        saved
            .reviews
            .iter()
            .find(|review| review.anchor == "demo.other"),
        Some(&unchanged)
    );
    assert_eq!(
        saved
            .reviews
            .iter()
            .find(|review| review.anchor == "demo.rule"),
        Some(&pending.reviews[0])
    );
    assert!(sage_meta::check(&fixture.0).is_valid());
}

#[test]
#[sage::test(meta.reviews.gate)]
fn library_and_cli_require_current_review_snapshots_without_mutating_them() {
    let fixture = Fixture::new();
    let missing = sage_meta::check(&fixture.0);
    assert!(!missing.is_valid());
    assert!(missing.review.unwrap().load_error.is_some());
    let output = fixture.run(&["check", "--json"]);
    assert!(!output.status.success());
    let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["review"]["valid"], false);
    assert!(
        String::from_utf8(output.stderr)
            .unwrap()
            .contains("sage-review")
    );

    fixture.approve();
    let saved = fixture.saved();
    let report = sage_meta::check(&fixture.0);
    assert!(report.is_valid());
    assert_eq!(report.review.unwrap().approved, 1);
    assert!(fixture.run(&["check"]).status.success());
    assert_eq!(fixture.saved(), saved);

    fixture.rust(&TEST.replace("2 + 2, 4", "2 + 2, 5"));
    let stale = fixture.run(&["check"]);
    assert!(!stale.status.success());
    assert!(
        String::from_utf8(stale.stderr)
            .unwrap()
            .contains("stale review for demo.rule")
    );
    assert_eq!(sage_meta::check(&fixture.0).review.unwrap().approved, 0);
    assert_eq!(fixture.saved(), saved);
}

#[test]
#[sage::test(meta.reviews.decisions)]
#[sage::test(meta.reviews.test_source)]
fn placeholder_decisions_require_matching_explicit_test_status() {
    let fixture = Fixture::new();
    let mut reviewed = review::prepare(&fixture.0).unwrap();
    reviewed.reviews[0].reviewer = "review test fixture".into();
    reviewed.reviews[0].reason = "A reviewed placeholder explicitly records the gap.".into();
    for decision in [Decision::Todo, Decision::NotApplicable, Decision::Pending] {
        reviewed.reviews[0].decision = decision;
        assert!(review::record(&fixture.0, &reviewed).is_err());
    }
    fixture.rust(&format!("mod todo {{ {TEST} }}"));
    let mut todo = review::prepare(&fixture.0).unwrap();
    assert_eq!(todo.reviews[0].input.tests[0].status, TestStatus::Todo);
    todo.reviews[0].reviewer = "review test fixture".into();
    todo.reviews[0].reason = "The TODO module explicitly records an unfinished test.".into();
    todo.reviews[0].decision = Decision::Approved;
    assert!(review::record(&fixture.0, &todo).is_err());
    todo.reviews[0].decision = Decision::Todo;
    review::record(&fixture.0, &todo).unwrap();
    assert_eq!(sage_meta::check(&fixture.0).review.unwrap().todo, 1);

    fixture.rust(&format!("mod todo {{ {TEST} }}\n{TEST}"));
    let mut mixed = review::prepare(&fixture.0).unwrap();
    assert_eq!(mixed.reviews[0].input.tests.len(), 2);
    mixed.reviews[0].reviewer = "review test fixture".into();
    mixed.reviews[0].reason = "Ordinary evidence exists, but a second test remains TODO.".into();
    mixed.reviews[0].decision = Decision::Approved;
    assert!(review::record(&fixture.0, &mixed).is_err());
    mixed.reviews[0].decision = Decision::Todo;
    review::record(&fixture.0, &mixed).unwrap();
    assert_eq!(sage_meta::check(&fixture.0).review.unwrap().todo, 1);

    std::fs::write(fixture.0.join("src/lib.rs"), format!(
        "#[sage::item(crate)] mod model {{}}\nmod not_applicable {{\n// {{anchor}}`demo.rule`\n{TEST}\n}}"
    )).unwrap();
    let mut na = review::prepare(&fixture.0).unwrap();
    assert_eq!(
        na.reviews[0].input.tests[0].status,
        TestStatus::NotApplicable
    );
    na.reviews[0].reviewer = "review test fixture".into();
    na.reviews[0].reason = "The N/A module explicitly records inapplicability.".into();
    na.reviews[0].decision = Decision::NotApplicable;
    review::record(&fixture.0, &na).unwrap();
    assert_eq!(
        sage_meta::check(&fixture.0).review.unwrap().not_applicable,
        1
    );

    fixture.rust(&format!("mod not_applicable {{ {TEST} }}\n{TEST}"));
    let mut mixed = review::prepare(&fixture.0).unwrap();
    assert_eq!(mixed.reviews[0].input.tests.len(), 2);
    mixed.reviews[0].reviewer = "review test fixture".into();
    mixed.reviews[0].reason =
        "Mixed ordinary and N/A evidence cannot be classified as all N/A.".into();
    mixed.reviews[0].decision = Decision::NotApplicable;
    assert!(review::record(&fixture.0, &mixed).is_err());
    mixed.reviews[0].decision = Decision::Approved;
    assert!(review::record(&fixture.0, &mixed).is_err());
}

#[test]
#[sage::test(meta.reviews.decisions)]
fn a_crate_name_does_not_classify_an_ordinary_test_as_a_placeholder() {
    for crate_name in ["todo", "not_applicable"] {
        let fixture = Fixture::new();
        let manifest = fixture.0.join("Cargo.toml");
        let cargo = std::fs::read_to_string(&manifest).unwrap();
        std::fs::write(
            &manifest,
            cargo.replace("name = \"demo\"", &format!("name = \"{crate_name}\"")),
        )
        .unwrap();
        let pending = review::prepare(&fixture.0).unwrap();
        let test = &pending.reviews[0].input.tests[0];
        assert_eq!(test.crate_name, crate_name);
        assert_eq!(test.rust_path, format!("{crate_name}::verifies_result"));
        assert_eq!(test.status, TestStatus::Ordinary);
    }
}

#[test]
#[sage::test(meta.reviews.staleness)]
#[sage::test(meta.reviews.recording)]
#[sage::test(meta.reviews.gate)]
fn schema_policy_duplicates_and_retired_claims_do_not_silently_pass() {
    let fixture = Fixture::new();
    let reviewed = fixture.approve();
    let path = fixture.0.join(review::CHECKLIST_FILE);
    let mut wrong_schema = reviewed.clone();
    wrong_schema.schema_version += 1;
    let mut wrong_policy = reviewed.clone();
    wrong_policy.policy_version += 1;
    let mut duplicate = reviewed.clone();
    duplicate.reviews.push(duplicate.reviews[0].clone());
    for invalid in [wrong_schema, wrong_policy, duplicate] {
        std::fs::write(&path, serde_json::to_string(&invalid).unwrap()).unwrap();
        assert!(!sage_meta::check(&fixture.0).is_valid());
    }
    std::fs::write(&path, serde_json::to_string(&reviewed).unwrap()).unwrap();
    fixture.claim("# Rule retired\n");
    fixture.rust("");
    assert!(!sage_meta::check(&fixture.0).is_valid());
    let retirement = review::prepare(&fixture.0).unwrap();
    assert!(retirement.reviews.is_empty());
    review::record(&fixture.0, &retirement).unwrap();
    assert!(sage_meta::check(&fixture.0).is_valid());
}

#[test]
#[sage::test(meta.reviews.recording)]
fn cli_preparation_and_recording_preserve_the_reviewed_snapshot() {
    let fixture = Fixture::new();
    let output = fixture.run(&["review", "prepare"]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(!fixture.0.join(review::CHECKLIST_FILE).exists());
    let mut pending: ReviewChecklist = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(pending.reviews[0].decision, Decision::Pending);
    pending.reviews[0].decision = Decision::Approved;
    pending.reviews[0].reviewer = "CLI fixture reviewer".into();
    pending.reviews[0].reason =
        "The assertion explicitly compares the addition result to four.".into();
    std::fs::write(
        fixture.0.join("reviewed.json"),
        serde_json::to_vec(&pending).unwrap(),
    )
    .unwrap();
    let output = fixture.run(&["review", "record", "reviewed.json"]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let saved = review::load(&fixture.0.join(review::CHECKLIST_FILE)).unwrap();
    assert_eq!(saved.reviews[0].input, pending.reviews[0].input);
    assert!(fixture.run(&["check"]).status.success());
    assert!(!fixture.run(&["review", "record"]).status.success());
    assert!(
        !fixture
            .run(&["review", "prepare", "unexpected"])
            .status
            .success()
    );
}

#[test]
#[sage::test(meta.reviews.review_scope)]
#[sage::test(meta.reviews.decisions)]
fn implementation_placeholders_prevent_approval_even_with_ordinary_tests() {
    let fixture = Fixture::new();
    for status in ["todo", "not_applicable"] {
        std::fs::write(fixture.0.join("src/lib.rs"), format!(
            "#[sage::item(crate)] mod model {{}}\nmod {status} {{\n// {{anchor}}`demo.rule`\n// Explicit implementation gap.\n}}\n{TEST}"
        )).unwrap();
        let mut packet = review::prepare(&fixture.0).unwrap();
        let record = &mut packet.reviews[0];
        assert_eq!(record.input.tests[0].status, TestStatus::Ordinary);
        assert_eq!(
            record.input.implementations[0].explanation.as_deref(),
            Some("Explicit implementation gap.")
        );
        record.reviewer = "fixture".into();
        record.reason = "Implementation gap with an ordinary test.".into();
        record.decision = Decision::Approved;
        assert!(review::record(&fixture.0, &packet).is_err());
        packet.reviews[0].decision = Decision::NotApplicable;
        assert!(review::record(&fixture.0, &packet).is_err());
        packet.reviews[0].decision = Decision::Todo;
        assert_eq!(
            review::record(&fixture.0, &packet).is_ok(),
            status == "todo"
        );
    }
}

#[test]
#[sage::test(meta.reviews.staleness)]
#[sage::test(meta.reviews.preparation)]
fn test_only_policy_reviews_load_but_require_fresh_review() {
    let fixture = Fixture::new();
    let approved = fixture.approve();
    let mut legacy = serde_json::to_value(approved).unwrap();
    legacy["policy_version"] = 1.into();
    let input = legacy["reviews"][0]["input"].as_object_mut().unwrap();
    input.remove("implementations");
    input.remove("implementation_sources");
    std::fs::write(
        fixture.0.join(review::CHECKLIST_FILE),
        serde_json::to_string(&legacy).unwrap(),
    )
    .unwrap();
    let saved = fixture.saved();
    assert!(review::load(&fixture.0.join(review::CHECKLIST_FILE)).is_ok());
    assert!(!sage_meta::check(&fixture.0).is_valid());
    let pending = review::prepare(&fixture.0).unwrap();
    assert_eq!(pending.reviews.len(), 1);
    assert_eq!(pending.reviews[0].decision, Decision::Pending);
    assert_eq!(pending.policy_version, review::POLICY_VERSION);
    assert!(!pending.reviews[0].input.implementations.is_empty());
    assert_eq!(fixture.saved(), saved);
}

#[test]
#[sage::test(meta.reviews.review_scope)]
#[sage::test(meta.reviews.staleness)]
fn implementation_associations_and_inherited_cfg_are_review_inputs() {
    let fixture = Fixture::new();
    let root = format!("#[sage::item(crate)] mod model {{}}\n#[cfg(unix)] mod behavior;\n{TEST}");
    std::fs::write(fixture.0.join("src/lib.rs"), &root).unwrap();
    let behavior = "// {anchor}`demo.rule`<<\npub fn result() -> u32 { 4 }\n// >>\n";
    std::fs::write(fixture.0.join("src/behavior.rs"), behavior).unwrap();
    let approved = fixture.approve();
    let input = &approved.reviews[0].input;
    assert_eq!(input.implementation_sources.len(), 1);
    assert_eq!(
        input.implementation_sources["src/behavior.rs"].join("\n"),
        behavior
    );
    let site = &input.implementations[0];
    assert_eq!(site.crate_name, "demo");
    assert_eq!(site.target, "lib:demo");
    assert_eq!(site.module, "demo::behavior");
    assert_eq!(site.cfg, ["#[cfg(unix)]"]);
    assert!(site.region);
    assert_eq!(site.excerpt.join("\n"), behavior);

    // Parent cfg is outside the associated file; association metadata captures it.
    std::fs::write(
        fixture.0.join("src/lib.rs"),
        root.replace("unix", "windows"),
    )
    .unwrap();
    let changed = review::prepare(&fixture.0).unwrap();
    assert_eq!(
        changed.reviews[0].input.implementation_sources,
        input.implementation_sources
    );
    assert_eq!(changed.reviews[0].input.tests, input.tests);
    assert_ne!(
        changed.reviews[0].input.implementations,
        input.implementations
    );
    assert!(review::record(&fixture.0, &approved).is_err());

    std::fs::write(fixture.0.join("src/lib.rs"), &root).unwrap();
    assert!(review::prepare(&fixture.0).unwrap().reviews.is_empty());
    std::fs::write(
        fixture.0.join("src/behavior.rs"),
        format!("{behavior}// {{anchor}}`demo.rule`\n"),
    )
    .unwrap();
    let added = review::prepare(&fixture.0).unwrap();
    assert_eq!(added.reviews[0].input.implementations.len(), 2);
    assert!(!sage_meta::check(&fixture.0).is_valid());
    fixture.approve();
    std::fs::write(fixture.0.join("src/behavior.rs"), behavior).unwrap();
    assert_eq!(
        review::prepare(&fixture.0).unwrap().reviews[0]
            .input
            .implementations
            .len(),
        1
    );
    assert!(!sage_meta::check(&fixture.0).is_valid());
}
