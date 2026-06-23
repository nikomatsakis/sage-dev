#![feature(rustc_private)]

use std::path::{Path, PathBuf};

use rust_ref::{Crate, NormalizedDef};

pub mod annotations;

pub fn fixtures_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../test-fixtures/oracle")
}

#[derive(Debug)]
pub enum Fixture {
    SingleFile(PathBuf),
    Directory { entry: PathBuf, files: Vec<PathBuf> },
}

impl Fixture {
    pub fn name(&self) -> String {
        let base = fixtures_dir();
        match self {
            Fixture::SingleFile(path) => path
                .strip_prefix(&base)
                .unwrap_or(path)
                .to_string_lossy()
                .to_string(),
            Fixture::Directory { entry, .. } => entry
                .parent()
                .and_then(|p| p.parent())
                .and_then(|p| p.strip_prefix(&base).ok())
                .map(|p| p.to_string_lossy().to_string())
                .unwrap_or_else(|| entry.to_string_lossy().to_string()),
        }
    }

    pub fn source_text(&self) -> String {
        match self {
            Fixture::SingleFile(path) => std::fs::read_to_string(path).unwrap(),
            Fixture::Directory { entry, .. } => std::fs::read_to_string(entry).unwrap(),
        }
    }

    pub fn oracle_output(&self) -> Result<Crate<NormalizedDef>, String> {
        let entry = match self {
            Fixture::SingleFile(path) => path.clone(),
            Fixture::Directory { entry, .. } => entry.clone(),
        };
        sage_oracle::analyze_file(&entry).map_err(|e| format!("{}", e))
    }

    pub fn sage_output(&self) -> Crate<NormalizedDef> {
        match self {
            Fixture::SingleFile(path) => {
                let source = std::fs::read_to_string(path).unwrap();
                sage_test_harness::with_test_crate(&source, |db, root| {
                    sage_emit::emit_module(db, root)
                })
            }
            Fixture::Directory { entry, files } => {
                let src_dir = entry.parent().unwrap();
                let pairs: Vec<(String, String)> = files
                    .iter()
                    .map(|f| {
                        let rel = f
                            .strip_prefix(src_dir)
                            .unwrap()
                            .to_string_lossy()
                            .to_string();
                        let content = std::fs::read_to_string(f).unwrap();
                        (rel, content)
                    })
                    .collect();
                let refs: Vec<(&str, &str)> = pairs
                    .iter()
                    .map(|(p, c)| (p.as_str(), c.as_str()))
                    .collect();
                sage_test_harness::with_test_crate_files(&refs, |db, root| {
                    sage_emit::emit_module(db, root)
                })
            }
        }
    }

    pub fn has_annotations(&self) -> bool {
        let source = self.source_text();
        source.contains("//#")
    }
}

pub fn discover_fixtures() -> Vec<Fixture> {
    let dir = fixtures_dir();
    let mut fixtures = Vec::new();
    discover_recursive(&dir, &mut fixtures);
    fixtures.sort_by(|a, b| a.name().cmp(&b.name()));
    fixtures
}

fn discover_recursive(dir: &Path, fixtures: &mut Vec<Fixture>) {
    let mut entries: Vec<_> = std::fs::read_dir(dir)
        .unwrap_or_else(|e| panic!("cannot read {}: {}", dir.display(), e))
        .filter_map(|e| e.ok())
        .collect();
    entries.sort_by_key(|e| e.path());

    for entry in &entries {
        let path = entry.path();
        if path.is_file() && path.extension().is_some_and(|ext| ext == "rs") {
            fixtures.push(Fixture::SingleFile(path));
        } else if path.is_dir() {
            let src_dir = path.join("src");
            let lib = src_dir.join("lib.rs");
            let main = src_dir.join("main.rs");
            if lib.exists() || main.exists() {
                let entry_file = if lib.exists() { lib } else { main };
                let files = collect_rs_files(&src_dir);
                fixtures.push(Fixture::Directory {
                    entry: entry_file,
                    files,
                });
            } else {
                discover_recursive(&path, fixtures);
            }
        }
    }
}

fn collect_rs_files(dir: &Path) -> Vec<PathBuf> {
    let mut files = Vec::new();
    collect_rs_recursive(dir, &mut files);
    files.sort();
    files
}

fn collect_rs_recursive(dir: &Path, files: &mut Vec<PathBuf>) {
    for entry in std::fs::read_dir(dir).unwrap().filter_map(|e| e.ok()) {
        let path = entry.path();
        if path.is_file() && path.extension().is_some_and(|ext| ext == "rs") {
            files.push(path);
        } else if path.is_dir() {
            collect_rs_recursive(&path, files);
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// Comparison
// ═══════════════════════════════════════════════════════════════════════════

pub fn assert_crates_eq(
    fixture_name: &str,
    lhs: &Crate<NormalizedDef>,
    rhs: &Crate<NormalizedDef>,
) -> Result<(), String> {
    let lhs_json = serde_json::to_value(lhs).unwrap();
    let rhs_json = serde_json::to_value(rhs).unwrap();

    if lhs_json == rhs_json {
        return Ok(());
    }

    let diff = assert_json_diff::assert_json_matches_no_panic(
        &lhs_json,
        &rhs_json,
        assert_json_diff::Config::new(assert_json_diff::CompareMode::Strict),
    );
    match diff {
        Ok(()) => Ok(()),
        Err(msg) => Err(format!(
            "fixture '{}' diverges between oracle and sage:\n{}",
            fixture_name, msg
        )),
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// Annotation-based checking
// ═══════════════════════════════════════════════════════════════════════════

/// Detect whether sage's output contains error markers (`"?..."` strings).
pub fn sage_output_has_errors(sage: &Crate<NormalizedDef>) -> bool {
    let json = serde_json::to_string(sage).unwrap();
    json.contains("\"?")
}

/// Run annotation-based checks for a fixture with `//#` annotations.
pub fn check_annotations(
    fixture: &Fixture,
    parsed: &annotations::ParsedAnnotations,
) -> Result<(), String> {
    let sage = fixture.sage_output();
    let sage_has_errors = sage_output_has_errors(&sage);

    let mut errors = Vec::new();

    // If annotations expect errors, sage should have produced errors.
    let expects_errors = parsed
        .annotations
        .iter()
        .any(|a| a.severity == annotations::ExpectedSeverity::Error);

    if expects_errors && !sage_has_errors {
        errors.push("annotations expect ERROR but sage produced no errors".to_string());
    }

    if !expects_errors && sage_has_errors {
        errors.push(format!(
            "sage produced errors but no ERROR annotations are present"
        ));
    }

    // Check oracle agreement.
    let oracle_result = fixture.oracle_output();
    let oracle_errored = oracle_result.is_err();

    if expects_errors && !parsed.directives.rustc_ok && !oracle_errored {
        errors.push(
            "sage expected errors but rustc succeeded (add `//# RUSTC OK` if intentional)"
                .to_string(),
        );
    }

    if !expects_errors && !oracle_errored {
        // Both should succeed — compare output normally.
        if let Ok(oracle) = &oracle_result {
            if let Err(msg) = assert_crates_eq(&fixture.name(), oracle, &sage) {
                errors.push(msg);
            }
        }
    }

    if oracle_errored && !expects_errors && !parsed.directives.rustc_error {
        errors.push(format!(
            "rustc errored but no ERROR annotations present: {}",
            oracle_result.unwrap_err()
        ));
    }

    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors.join("\n"))
    }
}
