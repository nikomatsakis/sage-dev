#![feature(rustc_private)]
#![feature(register_tool)]
#![register_tool(sage)]

use std::process::Command;

#[test]
#[sage::test(crates.sage_test_harness.compile_tests.annotation_validation)]
#[sage::test(crates.sage_test_harness.compile_tests.callsite_updates)]
#[sage::test(crates.sage_test_harness.compile_tests.invocation)]
#[sage::test(crates.sage_test_harness.compile_tests.blessing_gate)]
#[sage::test(crates.sage_test_harness.compile_tests.review_visibility)]
fn blessing_edits_the_inline_callsite_and_then_passes_without_updates() {
    // Compile a disposable copy: expect-test must never update this test suite
    // while exercising update mode. The complete fixture and assertion remain
    // visibly inside the annotated function's source for anchor review.
    let project = tempfile::tempdir().unwrap();
    let harness = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    std::fs::create_dir(project.path().join("src")).unwrap();
    std::fs::write(
        project.path().join("Cargo.toml"),
        format!(
            r#"
[package]
name = "inline_callsite_probe"
version = "0.0.0"
edition = "2024"
[workspace]
[dependencies]
sage-test-harness = {{ path = {:?} }}
expect-test = "1.5.1"
[patch.crates-io]
salsa = {{ path = {:?} }}
"#,
            harness,
            harness.join("../../vendor/salsa")
        ),
    )
    .unwrap();
    // Reuse the repository's pinned dependency resolution, not a fresh online
    // resolution of the disposable package's transitive dependencies.
    std::fs::copy(
        harness.join("../../Cargo.lock"),
        project.path().join("Cargo.lock"),
    )
    .unwrap();
    let source = r##"#![feature(rustc_private)]
use sage_test_harness::compile_test;
use expect_test::expect;
#[test]
fn inline_source() {
    compile_test! {
        source: "fn answer() -> u16 {\n    22 //# ty\n}",
        observations: expect![[r#""#]],
        diagnostics: expect![""],
    }
}
"##;
    let path = project.path().join("src/lib.rs");
    std::fs::write(&path, source).unwrap();
    let target = harness.join("../../target");
    let run = |update, expected_success| {
        let mut command = Command::new("cargo");
        command
            .args(["test", "--offline", "--lib", "--quiet"])
            .current_dir(project.path())
            .env("CARGO_TARGET_DIR", &target)
            .env("RUSTC", sage_host::metadata::our_rustc());
        if update {
            command.env("UPDATE_EXPECT", "1");
        } else {
            command.env_remove("UPDATE_EXPECT");
        }
        let output = command.output().unwrap();
        assert_eq!(
            output.status.success(),
            expected_success,
            "{}\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8_lossy(&output.stdout).to_string()
    };
    // A rustc-only warning is an unexpected diagnostic even with UPDATE_EXPECT.
    // The stale observation must remain byte-for-byte unchanged on failure.
    let unexpected = source.replace("fn answer()", "#[warn(dead_code)] fn answer()");
    std::fs::write(&path, &unexpected).unwrap();
    let failure = run(true, false);
    assert!(
        failure.contains("Rustc diagnostic markers do not match"),
        "{failure}"
    );
    assert_eq!(std::fs::read_to_string(&path).unwrap(), unexpected);
    std::fs::write(&path, source).unwrap();
    let stale = run(false, false);
    assert!(stale.contains("expect test failed"), "{stale}");
    assert_eq!(std::fs::read_to_string(&path).unwrap(), source);
    run(true, true);
    let blessed = std::fs::read_to_string(&path).unwrap();
    assert!(
        blessed.contains("src/lib.rs: literal `22`") && blessed.contains("type: u16"),
        "{blessed}"
    );
    assert!(blessed.contains("22 //# ty"));
    assert!(blessed.contains("diagnostics: expect![\"\"]"));
    assert_eq!(blessed.matches("src/lib.rs: literal").count(), 1);
    run(false, true);
    assert_eq!(std::fs::read_to_string(path).unwrap(), blessed);
}
