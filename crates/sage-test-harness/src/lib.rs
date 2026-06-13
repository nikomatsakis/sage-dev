use expect_test::Expect;
pub use expect_test::expect;
use sage_ir::Db;
use sage_ir::db::Database;
use sage_ir::item::{FnAst, LocalModItemSym};
use sage_ir::module::ModSymbol;
use sage_ir::resolve::SourceRoot;
use sage_ir::scope::ScopeSymbol;
use sage_ir::source::SourceFile;
use sage_ir::symbol::FnSymbol;
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
            let (source_root, root) = self.setup(db);
            let mut all_errors = Vec::new();

            let items = sage_ir::resolve::module_items(db, root);
            for item in &items {
                if let LocalModItemSym::Function(fn_ast) = item {
                    let errors = self.check_function(db, *fn_ast, root, source_root);
                    all_errors.extend(errors);
                }
            }

            all_errors
        })
    }

    fn check_function<'db>(
        &self,
        db: &'db dyn Db,
        fn_ast: FnAst<'db>,
        module: ModSymbol<'db>,
        source_root: SourceRoot,
    ) -> Vec<String> {
        let scope = ScopeSymbol::Module(module, source_root);
        let fn_sym = FnSymbol::local(fn_ast, scope);
        let typed = fn_sym.body(db);
        typed.errors.clone()
    }

    fn setup<'db>(&self, db: &'db Database) -> (SourceRoot, ModSymbol<'db>) {
        let source_files: Vec<SourceFile> = self
            .files
            .iter()
            .map(|(path, text)| SourceFile::new(db, path.clone(), text.clone()))
            .collect();
        let source_root = SourceRoot::new(db, source_files.clone());

        let lib_file = source_files
            .iter()
            .find(|f| {
                let p = f.path(db);
                p == "lib.rs" || p == "main.rs"
            })
            .copied()
            .expect("fixture has no lib.rs or main.rs");

        let root_mod = sage_ir::item::ModAst::crate_root(db, lib_file);
        let root = ModSymbol::ast(root_mod);
        (source_root, root)
    }
}
