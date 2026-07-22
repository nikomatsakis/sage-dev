#![feature(rustc_private)]

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use libtest_mimic::{Arguments, Failed, Trial};
use rust_ref::{Crate, DefKind, DefPathSegment, Expr, Item, Module, NormalizedDef};
use sage_oracle_harness::{
    Fixture, assert_crates_eq, check_annotations, combined, discover_fixtures, fixtures_dir,
};

const DB_DROP_GUARD_SNAPSHOT: &str = include_str!("snapshots/db_drop_guard.json");

fn output_dir() -> PathBuf {
    let base = std::env::temp_dir().join("sage-oracle-output");
    fs::create_dir_all(&base).unwrap();
    let mut n = 0u32;
    loop {
        let candidate = base.join(format!("run-{n}"));
        match fs::create_dir(&candidate) {
            Ok(()) => return candidate,
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
                n += 1;
            }
            Err(e) => panic!("failed to create {}: {e}", candidate.display()),
        }
    }
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

fn assert_db_drop_guard_coverage(side: &str, krate: &Crate<NormalizedDef>) -> Result<(), Failed> {
    fn find_db_body(module: &Module<NormalizedDef>) -> Option<&Expr<NormalizedDef>> {
        module.items.iter().find_map(|item| match item {
            Item::Fn(function) if function.name == "db" => function.body.as_ref(),
            Item::Mod(module) => find_db_body(module),
            Item::Fn(_) | Item::Struct(_) | Item::Enum(_) => None,
        })
    }

    let Some(Expr::Block {
        tail: Some(tail), ..
    }) = find_db_body(&krate.root)
    else {
        return Err(format!("{side} omitted the source-written DbDropGuard::db body").into());
    };
    let Expr::Call { target, args, .. } = tail.as_ref() else {
        return Err(format!("{side} did not emit DbDropGuard::db as a resolved call").into());
    };
    let NormalizedDef::External(path) = target else {
        return Err(format!("{side} did not emit an external Clone::clone target").into());
    };
    // ANCHOR: example_exact_external_def_path
    let expected_segments = [
        DefPathSegment {
            kind: DefKind::Mod,
            name: "clone".to_owned(),
        },
        DefPathSegment {
            kind: DefKind::Trait,
            name: "Clone".to_owned(),
        },
        DefPathSegment {
            kind: DefKind::Fn,
            name: "clone".to_owned(),
        },
    ];
    if path.krate != "core" || path.segments != expected_segments {
        return Err(format!("{side} emitted the wrong Clone::clone target: {path:?}").into());
    }
    // ANCHOR_END: example_exact_external_def_path
    let [
        Expr::Ref {
            mutable: false,
            expr,
            ..
        },
    ] = args.as_slice()
    else {
        return Err(format!("{side} omitted the explicit shared receiver borrow").into());
    };
    let Expr::Field { expr, field, .. } = expr.as_ref() else {
        return Err(format!("{side} omitted the resolved db field access").into());
    };
    if field.index != 0 {
        return Err(format!("{side} selected the wrong DbDropGuard field").into());
    }
    let Expr::Deref { expr, .. } = expr.as_ref() else {
        return Err(format!("{side} omitted the explicit self dereference").into());
    };
    if !matches!(expr.as_ref(), Expr::Local { name, index: 0 } if name == "self") {
        return Err(format!("{side} emitted the wrong method receiver").into());
    }
    Ok(())
}

// ANCHOR: example_exact_db_drop_guard_snapshot
fn assert_db_drop_guard_snapshot(side: &str, krate: &Crate<NormalizedDef>) -> Result<(), Failed> {
    let actual = format!("{}\n", serde_json::to_string_pretty(krate).unwrap());
    if actual.as_bytes() != DB_DROP_GUARD_SNAPSHOT.as_bytes() {
        return Err(format!(
            "{side} DbDropGuard output differs from the checked-in exact JSON snapshot"
        )
        .into());
    }
    Ok(())
}
// ANCHOR_END: example_exact_db_drop_guard_snapshot

fn run_fixture(fixture: &Fixture, out_dir: &Path) -> Result<(), Failed> {
    let source = fixture.source_text();
    let parsed = sage_oracle_harness::annotations::parse_annotations(&source);

    if !parsed.annotations.is_empty() || parsed.directives.rustc_ok || parsed.directives.rustc_error
    {
        // Annotation-based test: check diagnostics and oracle agreement.
        if let Err(msg) = check_annotations(fixture, &parsed) {
            return Err(format!("{msg}\n\nReproduce:\n{}", repro_commands(fixture)).into());
        }
        return Ok(());
    }

    // Standard comparison test: both sides must produce identical output.
    // Uses combined mode — single rustc session provides both oracle output
    // and live TcxDb for sage (so sage can resolve external crate items).
    let (oracle_path, sage_path) = output_paths(fixture, out_dir);

    let (oracle_result, sage) = combined::run_combined(fixture);
    let oracle =
        oracle_result.unwrap_or_else(|e| panic!("oracle failed on {}: {}", fixture.name(), e));

    fs::write(&oracle_path, serde_json::to_string_pretty(&oracle).unwrap()).unwrap();
    fs::write(&sage_path, serde_json::to_string_pretty(&sage).unwrap()).unwrap();

    if fixture.name() == "mini_redis/db_drop_guard.rs" {
        assert_db_drop_guard_coverage("oracle", &oracle)?;
        assert_db_drop_guard_coverage("sage", &sage)?;
        assert_db_drop_guard_snapshot("oracle", &oracle)?;
        assert_db_drop_guard_snapshot("sage", &sage)?;
    }

    if let Err(msg) = assert_crates_eq(&fixture.name(), &oracle, &sage) {
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
    eprintln!();

    let tests: Vec<_> = fixtures
        .iter()
        .map(|fixture| {
            let f = Arc::clone(fixture);
            let dir = out_dir.clone();
            Trial::test(f.name(), move || run_fixture(&f, &dir))
        })
        .collect();

    let conclusion = libtest_mimic::run(&args, tests);

    eprintln!();
    eprintln!("════════════════════════════════════════════════════════════");
    if conclusion.num_failed > 0 {
        eprintln!(
            "  \x1b[1;31m{} passed, {} failed\x1b[0m",
            conclusion.num_passed, conclusion.num_failed,
        );
    } else {
        eprintln!(
            "  \x1b[1;32m{} passed, {} failed\x1b[0m",
            conclusion.num_passed, conclusion.num_failed,
        );
    }
    eprintln!("  output: \x1b[1m{}\x1b[0m", out_dir.display());
    eprintln!("════════════════════════════════════════════════════════════");

    conclusion.exit_if_failed();
}
