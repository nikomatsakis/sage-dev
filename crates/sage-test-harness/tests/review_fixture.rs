#![feature(rustc_private)]
#![feature(register_tool)]
#![register_tool(sage)]

use sage_test_harness::compile_test;

#[test]
#[sage::test(crates.sage_test_harness.compile_tests.project_isolation)]
fn fixtures_ignore_caller_directory_and_workspace_defaults() {
    const CHILD: &str = "SAGE_PROJECT_ISOLATION_CHILD";
    if std::env::var_os(CHILD).is_some() {
        // Identical logical filenames and item names must still use each
        // invocation's own source, with stable fixture-relative observations.
        compile_test! {
            source: "fn value() -> u16 {\n 22 //# ty\n}",
            observations: expect_test::expect!["src/lib.rs: literal `22`\n  type: u16\n"],
        }
        compile_test! {
            source: "fn value() -> bool {\n true //# ty\n}",
            observations: expect_test::expect!["src/lib.rs: literal `true`\n  type: bool\n"],
        }
        return;
    }

    let unrelated = tempfile::tempdir().unwrap();
    let workspace = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(workspace.path().join("default/src")).unwrap();
    std::fs::write(
        workspace.path().join("Cargo.toml"),
        "[workspace]\nmembers = [\"default\"]\ndefault-members = [\"default\"]\nresolver = \"3\"\n",
    )
    .unwrap();
    std::fs::write(
        workspace.path().join("default/Cargo.toml"),
        "[package]\nname = \"unrelated_default\"\nversion = \"0.0.0\"\nedition = \"2024\"\n",
    )
    .unwrap();
    std::fs::write(
        workspace.path().join("default/src/lib.rs"),
        "compile_error!(\"caller workspace must not be compiled\");",
    )
    .unwrap();
    for directory in [unrelated.path(), workspace.path()] {
        let output = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "fixtures_ignore_caller_directory_and_workspace_defaults",
                "--nocapture",
            ])
            .env(CHILD, "1")
            .env_remove("UPDATE_EXPECT")
            .current_dir(directory)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "fixture failed from {}:\n{}\n{}",
            directory.display(),
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr),
        );
        assert!(String::from_utf8_lossy(&output.stdout).contains("1 passed"));
    }
}

#[test]
#[sage::test(crates.sage_test_harness.compile_tests.project_isolation)]
fn ordinary_library_disables_test_configuration() {
    compile_test! {
        source: "#[cfg(test)]\nfn excluded() -> u16 { true }\nfn included() {}",
    }
}

#[test]
#[sage::test(crates.sage_test_harness.compile_tests.project_isolation)]
fn ordinary_library_disables_unselected_features() {
    compile_test! {
        source: "#[cfg(feature = \"unused\")]\nfn excluded() -> u16 { true }\nfn included() {}",
    }
}

#[test]
#[sage::test(crates.sage_test_harness.compile_tests.fixtures)]
fn explicit_module_path_can_reach_fixture_file_outside_src() {
    compile_test! {
        files: {
            "src/lib.rs": "#[path = \"../other.rs\"] mod other;",
            "other.rs": "#[warn(dead_code)] pub fn included() {} //# WARN if rustc",
        },
    }
}

#[test]
#[sage::test(crates.sage_test_harness.compile_tests.fixtures)]
fn explicit_module_path_in_inline_module_uses_inline_directory() {
    compile_test! {
        files: {
            "src/lib.rs": "mod inline { #[path = \"child.rs\"] mod child; }",
            "src/inline/child.rs": "#[warn(dead_code)] pub fn included() {} //# WARN if rustc",
        },
    }
}

#[test]
#[sage::test(crates.sage_test_harness.compile_tests.fixtures)]
fn explicit_path_on_inline_module_controls_child_directory() {
    compile_test! {
        files: {
            "src/lib.rs": "#[path = \"alternate\"] mod inline { mod child; }",
            "src/alternate/child.rs": "#[warn(dead_code)] pub fn included() {} //# WARN if rustc",
        },
    }
}

#[test]
#[sage::test(crates.sage_test_harness.compile_tests.fixtures)]
fn explicit_module_path_accepts_non_rs_source_filename() {
    compile_test! {
        files: {
            "src/lib.rs": "#[path = \"module.inc\"] mod included;",
            "src/module.inc": "#[warn(dead_code)] pub fn value() {} //# WARN if rustc",
        },
    }
}
