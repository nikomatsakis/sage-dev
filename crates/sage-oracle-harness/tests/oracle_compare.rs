#![feature(rustc_private)]

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use libtest_mimic::{Arguments, Failed, Trial};
use sage_oracle_harness::{
    Fixture, assert_crates_eq, discover_fixtures, fixtures_dir, normalize_pair, strip_bodies,
};

fn output_dir() -> PathBuf {
    let base = std::env::temp_dir().join("sage-oracle-output");
    let mut n = 0u32;
    let dir = loop {
        let candidate = base.join(format!("run-{n}"));
        if !candidate.exists() {
            break candidate;
        }
        n += 1;
    };
    fs::create_dir_all(&dir).unwrap();
    dir
}

fn output_paths(fixture: &Fixture, out_dir: &Path) -> (PathBuf, PathBuf) {
    let name = fixture.name();
    let dir = out_dir.join(Path::new(&name).parent().unwrap_or(Path::new("")));
    fs::create_dir_all(&dir).unwrap();

    let stem = Path::new(&name)
        .file_name()
        .unwrap_or_default()
        .to_string_lossy();
    let oracle_path = dir.join(format!("{stem}.oracle.json"));
    let sage_path = dir.join(format!("{stem}.sage.json"));
    (oracle_path, sage_path)
}

fn repro_commands(fixture: &Fixture) -> String {
    let fixtures_root = fixtures_dir();
    match fixture {
        Fixture::SingleFile(path) => {
            let rel = path.strip_prefix(&fixtures_root).unwrap_or(path);
            let fixture_path = format!("test-fixtures/oracle/{}", rel.display());
            format!(
                "  cargo run -p sage-oracle -- {fixture_path}\n  \
                 cargo run -p sage-emit -- {fixture_path}"
            )
        }
        Fixture::Directory { entry, files } => {
            let rel_entry = entry.strip_prefix(&fixtures_root).unwrap_or(entry);
            let entry_str = format!("test-fixtures/oracle/{}", rel_entry.display());
            let extra: Vec<String> = files
                .iter()
                .filter(|f| *f != entry)
                .map(|f| {
                    let rel = f.strip_prefix(&fixtures_root).unwrap_or(f);
                    format!("test-fixtures/oracle/{}", rel.display())
                })
                .collect();
            let oracle_cmd = format!("  cargo run -p sage-oracle -- {entry_str}");
            let sage_cmd = if extra.is_empty() {
                format!("  cargo run -p sage-emit -- {entry_str}")
            } else {
                format!(
                    "  cargo run -p sage-emit -- {entry_str} {}",
                    extra.join(" ")
                )
            };
            format!("{oracle_cmd}\n{sage_cmd}")
        }
    }
}

fn run_fixture(fixture: &Fixture, out_dir: &Path) -> Result<(), Failed> {
    let (oracle_path, sage_path) = output_paths(fixture, out_dir);

    let oracle_raw = fixture.oracle_output();
    let sage_raw = fixture.sage_output();

    fs::write(
        &oracle_path,
        serde_json::to_string_pretty(&oracle_raw).unwrap(),
    )
    .unwrap();
    fs::write(&sage_path, serde_json::to_string_pretty(&sage_raw).unwrap()).unwrap();

    // Signature comparison
    let oracle_sig = strip_bodies(&oracle_raw);
    let sage_sig = strip_bodies(&sage_raw);
    if let Err(msg) = assert_crates_eq(
        &format!("{} [signatures]", fixture.name()),
        &oracle_sig,
        &sage_sig,
    ) {
        return Err(format!(
            "{msg}\n\n\
             Output files:\n  oracle: {}\n  sage:   {}\n\n\
             Reproduce:\n{}",
            oracle_path.display(),
            sage_path.display(),
            repro_commands(fixture),
        )
        .into());
    }

    // Full comparison (normalized)
    let (oracle_norm, sage_norm) = normalize_pair(&oracle_raw, &sage_raw);
    if let Err(msg) = assert_crates_eq(
        &format!("{} [full]", fixture.name()),
        &oracle_norm,
        &sage_norm,
    ) {
        return Err(format!(
            "{msg}\n\n\
             Output files:\n  oracle: {}\n  sage:   {}\n\n\
             Reproduce:\n{}",
            oracle_path.display(),
            sage_path.display(),
            repro_commands(fixture),
        )
        .into());
    }

    Ok(())
}

fn main() {
    let args = Arguments::from_args();
    let out_dir = output_dir();
    let fixtures: Vec<Arc<Fixture>> = discover_fixtures().into_iter().map(Arc::new).collect();

    eprintln!("oracle output dir: {}", out_dir.display());

    let tests: Vec<_> = fixtures
        .iter()
        .map(|fixture| {
            let f = Arc::clone(fixture);
            let dir = out_dir.clone();
            Trial::test(f.name(), move || run_fixture(&f, &dir))
        })
        .collect();

    libtest_mimic::run(&args, tests).exit();
}
