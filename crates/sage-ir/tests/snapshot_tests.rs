use std::path::Path;

use expect_test::expect_file;
use sage_ir::db::Database;
use sage_ir::lower::parse_source_file;
use sage_ir::source::SourceFile;
use salsa::Database as _;

fn collect_rs_files(dir: &Path) -> Vec<std::path::PathBuf> {
    let mut files = Vec::new();
    if !dir.is_dir() {
        return files;
    }
    for entry in std::fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            files.extend(collect_rs_files(&path));
        } else if path.extension().is_some_and(|e| e == "rs") {
            files.push(path);
        }
    }
    files.sort();
    files
}

#[test]
fn mini_redis_signatures() {
    let db = Database::default();
    let fixture_dir =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../test-fixtures/mini-redis/src");

    db.attach(|db| {
        let mut out = String::new();
        let files = collect_rs_files(&fixture_dir);

        for path in &files {
            let rel = path.strip_prefix(&fixture_dir).unwrap();
            let text = std::fs::read_to_string(path).unwrap();
            let file = SourceFile::new(db, rel.display().to_string(), text);
            let items = parse_source_file(db, file);

            out.push_str(&format!("// --- {} ---\n", rel.display()));
            for item in items {
                out.push_str(&format!("{item}\n"));
            }
            out.push('\n');
        }

        expect_file!["./snapshots/mini_redis_signatures.txt"].assert_eq(&out);

        // No error nodes should appear in the output.
        assert!(
            !out.contains("{error"),
            "signature output contains {{error}} nodes"
        );
        // Note: {missing} can appear in function bodies for unsupported
        // patterns/expressions. This is expected — the assertion only
        // guards against {error} (parse failures).
    });
}

#[test]
fn mini_redis_bodies() {
    let db = Database::default();
    let fixture_dir =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../test-fixtures/mini-redis/src");

    db.attach(|db| {
        let mut out = String::new();
        let files = collect_rs_files(&fixture_dir);

        for path in &files {
            let rel = path.strip_prefix(&fixture_dir).unwrap();
            let text = std::fs::read_to_string(path).unwrap();
            let file = SourceFile::new(db, rel.display().to_string(), text);
            let items = parse_source_file(db, file);

            out.push_str(&format!("// --- {} ---\n", rel.display()));
            for item in items {
                if let sage_ir::item::LocalModItemSym::Function(f) = item {
                    struct FnBody<'a>(sage_ir::item::FnAst<'a>);
                    impl std::fmt::Display for FnBody<'_> {
                        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                            sage_ir::display::dump_function_body(f, self.0)
                        }
                    }
                    out.push_str(&format!("{}\n", FnBody(*f)));
                }
            }
            out.push('\n');
        }

        expect_file!["./snapshots/mini_redis_bodies.txt"].assert_eq(&out);

        // No error or missing nodes should appear in the output.
        assert!(
            !out.contains("{error"),
            "body output contains {{error}} nodes"
        );
        assert!(
            !out.contains("{missing}"),
            "body output contains {{missing}} nodes"
        );
    });
}
