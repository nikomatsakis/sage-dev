#![feature(rustc_private)]

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use libtest_mimic::{Arguments, Failed, Trial};
use rust_ref::{
    CallDispatch, Crate, DefKind, DefPathSegment, Expr, Item, Module, NormalizedDef, Type,
};
use sage_oracle_harness::{
    Fixture, assert_crates_eq, check_annotations, combined, discover_fixtures, fixtures_dir,
};

const DB_DROP_GUARD_SNAPSHOT: &str = include_str!("snapshots/db_drop_guard.json");
const PARSE_NEXT_SNAPSHOT: &str = include_str!("snapshots/parse_next.json");

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

fn assert_parse_next_coverage(side: &str, krate: &Crate<NormalizedDef>) -> Result<(), Failed> {
    fn find_next_body(module: &Module<NormalizedDef>) -> Option<&Expr<NormalizedDef>> {
        module.items.iter().find_map(|item| match item {
            Item::Fn(function) if function.name == "next" => function.body.as_ref(),
            Item::Mod(module) => find_next_body(module),
            Item::Fn(_) | Item::Struct(_) | Item::Enum(_) => None,
        })
    }

    fn is_external_item(def: &NormalizedDef, krate: &str, kind: DefKind, name: &str) -> bool {
        matches!(
            def,
            NormalizedDef::External(path)
                if path.krate == krate
                    && path.segments.last().is_some_and(|segment| {
                        segment.kind == kind && segment.name == name
                    })
        )
    }

    fn assert_into_iter(side: &str, ty: &Type<NormalizedDef>) -> Result<(), Failed> {
        let Type::Def { target, type_args } = ty else {
            return Err(
                format!("{side} did not retain Iterator::next's concrete Self type").into(),
            );
        };
        if !is_external_item(target, "alloc", DefKind::Struct, "IntoIter") {
            return Err(format!("{side} emitted the wrong Iterator::next Self type").into());
        }
        let [
            Type::Def { target: frame, .. },
            Type::Def {
                target: allocator, ..
            },
        ] = type_args.as_slice()
        else {
            return Err(format!("{side} omitted IntoIter<Frame, Global> substitutions").into());
        };
        if !matches!(frame, NormalizedDef::Local(1))
            || !is_external_item(allocator, "alloc", DefKind::Struct, "Global")
        {
            return Err(format!("{side} emitted the wrong IntoIter substitutions").into());
        }
        Ok(())
    }

    let Some(Expr::Block {
        tail: Some(tail), ..
    }) = find_next_body(&krate.root)
    else {
        return Err(format!("{side} omitted the source-written Parse::next body").into());
    };
    let Expr::Call {
        target,
        dispatch,
        owner_type_args,
        method_type_args,
        args,
        ..
    } = tail.as_ref()
    else {
        return Err(format!("{side} did not emit Option::ok_or as a resolved call").into());
    };
    if !is_external_item(target, "core", DefKind::Fn, "ok_or")
        || !matches!(dispatch, CallDispatch::Direct)
        || !matches!(
            owner_type_args.as_slice(),
            [Type::Def {
                target: NormalizedDef::Local(1),
                ..
            }]
        )
        || !matches!(
            method_type_args.as_slice(),
            [Type::Def {
                target: NormalizedDef::Local(2),
                ..
            }]
        )
    {
        return Err(
            format!("{side} emitted the wrong Option::ok_or dispatch or substitutions").into(),
        );
    }

    let [
        Expr::Call {
            target,
            dispatch,
            owner_type_args,
            method_type_args,
            args,
            ..
        },
        _,
    ] = args.as_slice()
    else {
        return Err(format!("{side} omitted the resolved Iterator::next receiver call").into());
    };
    if !is_external_item(target, "core", DefKind::Fn, "next") || !method_type_args.is_empty() {
        return Err(format!("{side} emitted the wrong Iterator::next target").into());
    }
    let CallDispatch::StaticTrait {
        self_ty,
        trait_target,
        trait_type_args,
    } = dispatch
    else {
        return Err(
            format!("{side} did not retain static trait dispatch for Iterator::next").into(),
        );
    };
    if !is_external_item(trait_target, "core", DefKind::Trait, "Iterator")
        || !trait_type_args.is_empty()
        || owner_type_args.as_slice() != [self_ty.clone()]
    {
        return Err(format!("{side} emitted inconsistent Iterator dispatch metadata").into());
    }
    assert_into_iter(side, self_ty)?;
    let [Expr::Ref { mutable: true, .. }] = args.as_slice() else {
        return Err(
            format!("{side} omitted Iterator::next's explicit mutable receiver borrow").into(),
        );
    };
    Ok(())
}

fn assert_parse_next_snapshot(side: &str, krate: &Crate<NormalizedDef>) -> Result<(), Failed> {
    let actual = format!("{}\n", serde_json::to_string_pretty(krate).unwrap());
    if actual.contains("\"alias\"")
        || actual.contains("\"primitive\": \"?")
        || actual.contains("unsupported")
    {
        return Err(format!("{side} Parse::next output contains an unresolved type").into());
    }
    // ANCHOR: example_exact_parse_next_snapshot
    if actual.as_bytes() != PARSE_NEXT_SNAPSHOT.as_bytes() {
        return Err(format!(
            "{side} Parse::next output differs from the checked-in exact JSON snapshot"
        )
        .into());
    }
    // ANCHOR_END: example_exact_parse_next_snapshot
    Ok(())
}

fn assert_external_adt_default_coverage(
    side: &str,
    krate: &Crate<NormalizedDef>,
) -> Result<(), Failed> {
    let Some(Item::Fn(function)) = krate
        .root
        .items
        .iter()
        .find(|item| matches!(item, Item::Fn(function) if function.name == "take"))
    else {
        return Err(format!("{side} omitted the take function").into());
    };
    let [parameter] = function.params.as_slice() else {
        return Err(format!("{side} emitted the wrong take parameters").into());
    };
    let rust_ref::Type::Def { target, type_args } = &parameter.ty else {
        return Err(format!("{side} did not emit IntoIter as a nominal type").into());
    };
    let NormalizedDef::External(target) = target else {
        return Err(format!("{side} did not resolve IntoIter externally").into());
    };
    if target.krate != "alloc"
        || target
            .segments
            .last()
            .is_none_or(|segment| segment.kind != DefKind::Struct || segment.name != "IntoIter")
    {
        return Err(format!("{side} resolved the wrong IntoIter definition: {target:?}").into());
    }
    let [
        rust_ref::Type::Def { target: frame, .. },
        rust_ref::Type::Def {
            target: allocator, ..
        },
    ] = type_args.as_slice()
    else {
        return Err(format!("{side} did not materialize IntoIter's allocator default").into());
    };
    if !matches!(frame, NormalizedDef::Local(_)) {
        return Err(format!("{side} emitted the wrong IntoIter element: {frame:?}").into());
    }
    let NormalizedDef::External(allocator) = allocator else {
        return Err(format!("{side} did not resolve Global externally").into());
    };
    if allocator.krate != "alloc"
        || allocator
            .segments
            .last()
            .is_none_or(|segment| segment.kind != DefKind::Struct || segment.name != "Global")
    {
        return Err(format!("{side} emitted the wrong allocator default: {allocator:?}").into());
    }
    Ok(())
}

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

    if fixture.name() == "mini_redis/parse_next.rs" {
        assert_parse_next_coverage("oracle", &oracle)?;
        assert_parse_next_coverage("sage", &sage)?;
        assert_parse_next_snapshot("oracle", &oracle)?;
        assert_parse_next_snapshot("sage", &sage)?;
    }

    if fixture.name() == "basics/external_adt_default.rs" {
        assert_external_adt_default_coverage("oracle", &oracle)?;
        assert_external_adt_default_coverage("sage", &sage)?;
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
