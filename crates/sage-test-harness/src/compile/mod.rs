//! Inline source fixtures using the production host. Preparation cannot update snapshots.
mod annotations;
mod observe;
mod source;
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
        if markers.iter().any(|marker| marker.kind == Kind::Type) != self.observations.is_some() {
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
        let host = SageHost::try_open(project.path(), &[])?;
        host.take_inspection_log();
        host.with_context(|context| {
            let output = observe::collect(context, &markers, self.queries.is_some())?;
            if self.diagnostics.is_none() && !output.diagnostics.is_empty() {
                return Err(format!(
                    "diagnostics expectation omitted, but diagnostics were emitted:\n{}",
                    output.diagnostics
                ));
            }
            Ok(output)
        })
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
    #[sage::test(crates.sage_test_harness.compile_tests.edition)]
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
        assert!(error.contains("0 markers, 1 diagnostics"), "{error}");
        let mut selected = CompileTest::default();
        selected.source("fn good() {} //# ty\nfn bad() -> u16 { true }");
        selected.observations(expect![""]);
        let error = selected.prepare().unwrap_err();
        assert!(error.contains("0 markers, 1 diagnostics"), "{error}");
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
