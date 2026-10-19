// {anchor}`crates.sage_test_harness.compile_tests.production_execution`
//! Independent rustc diagnostics for the same materialized fixture.
// {anchor}`crates.sage_test_harness.compile_tests.fixtures`
use super::annotations::{Compiler, Marker};
use sage_ir::diagnostic::{DiagnosticSnapshot, Severity, SourceLocation};
use serde_json::Value;
use std::collections::BTreeMap;
use std::path::Path;
use std::process::{Command, Output};
use std::sync::OnceLock;

// {anchor}`crates.sage_test_harness.compile_tests.dependencies`
// {anchor}`crates.sage_test_harness.compile_tests.blessing_gate`
// {anchor}`crates.sage_test_harness.compile_tests.rustc_execution`
// {anchor}`crates.sage_test_harness.compile_tests.rustc_lints`
// {anchor}`crates.sage_test_harness.compile_tests.compiler_conditions`
// {anchor}`crates.sage_test_harness.compile_tests.edition`
// {anchor}`crates.sage_test_harness.compile_tests.project_isolation`
pub(super) fn check(
    project: &Path,
    files: &BTreeMap<String, String>,
    edition: &str,
    markers: &[Marker],
) -> Result<(), String> {
    let output = execute(
        Command::new(sage_host::metadata::our_rustc())
            .current_dir(project)
            .args([
                "src/lib.rs",
                "--crate-name",
                "compile_test_fixture",
                "--crate-type",
                "lib",
                "--edition",
                edition,
                "--emit=metadata",
                "-o",
                "fixture.rmeta",
                "--error-format=json",
                "--sysroot",
                sage_host::metadata::our_sysroot(),
            ])
            .args(quiet_lints()?),
    )?;
    validate_output(output, files, markers)
}

fn execute(command: &mut Command) -> Result<Output, String> {
    command
        .output()
        .map_err(|error| format!("rustc could not start: {error}"))
}

fn validate_output(
    output: Output,
    files: &BTreeMap<String, String>,
    markers: &[Marker],
) -> Result<(), String> {
    let stderr = String::from_utf8(output.stderr)
        .map_err(|error| format!("rustc emitted non-UTF-8 output: {error}"))?;
    if !output.stdout.is_empty() {
        return Err("rustc emitted unexpected stdout".into());
    }
    let diagnostics = parse(&stderr, files)?;
    let errors = diagnostics.iter().any(|d| d.severity == Severity::Error);
    match (output.status.code(), errors) {
        (Some(0), false) | (Some(1), true) => {}
        _ => {
            return Err(format!(
                "rustc abnormal or inconsistent exit: {}\n{stderr}",
                output.status
            ));
        }
    }
    super::observe::match_compiler_diagnostics(Compiler::Rustc, markers, &diagnostics)
        .map_err(|error| format!("{error}\nrustc output:\n{stderr}"))
}

// {anchor}`crates.sage_test_harness.compile_tests.diagnostic_locations`
// {anchor}`crates.sage_test_harness.compile_tests.diagnostic_counts`
fn parse(
    stderr: &str,
    files: &BTreeMap<String, String>,
) -> Result<Vec<DiagnosticSnapshot>, String> {
    let mut diagnostics = Vec::new();
    for line in stderr.lines() {
        let value: Value = serde_json::from_str(line)
            .map_err(|error| format!("invalid rustc JSON: {error}\n{line}"))?;
        if value["$message_type"].as_str() != Some("diagnostic") {
            return Err(format!("unexpected rustc output: {line}"));
        }
        let message = string(&value, "message")?;
        let level = string(&value, "level")?;
        let spans = value["spans"]
            .as_array()
            .ok_or("rustc diagnostic lacks spans")?;
        if level == "failure-note" {
            continue;
        }
        let severity = match level {
            "error" => Severity::Error,
            "warning" => Severity::Warning,
            _ => return Err(format!("unexpected rustc diagnostic level: {level}")),
        };
        // These are rustc's final count summaries, not source diagnostics.
        if spans.is_empty() && value["code"].is_null() && is_summary(message, &severity) {
            continue;
        }
        let mut primary = Vec::new();
        for span in spans {
            match span["is_primary"].as_bool() {
                Some(true) => locations(span, files, &mut primary)?,
                Some(false) => {}
                None => return Err("rustc span lacks is_primary".into()),
            }
        }
        diagnostics.push(DiagnosticSnapshot {
            severity,
            message: message.into(),
            primary,
            labels: vec![],
            notes: vec![],
        });
    }
    Ok(diagnostics)
}

fn is_summary(message: &str, severity: &Severity) -> bool {
    fn count(text: &str, singular: &str, plural: &str) -> bool {
        text.split_once(' ').is_some_and(|(n, suffix)| {
            n.parse::<usize>().is_ok() && (suffix == singular || suffix == plural)
        })
    }
    match severity {
        Severity::Warning => count(message, "warning emitted", "warnings emitted"),
        Severity::Error => message
            .strip_prefix("aborting due to ")
            .is_some_and(|rest| {
                let (errors, warnings) = rest
                    .split_once("; ")
                    .map_or((rest, None), |(a, b)| (a, Some(b)));
                count(errors, "previous error", "previous errors")
                    && warnings.is_none_or(|w| count(w, "warning emitted", "warnings emitted"))
            }),
    }
}

fn string<'a>(value: &'a Value, key: &str) -> Result<&'a str, String> {
    value[key]
        .as_str()
        .ok_or_else(|| format!("rustc output lacks string {key}"))
}

fn locations(
    span: &Value,
    files: &BTreeMap<String, String>,
    result: &mut Vec<SourceLocation>,
) -> Result<(), String> {
    let mut span = span;
    loop {
        let raw_path = string(span, "file_name")?;
        let path = sage_ir::source::normalize_path(raw_path);
        if let Some(source) = files
            .get(&path)
            .filter(|_| !Path::new(raw_path).is_absolute())
        {
            let offset = |key: &str| -> Result<usize, String> {
                span[key]
                    .as_u64()
                    .and_then(|n| usize::try_from(n).ok())
                    .ok_or_else(|| format!("rustc span lacks offset {key}"))
            };
            let start = offset("byte_start")?;
            let end = offset("byte_end")?;
            if start > end
                || end > source.len()
                || !source.is_char_boundary(start)
                || !source.is_char_boundary(end)
            {
                return Err(format!("rustc span outside fixture source: {path}"));
            }
            result.push(SourceLocation {
                path: path.clone(),
                source: source.clone(),
                start,
                end,
            });
        }
        match span.get("expansion") {
            Some(Value::Null) => break,
            Some(expansion) => {
                span = expansion
                    .get("span")
                    .ok_or("rustc expansion lacks invocation span")?;
            }
            None => return Err("rustc span lacks expansion provenance".into()),
        }
    }
    Ok(())
}

// {anchor}`crates.sage_test_harness.compile_tests.rustc_lints`
fn quiet_lints() -> Result<&'static [String], String> {
    static FLAGS: OnceLock<Result<Vec<String>, String>> = OnceLock::new();
    FLAGS
        .get_or_init(|| {
            let output = Command::new(sage_host::metadata::our_rustc())
                .args(["-W", "help"])
                .output()
                .map_err(|e| format!("rustc lint inventory failed: {e}"))?;
            if !output.status.success() {
                return Err("rustc lint inventory failed".into());
            }
            let help = String::from_utf8(output.stdout).map_err(|e| e.to_string())?;
            let flags: Vec<_> = help
                .lines()
                .filter_map(|line| {
                    let mut words = line.split_whitespace();
                    let name = words.next()?;
                    (words.next()? == "warn" && name != "warnings").then(|| format!("-A{name}"))
                })
                .collect();
            if !flags.iter().any(|flag| flag == "-Adead-code") {
                return Err("unrecognized rustc lint inventory".into());
            }
            Ok(flags)
        })
        .as_ref()
        .map(Vec::as_slice)
        .map_err(Clone::clone)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn run(source: &str, edition: &str) -> Result<(), String> {
        let project = tempfile::tempdir().unwrap();
        std::fs::create_dir(project.path().join("src")).unwrap();
        std::fs::write(project.path().join("src/lib.rs"), source).unwrap();
        let files = BTreeMap::from([("src/lib.rs".into(), source.into())]);
        let markers = super::super::annotations::parse("src/lib.rs", source).unwrap();
        check(project.path(), &files, edition, &markers)
    }

    #[test]
    #[sage::test(crates.sage_test_harness.compile_tests.rustc_execution)]
    #[sage::test(crates.sage_test_harness.compile_tests.rustc_lints)]
    fn pinned_rustc_checks_errors_editions_and_normal_library_configuration() {
        assert!(
            run("fn f() -> u8 { true }", "2024")
                .unwrap_err()
                .contains("0 markers, 1 diagnostics")
        );
        run("fn f() -> u8 { true } //# ERROR", "2024").unwrap();
        run("fn f() -> u8 { 256 } //# ERROR", "2024").unwrap(); // deny-by-default overflow survives quiet lint policy
        run("fn f<T: IntoFuture>() {}", "2024").unwrap();
        run("fn f<T: IntoFuture>() {} //# ERROR", "2021").unwrap();
        run("#[cfg(test)] fn f() -> u8 { true }\n#[cfg(feature = \"unexpected\")] fn g() -> u8 { true }", "2024").unwrap();
    }

    #[test]
    #[sage::test(crates.sage_test_harness.compile_tests.diagnostic_locations)]
    #[sage::test(crates.sage_test_harness.compile_tests.diagnostic_counts)]
    fn nested_macro_errors_match_invocations_and_multiple_errors_need_multiple_markers() {
        run("macro_rules! inner { () => { fn bad() -> u8 { true } } }\nmacro_rules! outer { () => { inner!(); } }\nouter!(); //# ERROR", "2024").unwrap();
        let source = "fn f() { let _: u8 = true; let _: bool = 1; } //# ERROR\n//# ERROR";
        run(source, "2024").unwrap();
        assert!(
            run(source.trim_end_matches("\n//# ERROR"), "2024")
                .unwrap_err()
                .contains("1 markers, 2 diagnostics")
        );
    }

    fn span(start: usize, end: usize, primary: bool) -> Value {
        json!({"file_name":"src/lib.rs", "byte_start":start, "byte_end":end,
               "is_primary":primary, "expansion":null})
    }
    fn diagnostic(spans: Vec<Value>) -> Value {
        json!({"$message_type":"diagnostic", "level":"error", "message":"test error",
               "code":null, "spans":spans, "children":[]})
    }

    #[test]
    #[sage::test(crates.sage_test_harness.compile_tests.diagnostic_locations)]
    #[sage::test(crates.sage_test_harness.compile_tests.diagnostic_counts)]
    fn primary_spans_and_ancestry_match_but_labels_and_children_do_not() {
        let files = BTreeMap::from([("src/lib.rs".into(), "a\nb\nc\n".into())]);
        let mut origin = span(0, 1, true);
        origin["expansion"] = json!({"span":span(4, 5, false)});
        let mut diag = diagnostic(vec![origin, span(2, 3, false), span(6, 6, true)]);
        diag["children"] = json!([diagnostic(vec![span(2, 3, true)])]);
        let result = parse(&diag.to_string(), &files).unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(
            result[0]
                .primary
                .iter()
                .map(|p| *p.lines().start())
                .collect::<Vec<_>>(),
            vec![1, 3, 4]
        );
        let duplicate = format!("{diag}\n{diag}");
        assert_eq!(parse(&duplicate, &files).unwrap().len(), 2);
        let locationless = parse(&diagnostic(vec![]).to_string(), &files).unwrap();
        assert_eq!(locationless.len(), 1);
        assert!(
            super::super::observe::match_compiler_diagnostics(Compiler::Rustc, &[], &locationless)
                .is_err()
        );
    }

    #[test]
    #[sage::test(crates.sage_test_harness.compile_tests.rustc_execution)]
    fn malformed_compiler_output_and_invalid_fixture_offsets_fail() {
        let files = BTreeMap::from([("src/lib.rs".into(), "x".into())]);
        assert!(parse("not json", &files).is_err());
        assert!(parse("{}", &files).is_err());
        assert!(parse(&diagnostic(vec![span(0, 2, true)]).to_string(), &files).is_err());
        let mut diag = diagnostic(vec![span(0, 1, true)]);
        diag["level"] = json!("unexpected");
        assert!(parse(&diag.to_string(), &files).is_err());
    }

    #[test]
    #[sage::test(crates.sage_test_harness.compile_tests.diagnostic_locations)]
    fn empty_primary_and_nested_invocation_spans_keep_position_lines() {
        for (source, position, line) in [
            ("ab\ncd", 1, 1),
            ("ab\ncd", 3, 2),
            ("ab\ncd", 5, 2),
            ("ab\ncd\n", 6, 3),
        ] {
            let files = BTreeMap::from([("src/lib.rs".into(), source.into())]);
            for expansion in [false, true] {
                let mut primary = span(position, position, true);
                if expansion {
                    let mut inner = span(0, 1, false);
                    inner["expansion"] = json!({"span": primary});
                    primary = span(0, 1, true);
                    primary["expansion"] = json!({"span": inner});
                }
                let diagnostics = parse(&diagnostic(vec![primary]).to_string(), &files).unwrap();
                assert_eq!(diagnostics.len(), 1);
                let location = diagnostics[0].primary.last().unwrap();
                assert_eq!(location.lines(), line..=line);
                let marker = Marker {
                    file: "src/lib.rs".into(),
                    line,
                    kind: super::super::annotations::Kind::Error,
                    compiler: None,
                };
                assert!(
                    super::super::observe::match_compiler_diagnostics(
                        Compiler::Rustc,
                        &[marker.clone()],
                        &diagnostics,
                    )
                    .is_ok()
                );
                assert!(
                    super::super::observe::match_compiler_diagnostics(
                        Compiler::Rustc,
                        &[marker.clone(), marker],
                        &diagnostics,
                    )
                    .is_err()
                );
            }
        }
    }

    #[test]
    #[sage::test(crates.sage_test_harness.compile_tests.diagnostic_locations)]
    fn only_spanless_summaries_and_failure_notes_are_presentation() {
        let files = BTreeMap::from([("src/lib.rs".into(), "x".into())]);
        for (level, message) in [
            ("error", "aborting due to 1 previous error"),
            (
                "error",
                "aborting due to 2 previous errors; 1 warning emitted",
            ),
            ("warning", "1 warning emitted"),
            ("warning", "2 warnings emitted"),
            (
                "failure-note",
                "For more information about this error, try rustc --explain E0308.",
            ),
        ] {
            let mut summary = diagnostic(vec![]);
            summary["level"] = json!(level);
            summary["message"] = json!(message);
            assert!(parse(&summary.to_string(), &files).unwrap().is_empty());
        }
        for level in ["error", "warning"] {
            let mut source_diagnostic = diagnostic(vec![]);
            source_diagnostic["level"] = json!(level);
            let diagnostics = parse(&source_diagnostic.to_string(), &files).unwrap();
            assert_eq!(diagnostics.len(), 1);
            assert!(
                super::super::observe::match_compiler_diagnostics(
                    Compiler::Rustc,
                    &[],
                    &diagnostics,
                )
                .is_err()
            );
        }
        let mut located_warning = diagnostic(vec![span(0, 1, true)]);
        located_warning["level"] = json!("warning");
        located_warning["message"] = json!("1 warning emitted");
        let diagnostics = parse(&located_warning.to_string(), &files).unwrap();
        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].severity, Severity::Warning);
        let mut marker = Marker {
            file: "src/lib.rs".into(),
            line: 1,
            kind: super::super::annotations::Kind::Warning,
            compiler: None,
        };
        assert!(
            super::super::observe::match_compiler_diagnostics(
                Compiler::Rustc,
                &[marker.clone()],
                &diagnostics,
            )
            .is_ok()
        );
        marker.kind = super::super::annotations::Kind::Error;
        assert!(
            super::super::observe::match_compiler_diagnostics(
                Compiler::Rustc,
                &[marker],
                &diagnostics,
            )
            .is_err()
        );
    }
}

#[cfg(all(test, unix))]
mod execution_review_tests {
    use super::*;
    use std::os::unix::process::ExitStatusExt;
    use std::process::ExitStatus;

    #[test]
    #[sage::test(crates.sage_test_harness.compile_tests.rustc_execution)]
    fn missing_compiler_and_abnormal_or_inconsistent_output_fail_preparation() {
        let dir = tempfile::tempdir().unwrap();
        assert!(
            execute(&mut Command::new(dir.path().join("missing-rustc")))
                .unwrap_err()
                .contains("rustc could not start")
        );
        let files = BTreeMap::from([("src/lib.rs".into(), "x".into())]);
        let error = serde_json::json!({"$message_type":"diagnostic", "level":"error",
            "message":"fixture error", "code":null, "spans":[], "children":[]})
        .to_string();
        for (status, stderr, stdout, expected) in [
            (0, error.as_bytes(), vec![], "abnormal or inconsistent exit"),
            (
                1 << 8,
                b"".as_slice(),
                vec![],
                "abnormal or inconsistent exit",
            ),
            (
                2 << 8,
                error.as_bytes(),
                vec![],
                "abnormal or inconsistent exit",
            ),
            (9, b"".as_slice(), vec![], "abnormal or inconsistent exit"),
            (0, b"not json".as_slice(), vec![], "invalid rustc JSON"),
            (0, b"\xff".as_slice(), vec![], "non-UTF-8"),
            (0, b"".as_slice(), vec![b'x'], "unexpected stdout"),
        ] {
            let output = Output {
                status: ExitStatus::from_raw(status),
                stdout,
                stderr: stderr.to_vec(),
            };
            let actual = validate_output(output, &files, &[]).unwrap_err();
            assert!(actual.contains(expected), "{actual}");
        }
        let success = Output {
            status: ExitStatus::from_raw(0),
            stdout: vec![],
            stderr: vec![],
        };
        validate_output(success, &files, &[]).unwrap();
    }
}
