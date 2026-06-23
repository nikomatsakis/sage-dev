#![feature(rustc_private)]

use std::path::{Path, PathBuf};

use rust_ref::{Crate, NormalizedDef};

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

    pub fn oracle_output(&self) -> Crate<NormalizedDef> {
        let entry = match self {
            Fixture::SingleFile(path) => path.clone(),
            Fixture::Directory { entry, .. } => entry.clone(),
        };
        sage_oracle::analyze_file(&entry)
            .unwrap_or_else(|e| panic!("oracle failed on {}: {}", entry.display(), e))
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
