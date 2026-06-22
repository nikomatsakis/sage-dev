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
        let db = Database::default();
        db.attach(|db| {
            let (_krate, root) = self.setup(db);

            // TODO: diagnostics are not yet surfaced from TyBody; just
            // verify that body checking doesn't panic for now.
            let items = root.expanded_module_items(db);
            for item in items {
                if let sage_ir::symbol::SymbolData::FnSymbol(FnSymbol::Local(local_fn)) =
                    item.data(db)
                {
                    let _ = local_fn.body(db);
                }
            }

            vec![]
        })
    }

    fn setup<'db>(&self, db: &'db Database) -> (LocalCrateSymbol<'db>, ModSymbol<'db>) {
        let source_files: Vec<SourceFile> = self
            .files
            .iter()
            .map(|(path, text)| SourceFile::new(db, path.clone(), text.clone()))
            .collect();

        let lib_file = source_files
            .iter()
            .find(|f| {
                let p = f.path(db);
                p == "lib.rs" || p == "main.rs"
            })
            .copied()
            .expect("fixture has no lib.rs or main.rs");

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
}
