#![feature(rustc_private)]
#![feature(register_tool)]
#![register_tool(sage)]

use sage_test_harness::compile_test;

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
            "other.rs": "pub fn included() {} //# WARN if rustc",
        },
    }
}

#[test]
#[sage::test(crates.sage_test_harness.compile_tests.fixtures)]
fn explicit_module_path_in_inline_module_uses_inline_directory() {
    compile_test! {
        files: {
            "src/lib.rs": "mod inline { #[path = \"child.rs\"] mod child; }",
            "src/inline/child.rs": "pub fn included() {} //# WARN if rustc",
        },
    }
}

#[test]
#[sage::test(crates.sage_test_harness.compile_tests.fixtures)]
fn explicit_path_on_inline_module_controls_child_directory() {
    compile_test! {
        files: {
            "src/lib.rs": "#[path = \"alternate\"] mod inline { mod child; }",
            "src/alternate/child.rs": "pub fn included() {} //# WARN if rustc",
        },
    }
}

#[test]
#[sage::test(crates.sage_test_harness.compile_tests.fixtures)]
fn explicit_module_path_accepts_non_rs_source_filename() {
    compile_test! {
        files: {
            "src/lib.rs": "#[path = \"module.inc\"] mod included;",
            "src/module.inc": "pub fn value() {} //# WARN if rustc",
        },
    }
}
