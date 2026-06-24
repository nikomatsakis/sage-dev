use expect_test::Expect;
pub use expect_test::expect;
use sage_ir::Db;
use sage_ir::db::Database;
use sage_ir::local_syms::mods::{LocalModSym, ModBodySource};
use sage_ir::name::Name;
use sage_ir::parse::parse_str_to_cst;
use sage_ir::scope::{LocalCrateSymbol, ScopeSymbol, local_crate};
use sage_ir::source::SourceFile;
use sage_ir::span::{AbsoluteSpan, ParseSource};
use sage_ir::symbol::{FnSymbol, ModSymbol};
use sage_stash::{Stash, Stashed};
use salsa::Database as _;

pub struct TestCrate {
    files: Vec<(String, String)>,
}

impl TestCrate {
    pub fn in_memory(source: &str) -> Self {
        Self {
            files: vec![("lib.rs".to_owned(), source.to_owned())],
        }
    }

    pub fn file(mut self, path: &str, content: &str) -> Self {
        self.files.push((path.to_owned(), content.to_owned()));
        self
    }

    pub fn check_ok(&self) {
        let errors = self.collect_errors();
        if !errors.is_empty() {
            panic!("expected no errors but got:\n{}", errors.join("\n"));
        }
    }

    pub fn check_errors(&self, expect: Expect) {
        let errors = self.collect_errors();
        let actual = errors.join("\n");
        expect.assert_eq(&actual);
    }

    fn collect_errors(&self) -> Vec<String> {
        let mut db = Database::default();
        let lib_file = self.register_files(&mut db);
        db.attach(|db| {
            let (_krate, root) = setup_root_module(db, lib_file);
            let mut all_errors = Vec::new();

            let items = root.expanded_module_items(db);
            for item in items {
                if let sage_ir::symbol::SymbolData::FnSymbol(FnSymbol::Local(local_fn)) =
                    item.data(db)
                {
                    let checked = local_fn.body(db);
                    for diag in &checked.diagnostics {
                        all_errors.push(diag.render(db));
                    }
                }
            }

            all_errors
        })
    }

    fn register_files(&self, db: &mut Database) -> SourceFile {
        let mut lib_file = None;
        for (path, content) in &self.files {
            let sf = db.add_source_file(path.clone(), content.clone());
            if path == "lib.rs" || path == "main.rs" {
                lib_file = Some(sf);
            }
        }
        lib_file.expect("fixture has no lib.rs or main.rs")
    }
}

/// Execute a callback with a fully set-up sage crate from in-memory source.
/// This handles the salsa tracked-function requirement for creating tracked structs.
pub fn with_test_crate<R>(
    source: &str,
    f: impl for<'db> FnOnce(&'db dyn Db, ModSymbol<'db>) -> R,
) -> R {
    with_test_crate_files(&[("lib.rs", source)], f)
}

/// Execute a callback with a multi-file sage crate.
/// Files are given as `(path, content)` pairs. One must be `lib.rs` or `main.rs`.
pub fn with_test_crate_files<R>(
    files: &[(&str, &str)],
    f: impl for<'db> FnOnce(&'db dyn Db, ModSymbol<'db>) -> R,
) -> R {
    let mut db = Database::default();
    let lib_file = {
        let mut lib = None;
        for (path, content) in files {
            let sf = db.add_source_file(path.to_string(), content.to_string());
            if *path == "lib.rs" || *path == "main.rs" {
                lib = Some(sf);
            }
        }
        lib.expect("fixture must include lib.rs or main.rs")
    };
    db.attach(|db| {
        let (_krate, root) = setup_root_module(db, lib_file);
        f(db, root)
    })
}

/// Tracked function that creates the root module and crate.
/// Being a tracked function provides the query-stack context that
/// `LocalModSym::new` (a tracked struct) requires.
#[salsa::tracked]
pub fn setup_root_module<'db>(
    db: &'db dyn Db,
    lib_file: SourceFile,
) -> (LocalCrateSymbol<'db>, ModSymbol<'db>) {
    let mut empty_stash = Stash::new();
    let empty_slice = empty_stash.alloc_slice::<sage_ir::cst::attrs::AttrCst>(&[]);
    let empty_attrs = Stashed::new(empty_stash, empty_slice);
    let abs_span = AbsoluteSpan {
        source: ParseSource::SourceFile(lib_file),
        start: 0,
        end: lib_file.text(db).len() as u32,
    };

    let root_mod = LocalModSym::new(
        db,
        Name::new(db, String::new()),
        None,
        ModBodySource::File(lib_file),
        empty_attrs,
        abs_span,
    );

    let krate = local_crate(db, root_mod);
    let scope = ScopeSymbol::Crate(krate);

    let source = ParseSource::SourceFile(lib_file);
    let items = parse_str_to_cst(db, source, lib_file.text(db), scope);
    sage_ir::local_syms::mods::unexpanded_items::specify(db, root_mod, items);

    let root = ModSymbol::Local(root_mod);
    (krate, root)
}
