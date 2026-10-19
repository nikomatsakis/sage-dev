// {anchor}`crates.sage_test_harness.compile_tests.rendering`
//! Inline source fixtures using the production host. Preparation cannot update snapshots.
// {anchor}`crates.sage_test_harness.compile_tests.diagnostic_counts`
// {anchor}`crates.sage_test_harness.compile_tests.completed_types`
// {anchor}`crates.sage_test_harness.compile_tests.grouped_observations`
mod annotations;
mod observe;
mod rustc;
mod source;
mod symbols;
mod types;

use annotations::{Kind, Marker};
use expect_test::Expect;
use sage_host::driver::SageHost;
use std::collections::{BTreeMap, BTreeSet};

/// Run an inline library fixture as an ordinary Cargo test.
// {anchor}`crates.sage_test_harness.compile_tests.invocation`<<
#[macro_export]
macro_rules! compile_test {
    (@fields $test:ident;) => {};
    (@fields $test:ident; files: { $($path:literal: $source:expr),* $(,)? } $(, $($rest:tt)*)?) => {
        $test.files(vec![$(($path.to_owned(), ($source).to_owned())),*]);
        $crate::compile_test!(@fields $test; $($($rest)*)?);
    };
    (@fields $test:ident; $field:ident: $value:expr $(, $($rest:tt)*)?) => {
        $test.$field($value);
        $crate::compile_test!(@fields $test; $($($rest)*)?);
    };
    ($($fields:tt)*) => {{
        let mut test = $crate::compile::CompileTest::default();
        $crate::compile_test!(@fields test; $($fields)*);
        test.run();
    }};
}
// >>

#[derive(Default)]
pub struct CompileTest {
    files: Option<Vec<(String, String)>>,
    edition: Option<String>,
    observations: Option<Expect>,
    diagnostics: Option<Expect>,
    queries: Option<Expect>,
}

macro_rules! expectation_field {
    ($name:ident) => {
        pub fn $name(&mut self, value: Expect) {
            assert!(
                self.$name.replace(value).is_none(),
                concat!("duplicate field: ", stringify!($name))
            );
        }
    };
}

impl CompileTest {
    pub fn source(&mut self, source: impl AsRef<str>) {
        self.files(vec![("src/lib.rs".into(), source.as_ref().into())]);
    }
    pub fn files(&mut self, files: Vec<(String, String)>) {
        assert!(
            self.files.replace(files).is_none(),
            "provide exactly one source or files field"
        );
    }
    pub fn edition(&mut self, edition: impl AsRef<str>) {
        assert!(
            self.edition.replace(edition.as_ref().into()).is_none(),
            "duplicate edition"
        );
    }
    expectation_field!(observations);
    expectation_field!(diagnostics);
    expectation_field!(queries);

    // {anchor}`crates.sage_test_harness.compile_tests.blessing_gate`<<
    // Preparation has no access to expectation assertions. Only a successful
    // return of ALL validated, rendered outputs can reach expect-test, whose
    // assertions may write snapshots when UPDATE_EXPECT is set.
    // {anchor}`crates.sage_test_harness.compile_tests.callsite_updates`
    // {anchor}`crates.sage_test_harness.compile_tests.review_visibility`
    pub fn run(self) {
        let output = self
            .prepare()
            .unwrap_or_else(|error| panic!("compile_test: {error}"));
        if let Some(expect) = self.observations {
            expect.assert_eq(&output.observations);
        }
        if let Some(expect) = self.diagnostics {
            expect.assert_eq(&output.diagnostics);
        }
        if let Some(expect) = self.queries {
            expect.assert_eq(&output.queries);
        }
    }
    // >>

    // {anchor}`crates.sage_test_harness.compile_tests.annotation_validation`
    // {anchor}`crates.sage_test_harness.compile_tests.query_capture`
    // {anchor}`crates.sage_test_harness.compile_tests.coordinates`
    fn prepare(&self) -> Result<Output, String> {
        let files = validate_files(self.files.as_deref().ok_or("missing source or files")?)?;
        let edition = self.edition.as_deref().unwrap_or("2024");
        // {anchor}`crates.sage_test_harness.compile_tests.edition`
        if sage_ir::scope::Edition::parse(edition).is_none() {
            return Err(format!("unknown edition: {edition}"));
        }
        let mut markers = Vec::new();
        for (path, source) in &files {
            markers.extend(annotations::parse(path, source)?);
        }
        if markers
            .iter()
            .any(|marker| matches!(marker.kind, Kind::Type | Kind::Symbol))
            != self.observations.is_some()
        {
            return Err(
                "observation markers require exactly one observations expectation, and vice versa"
                    .into(),
            );
        }
        // {anchor}`crates.sage_test_harness.compile_tests.project_isolation`
        let project = tempfile::tempdir().map_err(|e| e.to_string())?;
        std::fs::write(project.path().join("Cargo.toml"), format!(
            "[package]\nname = \"compile_test_fixture\"\nversion = \"0.0.0\"\nedition = \"{edition}\"\n[workspace]\n"
        )).map_err(|e| e.to_string())?;
        for (path, source) in &files {
            let target = project.path().join(path);
            std::fs::create_dir_all(target.parent().unwrap()).map_err(|e| e.to_string())?;
            std::fs::write(target, source).map_err(|e| e.to_string())?;
        }
        // {anchor}`crates.sage_test_harness.compile_tests.production_execution`
        // {anchor}`crates.sage_test_harness.compile_tests.dependencies`
        // {anchor}`crates.sage_test_harness.compile_tests.rustc_execution`
        // {anchor}`crates.sage_test_harness.compile_tests.compiler_conditions`
        // Run rustc independently even when Sage will report source errors.
        // Keep its result until Sage has also completed preparation; neither
        // compiler's diagnostic mismatch can reach expectation updates.
        let rustc_result = rustc::check(project.path(), &files, edition, &markers);
        let host = SageHost::try_open(project.path(), &[])?;
        host.take_inspection_log();
        let sage_result = host.with_context(|context| {
            let output = observe::collect(context, &markers, self.queries.is_some())?;
            if self.diagnostics.is_none() && !output.diagnostics.is_empty() {
                return Err(format!(
                    "diagnostics expectation omitted, but diagnostics were emitted:\n{}",
                    output.diagnostics
                ));
            }
            Ok(output)
        });
        match (sage_result, rustc_result) {
            (Ok(output), Ok(())) => Ok(output),
            (Err(sage), Err(rustc)) => Err(format!("{sage}\n{rustc}")),
            (Err(error), Ok(())) | (Ok(_), Err(error)) => Err(error),
        }
    }
}

#[derive(Debug)]
struct Output {
    observations: String,
    diagnostics: String,
    queries: String,
}

// {anchor}`crates.sage_test_harness.compile_tests.fixtures`<<
fn validate_files(files: &[(String, String)]) -> Result<BTreeMap<String, String>, String> {
    let mut result = BTreeMap::new();
    for (path, source) in files {
        if path.starts_with('/')
            || matches!(path.as_str(), "Cargo.toml" | "Cargo.lock" | "build.rs")
            || path.starts_with(".cargo/")
            || path.contains('\\')
            || path
                .split('/')
                .any(|p| p.is_empty() || p == "." || p == "..")
        {
            return Err(format!(
                "invalid fixture path: {path}; paths must be relative and cannot replace project configuration"
            ));
        }
        if result.insert(path.clone(), source.clone()).is_some() {
            return Err(format!("duplicate fixture path: {path}"));
        }
    }
    if !result.contains_key("src/lib.rs") {
        return Err("fixture is missing src/lib.rs".into());
    }
    for path in result.keys() {
        for (index, _) in path.match_indices('/') {
            if result.contains_key(&path[..index]) {
                return Err(format!("file/directory conflict: {path}"));
            }
        }
    }
    Ok(result)
}
// >>

#[cfg(test)]
mod tests {
    use super::*;
    use expect_test::expect;

    #[test]
    #[sage::test(crates.sage_test_harness.compile_tests.fixtures)]
    fn rejects_ambiguous_or_escaping_files() {
        for files in [
            vec![("src/other.rs", "")],
            vec![("src/lib.rs", ""), ("Cargo.toml", "")],
            vec![("src/lib.rs", ""), ("Cargo.lock", "")],
            vec![("src/lib.rs", ""), ("build.rs", "")],
            vec![("src/lib.rs", ""), (".cargo/config.toml", "")],
            vec![("src/lib.rs", ""), ("src/lib.rs", "")],
            vec![("src/lib.rs", ""), ("src/../Cargo.toml", "")],
            vec![("src/lib.rs", ""), ("/tmp/out.rs", "")],
            vec![
                ("src/lib.rs", ""),
                ("src/mod", ""),
                ("src/mod/child.rs", ""),
            ],
        ] {
            let files: Vec<_> = files
                .into_iter()
                .map(|(p, s)| (p.into(), s.into()))
                .collect();
            assert!(validate_files(&files).is_err(), "{files:?}");
        }
    }

    #[test]
    #[sage::test(crates.sage_test_harness.compile_tests.annotation_validation)]
    #[sage::test(crates.sage_test_harness.compile_tests.completed_types)]
    #[sage::test(crates.sage_test_harness.compile_tests.starting_expressions)]
    fn expectations_and_source_selections_must_agree() {
        let mut test = CompileTest::default();
        test.source("fn f() { 22; } //# ty");
        assert!(
            test.prepare()
                .unwrap_err()
                .contains("observations expectation")
        );
        let mut test = CompileTest::default();
        test.source("fn f() {}");
        test.observations(expect![""]);
        assert!(
            test.prepare()
                .unwrap_err()
                .contains("observations expectation")
        );
        let mut test = CompileTest::default();
        test.source("fn f() {\n}\n//# ty");
        test.observations(expect![""]);
        assert!(test.prepare().unwrap_err().contains("no expression"));
    }

    #[test]
    #[sage::test(crates.sage_test_harness.compile_tests.completed_types)]
    #[sage::test(crates.sage_test_harness.compile_tests.production_execution)]
    fn completed_types_ignore_expected_text_and_use_later_constraints() {
        let source = "fn f() -> u16 {\n let value = 22; //# ty\n value\n}";
        let mut first = CompileTest::default();
        first.source(source);
        first.observations(expect!["invented bool answer"]);
        let first = first.prepare().unwrap();
        let mut second = CompileTest::default();
        second.source(source);
        second.observations(expect!["invented u64 answer"]);
        let second = second.prepare().unwrap();
        assert_eq!(first.observations, second.observations);
        assert_eq!(
            first.observations,
            "src/lib.rs: literal `22`\n  type: u16\n"
        );
        assert!(first.diagnostics.is_empty());
        assert!(second.diagnostics.is_empty());
    }

    #[test]
    #[sage::test(crates.sage_test_harness.compile_tests.completed_types)]
    fn explicit_error_types_are_observed_after_checking() {
        let mut test = CompileTest::default();
        // Both compilers must reject the undefined name; its explicit error
        // type remains observable once that diagnostic is accounted for.
        test.source("fn f() -> u16 {\n missing //# ty\n //# ERROR\n}");
        test.observations(expect![""]);
        test.diagnostics(expect![""]);
        let output = test.prepare().unwrap();
        assert_eq!(
            output.observations,
            "src/lib.rs: path `missing`\n  type: <error>\n"
        );
        assert!(output.diagnostics.contains("unresolved"));
    }

    #[test]
    #[sage::test(crates.sage_test_harness.compile_tests.annotation_validation)]
    #[sage::test(crates.sage_test_harness.compile_tests.resolved_symbols)]
    fn symbol_selections_require_paths_and_expectations() {
        let mut test = CompileTest::default();
        test.source("fn f(value: u16) -> u16 { value } //# sym");
        assert!(
            test.prepare()
                .unwrap_err()
                .contains("observations expectation")
        );
        let mut test = CompileTest::default();
        test.source("fn f() -> u16 { 22 } //# sym");
        test.observations(expect![""]);
        assert!(test.prepare().unwrap_err().contains("no path expression"));
    }

    #[test]
    #[sage::test(crates.sage_test_harness.compile_tests.resolved_symbols)]
    #[sage::test(crates.sage_test_harness.compile_tests.grouped_observations)]
    #[sage::test(crates.sage_test_harness.compile_tests.rendering)]
    fn unresolved_paths_still_require_error_markers() {
        let mut test = CompileTest::default();
        test.source("fn f() -> u16 {\n missing //# sym\n}");
        test.observations(expect![""]);
        assert!(
            test.prepare()
                .unwrap_err()
                .contains("Sage diagnostic markers do not match: 0 markers, 1 diagnostics")
        );
        let mut test = CompileTest::default();
        test.source("fn f() -> u16 {\n missing //# sym\n //# ERROR\n}");
        test.observations(expect![""]);
        test.diagnostics(expect![""]);
        test.queries(expect![""]);
        let output = test.prepare().unwrap();
        assert_eq!(
            output.observations,
            "src/lib.rs: path `missing`\n  symbol: <unresolved>\n"
        );
        assert!(output.diagnostics.contains("error:"));
        let queries: serde_json::Value = serde_json::from_str(&output.queries).unwrap();
        assert!(!queries.as_array().unwrap().is_empty());
        assert!(!output.diagnostics.contains("symbol: <unresolved>"));
        assert!(!output.queries.contains("symbol: <unresolved>"));
    }

    #[test]
    #[sage::test(crates.sage_test_harness.compile_tests.resolved_symbols)]
    fn unsupported_associated_identity_is_not_unresolved() {
        use sage_ir::local_syms::{
            LocalAssociatedOwner, LocalModItemSym, associated::local_associated_items,
        };
        use sage_ir::symbol::{SymbolData, TraitSymbol};
        use sage_ir::tytree::PathResolution;
        crate::with_test_crate("trait Has { fn answer(); }", |db, root| {
            let [symbol] = root.expanded_module_items(db) else {
                panic!("one trait expected")
            };
            let SymbolData::TraitSymbol(TraitSymbol::Local(owner)) = symbol.data(db) else {
                panic!("local trait expected")
            };
            let [LocalModItemSym::Function(function)] =
                local_associated_items(db, LocalAssociatedOwner::Trait(owner))
            else {
                panic!("one associated function expected")
            };
            let error =
                super::symbols::resolution(db, PathResolution::Def((*function).into()), &[])
                    .unwrap_err();
            assert_eq!(error, "unsupported symbol identity");
        });
    }

    #[test]
    #[sage::test(crates.sage_test_harness.compile_tests.edition)]
    #[sage::test(crates.sage_test_harness.compile_tests.production_execution)]
    fn unknown_edition_fails_setup() {
        let mut test = CompileTest::default();
        test.source("");
        test.edition("future");
        assert!(test.prepare().unwrap_err().contains("unknown edition"));
    }

    #[test]
    #[sage::test(crates.sage_test_harness.compile_tests.checking_scope)]
    fn unselected_private_body_still_reports_errors() {
        let mut test = CompileTest::default();
        test.source("fn good() {}\nfn bad() -> u16 { true }");
        let error = test.prepare().unwrap_err();
        assert!(
            error.contains("Sage diagnostic markers do not match: 0 markers, 1 diagnostics"),
            "{error}"
        );
        let mut selected = CompileTest::default();
        selected.source("fn good() {} //# ty\nfn bad() -> u16 { true }");
        selected.observations(expect![""]);
        let error = selected.prepare().unwrap_err();
        assert!(
            error.contains("Sage diagnostic markers do not match: 0 markers, 1 diagnostics"),
            "{error}"
        );
    }

    #[test]
    #[sage::test(crates.sage_test_harness.compile_tests.fixtures)]
    fn unused_file_annotations_fail() {
        let mut test = CompileTest::default();
        test.files(vec![
            ("src/lib.rs".into(), "".into()),
            (
                "src/unused.rs".into(),
                "fn f() {} //# ERROR if rustc".into(),
            ),
        ]);
        assert!(
            test.prepare()
                .unwrap_err()
                .contains("outside the selected crate")
        );
    }

    #[test]
    #[sage::test(crates.sage_test_harness.compile_tests.query_capture)]
    #[sage::test(crates.sage_test_harness.compile_tests.query_projection)]
    #[sage::test(crates.sage_test_harness.compile_tests.project_isolation)]
    #[sage::test(crates.sage_test_harness.compile_tests.rendering)]
    fn cold_queries_are_stable_across_isolated_projects() {
        let mut first = CompileTest::default();
        first.source("fn f() -> u16 { 22 }");
        first.queries(expect![""]);
        let first = first.prepare().unwrap();
        let mut second = CompileTest::default();
        second.source("fn f() -> u16 { 22 }");
        second.queries(expect![""]);
        let second = second.prepare().unwrap();
        assert_eq!(first.queries, second.queries);
        assert!(first.queries.contains("executed"), "{}", first.queries);
        assert!(first.queries.contains("body"), "{}", first.queries);
        assert!(!first.queries.contains("/tmp/"));
        assert!(!first.queries.contains("Id("));
    }
}

#[cfg(test)]
mod rustc_tests {
    use super::*;

    #[test]
    #[sage::test(crates.sage_test_harness.compile_tests.rustc_execution)]
    #[sage::test(crates.sage_test_harness.compile_tests.rustc_lints)]
    #[sage::test(crates.sage_test_harness.compile_tests.compiler_conditions)]
    fn rustc_is_mandatory_and_warnings_can_be_enabled_in_source() {
        let mut quiet = CompileTest::default();
        quiet.source("fn unused() {}\nfn nonSnakeCase() {}");
        quiet.prepare().unwrap();

        let mut missing = CompileTest::default();
        missing.source("#![warn(dead_code)]\nfn unused() {}");
        let error = missing.prepare().unwrap_err();
        assert!(
            error.contains("Rustc diagnostic markers do not match: 0 markers, 1 diagnostics"),
            "{error}"
        );

        let mut expected = CompileTest::default();
        expected.source("#![warn(dead_code)]\nfn unused() {} //# WARN if rustc");
        assert!(expected.prepare().unwrap().diagnostics.is_empty());

        let mut denied = CompileTest::default();
        denied.source("#![deny(dead_code)]\nfn unused() {} //# ERROR if rustc");
        denied.prepare().unwrap();
    }

    #[test]
    #[sage::test(crates.sage_test_harness.compile_tests.compiler_conditions)]
    fn shared_markers_must_match_both_compilers() {
        let mut test = CompileTest::default();
        test.source("#![warn(dead_code)]\nfn unused() {} //# WARN");
        let error = test.prepare().unwrap_err();
        assert!(
            error.contains("Sage diagnostic markers do not match: 1 markers, 0 diagnostics"),
            "{error}"
        );
        let mut test = CompileTest::default();
        test.source("fn unused() {} //# ERROR if rustc");
        let error = test.prepare().unwrap_err();
        assert!(
            error.contains("Rustc diagnostic markers do not match: 1 markers, 0 diagnostics"),
            "{error}"
        );
    }
}

#[cfg(test)]
mod coordinate_review_tests {
    use super::*;

    #[test]
    #[sage::test(crates.sage_test_harness.compile_tests.coordinates)]
    #[sage::test(crates.sage_test_harness.compile_tests.compiler_conditions)]
    #[sage::test(crates.sage_test_harness.compile_tests.rendering)]
    fn evaluated_source_retains_blank_lines_indentation_and_unicode() {
        let mut test = CompileTest::default();
        // Leading blank line and indentation are intentional fixture contents.
        // Both compilers must match the same physical line after the Unicode name.
        test.source("\n    fn café() -> u8 {\n        true //# ERROR\n    }\n");
        test.diagnostics(expect_test::expect![""]);
        let output = test.prepare().unwrap();
        assert!(
            output.diagnostics.contains("src/lib.rs:2:"),
            "{}",
            output.diagnostics
        );
        assert!(
            output.diagnostics.contains("2 |       fn café()"),
            "{}",
            output.diagnostics
        );
        assert!(
            output.diagnostics.contains("3 | |         true //# ERROR"),
            "{}",
            output.diagnostics
        );
    }
}

#[cfg(test)]
mod edition_review_tests {
    use super::*;

    #[test]
    #[sage::test(crates.sage_test_harness.compile_tests.edition)]
    fn default_edition_supplies_into_future_but_2021_does_not() {
        let mut current = CompileTest::default();
        current.source("fn f<T: IntoFuture>() {}");
        current.prepare().unwrap();
        let mut older = CompileTest::default();
        older.edition("2021");
        older.source("fn f<T: IntoFuture>() {} //# ERROR");
        older.diagnostics(expect_test::expect![""]);
        assert!(
            older
                .prepare()
                .unwrap()
                .diagnostics
                .contains("unresolved trait")
        );
    }
}

#[cfg(test)]
mod compiler_conditions_review_tests {
    use super::*;
    use annotations::Compiler;
    use sage_ir::diagnostic::{DiagnosticSnapshot, Severity, SourceLocation};

    #[test]
    #[sage::test(crates.sage_test_harness.compile_tests.compiler_conditions)]
    fn compiler_qualifiers_and_shared_counts_are_independent() {
        for (kind, severity) in [("ERROR", Severity::Error), ("WARN", Severity::Warning)] {
            for qualifier in ["", " if SAGE", " if rustc"] {
                let source = format!("fn f() {{}} //# {kind}{qualifier}");
                let markers = annotations::parse("src/lib.rs", &source).unwrap();
                let diagnostic = DiagnosticSnapshot {
                    severity: severity.clone(),
                    primary: vec![SourceLocation {
                        path: "src/lib.rs".into(),
                        source: source.clone(),
                        start: 0,
                        end: 1,
                    }],
                    message: "fixture diagnostic".into(),
                    labels: vec![],
                    notes: vec![],
                };
                for compiler in [Compiler::Sage, Compiler::Rustc] {
                    let applies = qualifier.is_empty()
                        || (qualifier == " if SAGE" && compiler == Compiler::Sage)
                        || (qualifier == " if rustc" && compiler == Compiler::Rustc);
                    assert_eq!(
                        observe::match_compiler_diagnostics(compiler, &markers, &[]).is_ok(),
                        !applies,
                    );
                    assert_eq!(
                        observe::match_compiler_diagnostics(
                            compiler,
                            &markers,
                            std::slice::from_ref(&diagnostic),
                        )
                        .is_ok(),
                        applies,
                    );
                    assert!(
                        observe::match_compiler_diagnostics(
                            compiler,
                            &markers,
                            &[diagnostic.clone(), diagnostic.clone()],
                        )
                        .is_err()
                    );
                }
            }
        }
    }
}

#[cfg(test)]
mod diagnostic_count_review_tests {
    use super::*;
    use sage_ir::diagnostic::{DiagnosticSnapshot, Severity, SourceLocation};

    #[test]
    #[sage::test(crates.sage_test_harness.compile_tests.diagnostic_counts)]
    #[sage::test(crates.sage_test_harness.compile_tests.rendering)]
    fn matching_preserves_identical_diagnostics_and_output_order() {
        let source = "fn f() {} //# ERROR\n//# ERROR\n//# ERROR";
        let markers = annotations::parse("src/lib.rs", source).unwrap();
        let first = DiagnosticSnapshot {
            severity: Severity::Error,
            primary: vec![SourceLocation {
                path: "src/lib.rs".into(),
                source: source.into(),
                start: 0,
                end: 1,
            }],
            message: "first".into(),
            labels: vec![],
            notes: vec![],
        };
        let mut second = first.clone();
        second.message = "second".into();
        let diagnostics = vec![first.clone(), second, first];
        let before = diagnostics
            .iter()
            .map(DiagnosticSnapshot::render)
            .collect::<Vec<_>>();
        observe::match_diagnostics(&markers, &diagnostics).unwrap();
        assert_eq!(
            diagnostics
                .iter()
                .map(DiagnosticSnapshot::render)
                .collect::<Vec<_>>(),
            before
        );
        assert!(observe::match_diagnostics(&markers[..2], &diagnostics).is_err());
    }

    #[test]
    #[sage::test(crates.sage_test_harness.compile_tests.diagnostic_counts)]
    fn matching_snapshot_cannot_hide_missing_diagnostic() {
        let mut test = CompileTest::default();
        test.source("fn f() {} //# ERROR");
        // The actual diagnostic rendering is empty, exactly as this expectation.
        test.diagnostics(expect_test::expect![""]);
        let error = test.prepare().unwrap_err();
        assert!(
            error.contains("Sage diagnostic markers do not match: 1 markers, 0 diagnostics"),
            "{error}"
        );
    }

    #[test]
    #[sage::test(crates.sage_test_harness.compile_tests.diagnostic_counts)]
    fn incomplete_expansion_is_a_source_diagnostic() {
        let mut test = CompileTest::default();
        test.source("missing!();");
        let error = test.prepare().unwrap_err();
        assert!(
            error.contains("Sage diagnostic markers do not match"),
            "{error}"
        );
        assert!(error.contains("module expansion is incomplete"), "{error}");
    }
}

#[cfg(test)]
mod query_capture_review_tests {
    use super::*;

    #[test]
    #[sage::test(crates.sage_test_harness.compile_tests.query_capture)]
    #[sage::test(crates.sage_test_harness.compile_tests.resolved_symbols)]
    fn cold_capture_includes_discovery_and_observation_reads_without_changing_bodies() {
        let source = "fn selected() -> std::cmp::Ordering {\n std::cmp::Ordering::Equal // SELECT\n}\nfn unselected() -> u16 { 33 }";
        let mut plain = CompileTest::default();
        plain.source(source);
        plain.queries(expect_test::expect![""]);
        let plain = plain.prepare().unwrap();
        let mut selected = CompileTest::default();
        selected.source(source.replace("// SELECT", "//# sym"));
        selected.observations(expect_test::expect![""]);
        selected.queries(expect_test::expect![""]);
        let selected = selected.prepare().unwrap();
        assert!(
            selected
                .observations
                .contains("symbol: variant std::cmp::Ordering::Equal"),
            "{}",
            selected.observations
        );
        // Selecting an observation must not alter the checked body set or hide
        // semantic reads performed while freezing external definition names.
        assert_eq!(plain.queries, selected.queries);
        let trace: serde_json::Value = serde_json::from_str(&selected.queries).unwrap();
        fn collect<'a>(nodes: &'a serde_json::Value, out: &mut Vec<&'a serde_json::Value>) {
            for node in nodes.as_array().unwrap() {
                out.push(node);
                collect(&node["children"], out);
            }
        }
        let mut nodes = Vec::new();
        collect(&trace, &mut nodes);
        for operation in ["setup_root_module", "::sig_", "::body_", "tcx::def_path"] {
            assert!(
                nodes.iter().any(
                    |node| node["operation"].as_str().unwrap().ends_with(operation)
                        && node["disposition"] == "executed"
                ),
                "missing cold {operation}: {}",
                selected.queries
            );
        }
        assert_eq!(
            nodes
                .iter()
                .filter(
                    |node| node["operation"].as_str().unwrap().ends_with("::body_")
                        && node["disposition"] == "executed"
                )
                .count(),
            2
        );
    }
}

#[cfg(test)]
mod rustc_execution_review_tests {
    use super::*;

    #[test]
    #[sage::test(crates.sage_test_harness.compile_tests.rustc_execution)]
    fn both_compilers_report_source_errors_from_materialized_modules() {
        let mut test = CompileTest::default();
        test.files(vec![
            ("src/lib.rs".into(), "mod child;".into()),
            ("src/child.rs".into(), "fn bad() -> u8 { true }".into()),
        ]);
        let error = test.prepare().unwrap_err();
        assert!(
            error.contains("Sage diagnostic markers do not match: 0 markers, 1 diagnostics"),
            "{error}"
        );
        assert!(
            error.contains("Rustc diagnostic markers do not match: 0 markers, 1 diagnostics"),
            "{error}"
        );
        assert!(error.contains("src/child.rs"), "{error}");
    }
}
