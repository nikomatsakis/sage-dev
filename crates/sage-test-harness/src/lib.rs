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

/// A diagnostic with resolved file position information.
#[derive(Clone, Debug)]
pub struct ResolvedDiagnostic {
    pub line: usize,
    pub message: String,
}

/// Collect diagnostics from a source file, resolved to line numbers.
pub fn collect_diagnostics(source: &str) -> Vec<ResolvedDiagnostic> {
    collect_diagnostics_files(&[("lib.rs", source)])
}

/// Collect diagnostics from multiple files, resolved to line numbers.
pub fn collect_diagnostics_files(files: &[(&str, &str)]) -> Vec<ResolvedDiagnostic> {
    let mut db = Database::default();
    let mut lib_file = None;
    for (path, content) in files {
        let sf = db.add_source_file(path.to_string(), content.to_string());
        if *path == "lib.rs" || *path == "main.rs" {
            lib_file = Some(sf);
        }
    }
    let lib_file = lib_file.expect("fixture must include lib.rs or main.rs");

    db.attach(|db| {
        let (_krate, root) = setup_root_module(db, lib_file);
        let mut diagnostics = Vec::new();

        let items = root.expanded_module_items(db);
        for item in items {
            if let sage_ir::symbol::SymbolData::FnSymbol(FnSymbol::Local(local_fn)) = item.data(db)
            {
                let checked = local_fn.body(db);
                for diag in &checked.diagnostics {
                    let abs = diag.span.resolve(db);
                    let source_text = match abs.source {
                        sage_ir::span::ParseSource::SourceFile(sf) => sf.text(db),
                        _ => continue,
                    };
                    let line = source_text[..abs.start as usize]
                        .chars()
                        .filter(|&c| c == '\n')
                        .count()
                        + 1;
                    diagnostics.push(ResolvedDiagnostic {
                        line,
                        message: diag.message.clone(),
                    });
                }
            }
        }

        diagnostics
    })
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
    let db = Database::default();
    with_test_crate_files_using_db(db, files, f)
}

/// Like `with_test_crate_files`, but uses a caller-provided `Database`.
/// This allows passing a `Database::with_proxy(...)` that has real external crate support.
pub fn with_test_crate_files_using_db<R>(
    mut db: Database,
    files: &[(&str, &str)],
    f: impl for<'db> FnOnce(&'db dyn Db, ModSymbol<'db>) -> R,
) -> R {
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

/// Force the same operation twice in one caller-provided database and return
/// the cold and warm query logs separately. This is intended for incremental
/// dependency tests whose external metadata is supplied by `ProxyTcxDb`.
pub fn with_test_crate_files_twice_using_db<R>(
    mut db: Database,
    files: &[(&str, &str)],
    f: impl for<'db> Fn(&'db dyn Db, ModSymbol<'db>) -> R,
) -> (R, String, String) {
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

    let first = db.attach(|db| {
        let (_krate, root) = setup_root_module(db, lib_file);
        f(db, root)
    });
    let cold = db.take_query_log();
    db.attach(|db| {
        let (_krate, root) = setup_root_module(db, lib_file);
        f(db, root)
    });
    let warm = db.take_query_log();
    (first, cold, warm)
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
        sage_ir::scope::Edition::Rust2021,
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

#[cfg(test)]
#[salsa::interned]
struct ObservedSymbol<'db> {
    symbol: sage_ir::symbol::Symbol<'db>,
}

#[cfg(test)]
#[salsa::tracked]
fn observe_symbol_identity<'db>(_db: &'db dyn Db, _symbol: ObservedSymbol<'db>) {}

#[cfg(test)]
mod file_module_scope_tests {
    use super::*;
    use sage_ir::symbol::{FnSymbol, StructSymbol, Symbol, SymbolData};
    use sage_ir::ty::Ty;

    #[test]
    fn items_in_a_file_backed_module_use_that_module_as_their_scope() {
        with_test_crate_files(
            &[("lib.rs", "mod child;"), ("child.rs", "struct Holder;")],
            |db, root| {
                let child = root
                    .expanded_module_items(db)
                    .iter()
                    .find_map(|symbol| match symbol.data(db) {
                        SymbolData::ModSymbol(ModSymbol::Local(module)) => Some(module),
                        _ => None,
                    })
                    .expect("child module");
                let holder = ModSymbol::Local(child)
                    .expanded_module_items(db)
                    .iter()
                    .find_map(|symbol| match symbol.data(db) {
                        SymbolData::StructSymbol(StructSymbol::Local(strukt)) => Some(strukt),
                        _ => None,
                    })
                    .expect("Holder struct");

                assert_eq!(holder.scope(db), ScopeSymbol::Module(child));
            },
        );
    }

    #[test]
    fn transitive_local_globs_resolve_and_glob_cycles_terminate() {
        with_test_crate_files(
            &[(
                "lib.rs",
                "mod c { pub use crate::b::*; pub struct Item; }\n\
                 mod b { pub use crate::c::*; }\n\
                 mod a { pub use crate::b::*; }\n\
                 use crate::a::*;\n\
                 fn resolved(_: Item) {}\n\
                 fn missing(_: Missing) {}",
            )],
            |db, root| {
                let parameter = |name: &str| {
                    let function = root
                        .expanded_module_items(db)
                        .iter()
                        .find_map(|symbol| match symbol.data(db) {
                            SymbolData::FnSymbol(FnSymbol::Local(function))
                                if function.name(db).text(db) == name =>
                            {
                                Some(function)
                            }
                            _ => None,
                        })
                        .unwrap_or_else(|| panic!("{name} function"));
                    let signature = function.sig(db);
                    let (stash, binder) = signature.open();
                    let [parameter] = &stash[binder.value.params] else {
                        panic!("expected one parameter")
                    };
                    match stash[*parameter] {
                        Ty::Adt(adt, arguments) => (Some(adt), stash[arguments].len(), false),
                        Ty::Error(_) => (None, 0, true),
                        other => panic!("unexpected {name} parameter type: {other:?}"),
                    }
                };

                let (Some(item), 0, false) = parameter("resolved") else {
                    panic!("transitive glob should resolve Item")
                };
                assert_eq!(Symbol::from(item).name(db).unwrap().0.text(db), "Item");
                assert_eq!(parameter("missing"), (None, 0, true));
            },
        );
    }
}

#[cfg(test)]
mod macro_expansion_tests {
    use super::*;
    use sage_ir::symbol::{StructSymbol, SymbolData};

    #[test]
    fn same_module_macro_resolution_reaches_a_fixed_point() {
        with_test_crate(
            r#"
                macro_rules! define_macro {
                    () => {
                        macro_rules! define_generated {
                            () => { struct Generated; }
                        }
                    }
                }

                define_macro!();
                define_generated!();
            "#,
            |db, root| {
                assert!(root.expanded_module_items(db).iter().any(|symbol| {
                    matches!(
                        symbol.data(db),
                        SymbolData::StructSymbol(StructSymbol::Local(strukt))
                            if strukt.name(db).text(db) == "Generated"
                    )
                }));
            },
        );
    }

    #[test]
    fn expanded_module_query_has_a_cold_and_warm_trace() {
        let (_, cold, warm) = with_test_crate_files_twice_using_db(
            Database::default(),
            &[(
                "lib.rs",
                "macro_rules! make { () => { struct Generated; } }\nmake!();",
            )],
            |db, root| {
                assert!(root.expanded_module_items(db).iter().any(|symbol| {
                    matches!(
                        symbol.data(db),
                        SymbolData::StructSymbol(StructSymbol::Local(strukt))
                            if strukt.name(db).text(db) == "Generated"
                    )
                }));
            },
        );

        assert!(
            cold.contains("local_expanded_module_items"),
            "cold expansion must execute the module query: {cold}"
        );
        assert!(
            !warm.contains("local_expanded_module_items"),
            "unchanged expansion must reuse the module query: {warm}"
        );
    }
}

#[cfg(test)]
mod derive_expansion_tests {
    use super::*;
    use sage_ir::resolve::{MacroKind, Namespace};
    use sage_ir::span::ParseSource;
    use sage_ir::symbol::{CrateNum, DefIndex, ImplSymbol, StructSymbol, SymExtKind, SymbolData};
    use sage_ir::tcx::{ExternalDefPath, RawChild, TcxDb};

    #[derive(Clone, Default)]
    struct BuiltinDeriveTcx {
        calls: Option<std::sync::Arc<std::sync::Mutex<Vec<String>>>>,
        edition_only_clone_trait: bool,
    }

    impl BuiltinDeriveTcx {
        fn tracing() -> (Self, std::sync::Arc<std::sync::Mutex<Vec<String>>>) {
            let calls = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
            (
                Self {
                    calls: Some(calls.clone()),
                    edition_only_clone_trait: false,
                },
                calls,
            )
        }

        fn record(&self, call: String) {
            if let Some(calls) = &self.calls {
                calls.lock().unwrap().push(call);
            }
        }
    }

    impl TcxDb for BuiltinDeriveTcx {
        fn extern_crate(&self, name: &str) -> Option<CrateNum> {
            self.record(format!("extern_crate({name})"));
            match name {
                "std" => Some(CrateNum(1)),
                "core" => Some(CrateNum(2)),
                _ => None,
            }
        }

        fn module_children(&self, crate_num: CrateNum, def_index: DefIndex) -> Vec<RawChild> {
            self.record(format!("module_children({},{})", crate_num.0, def_index.0));
            let child = |name: &str,
                         crate_num: u32,
                         def_index: u32,
                         namespace: Namespace,
                         kind: SymExtKind| RawChild {
                name: name.to_owned(),
                crate_num: CrateNum(crate_num),
                def_index: DefIndex(def_index),
                namespace,
                kind,
            };
            match (crate_num.0, def_index.0) {
                (1, 0) => {
                    let mut children = vec![
                        child("prelude", 1, 1, Namespace::Type, SymExtKind::Mod),
                        child("Pair", 1, 20, Namespace::Type, SymExtKind::Struct),
                    ];
                    if self.edition_only_clone_trait {
                        children.push(child(
                            "RenamedEditionClone",
                            2,
                            11,
                            Namespace::Type,
                            SymExtKind::Trait,
                        ));
                    }
                    children
                }
                (1, 1) => vec![
                    child("rust_2015", 1, 8, Namespace::Type, SymExtKind::Mod),
                    child("rust_2018", 1, 9, Namespace::Type, SymExtKind::Mod),
                    child("rust_2021", 1, 10, Namespace::Type, SymExtKind::Mod),
                    child("rust_2024", 1, 2, Namespace::Type, SymExtKind::Mod),
                ],
                (1, edition @ (2 | 8 | 9 | 10)) => {
                    let mut children = vec![
                        child("Clone", 2, 2, Namespace::Type, SymExtKind::Trait),
                        child("Debug", 2, 3, Namespace::Type, SymExtKind::Trait),
                        child(
                            "Clone",
                            1,
                            3,
                            Namespace::Macro(MacroKind::Derive),
                            SymExtKind::MacroDef,
                        ),
                        // One DefId can be exported through more than one macro
                        // sub-namespace. The resolution edge, not SymExt identity,
                        // selects the requested namespace.
                        child(
                            "Clone",
                            1,
                            3,
                            Namespace::Macro(MacroKind::Bang),
                            SymExtKind::MacroDef,
                        ),
                        child(
                            "Debug",
                            1,
                            4,
                            Namespace::Macro(MacroKind::Derive),
                            SymExtKind::MacroDef,
                        ),
                    ];
                    if self.edition_only_clone_trait && edition == 2 {
                        children.push(child(
                            "EditionClone",
                            2,
                            11,
                            Namespace::Type,
                            SymExtKind::Trait,
                        ));
                    }
                    children
                }
                (2, 0) => vec![
                    child("clone", 2, 1, Namespace::Type, SymExtKind::Mod),
                    child("fmt", 2, 4, Namespace::Type, SymExtKind::Mod),
                    child("marker", 2, 6, Namespace::Type, SymExtKind::Mod),
                ],
                (2, 1) => vec![child("Clone", 2, 2, Namespace::Type, SymExtKind::Trait)],
                (2, 4) => vec![child("Debug", 2, 3, Namespace::Type, SymExtKind::Trait)],
                (2, 6) => vec![child("Sized", 2, 5, Namespace::Type, SymExtKind::Trait)],
                _ => Vec::new(),
            }
        }

        fn item_name(&self, crate_num: CrateNum, def_index: DefIndex) -> Option<String> {
            self.record(format!("item_name({},{})", crate_num.0, def_index.0));
            match (crate_num.0, def_index.0) {
                (1, 0) => Some("std"),
                (1, 1) => Some("prelude"),
                (1, 2) => Some("rust_2024"),
                (1, 8) => Some("rust_2015"),
                (1, 9) => Some("rust_2018"),
                (1, 10) => Some("rust_2021"),
                (1, 20) => Some("Pair"),
                (1, 3) | (2, 2) => Some("Clone"),
                (1, 4) | (2, 3) => Some("Debug"),
                (2, 5) => Some("Sized"),
                (2, 7 | 12) => Some("clone"),
                (2, 11) => Some("EditionClone"),
                (2, 0) => Some("core"),
                (2, 1) => Some("clone"),
                (2, 4) => Some("fmt"),
                (2, 6) => Some("marker"),
                _ => None,
            }
            .map(str::to_owned)
        }

        fn is_module(&self, crate_num: CrateNum, def_index: DefIndex) -> bool {
            self.record(format!("is_module({},{})", crate_num.0, def_index.0));
            matches!(
                (crate_num.0, def_index.0),
                (1, 0 | 1 | 2 | 8 | 9 | 10) | (2, 0 | 1 | 4 | 6)
            )
        }

        fn is_builtin_derive(&self, crate_num: CrateNum, def_index: DefIndex) -> bool {
            self.record(format!(
                "is_builtin_derive({},{})",
                crate_num.0, def_index.0
            ));
            matches!((crate_num.0, def_index.0), (1, 3 | 4))
        }

        fn def_path(&self, _crate_num: CrateNum, _def_index: DefIndex) -> Option<String> {
            None
        }

        fn structured_def_path(
            &self,
            _crate_num: CrateNum,
            _def_index: DefIndex,
        ) -> Option<ExternalDefPath> {
            None
        }

        fn trait_signature(
            &self,
            crate_num: CrateNum,
            def_index: DefIndex,
        ) -> Option<sage_ir::tcx::RawTraitSignature> {
            self.record(format!("trait_signature({},{})", crate_num.0, def_index.0));
            use sage_ir::tcx::{
                RawDefId, RawGenericParam, RawGenericParamKind, RawTraitPredicate,
                RawTraitSemantics, RawTraitSignature, RawTy,
            };

            let self_param = RawGenericParam {
                index: 0,
                name: Some("Self".to_owned()),
                kind: RawGenericParamKind::Type,
            };
            match (crate_num.0, def_index.0) {
                (2, 2) => Some(RawTraitSignature {
                    generics: vec![self_param],
                    self_param_index: 0,
                    predicates: vec![RawTraitPredicate {
                        self_ty: RawTy::Param(0),
                        trait_def: RawDefId {
                            crate_num: CrateNum(2),
                            def_index: DefIndex(5),
                            kind: SymExtKind::Trait,
                        },
                        args: Vec::new(),
                    }],
                    semantics: RawTraitSemantics::Ordinary,
                    complete: true,
                }),
                (2, 5) => Some(RawTraitSignature {
                    generics: vec![self_param],
                    self_param_index: 0,
                    predicates: Vec::new(),
                    semantics: RawTraitSemantics::Sized,
                    complete: true,
                }),
                (2, 11) => Some(RawTraitSignature {
                    generics: vec![self_param],
                    self_param_index: 0,
                    predicates: Vec::new(),
                    semantics: RawTraitSemantics::Ordinary,
                    complete: true,
                }),
                _ => None,
            }
        }

        fn associated_items(
            &self,
            crate_num: CrateNum,
            def_index: DefIndex,
        ) -> Option<sage_ir::tcx::RawAssociatedItems> {
            self.record(format!("associated_items({},{})", crate_num.0, def_index.0));
            use sage_ir::tcx::{
                RawAssociatedItem, RawAssociatedItemKind, RawAssociatedItems, RawDefId,
            };

            match (crate_num.0, def_index.0) {
                (2, 2) => Some(RawAssociatedItems {
                    items: vec![RawAssociatedItem {
                        def: RawDefId {
                            crate_num: CrateNum(2),
                            def_index: DefIndex(7),
                            kind: SymExtKind::Fn,
                        },
                        name: "clone".to_owned(),
                        kind: RawAssociatedItemKind::Function,
                    }],
                    complete: true,
                }),
                (2, 3) => Some(RawAssociatedItems {
                    items: Vec::new(),
                    complete: true,
                }),
                (2, 11) => Some(RawAssociatedItems {
                    items: vec![RawAssociatedItem {
                        def: RawDefId {
                            crate_num: CrateNum(2),
                            def_index: DefIndex(12),
                            kind: SymExtKind::Fn,
                        },
                        name: "clone".to_owned(),
                        kind: RawAssociatedItemKind::Function,
                    }],
                    complete: true,
                }),
                _ => None,
            }
        }

        fn fn_signature(
            &self,
            crate_num: CrateNum,
            def_index: DefIndex,
        ) -> Option<sage_ir::tcx::RawFnSignature> {
            self.record(format!("fn_signature({},{})", crate_num.0, def_index.0));
            use sage_ir::cst::Mutability;
            use sage_ir::tcx::{
                RawDefId, RawFnSignature, RawGenericParam, RawGenericParamKind, RawReceiver,
                RawTraitPredicate, RawTy,
            };

            match (crate_num.0, def_index.0) {
                (2, 7 | 12) => Some(RawFnSignature {
                    owner_generics: vec![RawGenericParam {
                        index: 0,
                        name: Some("Self".to_owned()),
                        kind: RawGenericParamKind::Type,
                    }],
                    method_generics: Vec::new(),
                    owner_self_ty: Some(RawTy::Param(0)),
                    owner_trait: Some(RawTraitPredicate {
                        self_ty: RawTy::Param(0),
                        trait_def: RawDefId {
                            crate_num: CrateNum(2),
                            def_index: DefIndex(if def_index.0 == 7 { 2 } else { 11 }),
                            kind: SymExtKind::Trait,
                        },
                        args: Vec::new(),
                    }),
                    receiver: Some(RawReceiver::Ref(Mutability::Shared)),
                    params: Vec::new(),
                    ret: RawTy::Param(0),
                    predicates: Vec::new(),
                    ordinary_complete: true,
                    const_call_complete: true,
                }),
                _ => None,
            }
        }

        fn adt_signature(
            &self,
            crate_num: CrateNum,
            def_index: DefIndex,
        ) -> Option<sage_ir::tcx::RawAdtSignature> {
            self.record(format!("adt_signature({},{})", crate_num.0, def_index.0));
            use sage_ir::tcx::{
                RawAdtSignature, RawDefId, RawGenericDefault, RawGenericParam, RawGenericParamKind,
                RawTraitPredicate, RawTy,
            };

            match (crate_num.0, def_index.0) {
                (1, 20) => Some(RawAdtSignature {
                    generics: vec![
                        RawGenericParam {
                            index: 0,
                            name: Some("T".to_owned()),
                            kind: RawGenericParamKind::Type,
                        },
                        RawGenericParam {
                            index: 1,
                            name: Some("U".to_owned()),
                            kind: RawGenericParamKind::Type,
                        },
                    ],
                    defaults: vec![
                        RawGenericDefault::Absent,
                        RawGenericDefault::Type(RawTy::Param(0)),
                    ],
                    predicates: vec![RawTraitPredicate {
                        self_ty: RawTy::Param(1),
                        trait_def: RawDefId {
                            crate_num: CrateNum(2),
                            def_index: DefIndex(2),
                            kind: SymExtKind::Trait,
                        },
                        args: Vec::new(),
                    }],
                    ordinary_complete: true,
                    deferred_complete: false,
                }),
                (1, 21) => Some(RawAdtSignature {
                    generics: vec![
                        RawGenericParam {
                            index: 0,
                            name: Some("T".to_owned()),
                            kind: RawGenericParamKind::Type,
                        },
                        RawGenericParam {
                            index: 1,
                            name: Some("U".to_owned()),
                            kind: RawGenericParamKind::Type,
                        },
                    ],
                    defaults: vec![RawGenericDefault::Absent, RawGenericDefault::Absent],
                    predicates: Vec::new(),
                    ordinary_complete: true,
                    deferred_complete: true,
                }),
                (1, 22) => Some(RawAdtSignature {
                    generics: vec![
                        RawGenericParam {
                            index: 0,
                            name: Some("T".to_owned()),
                            kind: RawGenericParamKind::Type,
                        },
                        RawGenericParam {
                            index: 1,
                            name: Some("N".to_owned()),
                            kind: RawGenericParamKind::Const,
                        },
                    ],
                    defaults: vec![RawGenericDefault::Absent, RawGenericDefault::Absent],
                    predicates: Vec::new(),
                    ordinary_complete: false,
                    deferred_complete: false,
                }),
                (1, 23) => Some(RawAdtSignature {
                    generics: vec![RawGenericParam {
                        index: 0,
                        name: Some("T".to_owned()),
                        kind: RawGenericParamKind::Type,
                    }],
                    defaults: vec![RawGenericDefault::Unsupported],
                    predicates: Vec::new(),
                    ordinary_complete: true,
                    deferred_complete: true,
                }),
                _ => None,
            }
        }

        fn expand_proc_macro_derive(
            &self,
            _crate_num: CrateNum,
            _def_index: DefIndex,
            _item_source: &str,
        ) -> Option<String> {
            None
        }

        fn expand_proc_macro_bang(
            &self,
            _crate_num: CrateNum,
            _def_index: DefIndex,
            _input_tokens: &str,
        ) -> Option<String> {
            None
        }

        fn expand_proc_macro_attr(
            &self,
            _crate_num: CrateNum,
            _def_index: DefIndex,
            _attr_args: &str,
            _item_source: &str,
        ) -> Option<String> {
            None
        }
    }

    #[test]
    fn clone_derive_appends_an_impl_with_generated_source_provenance() {
        with_test_crate_files_using_db(
            Database::new(BuiltinDeriveTcx::default()),
            &[(
                "lib.rs",
                "#[derive(Debug, Clone)]\nstruct Db { shared: bool }",
            )],
            |db, root| {
                let items = root.expanded_module_items(db);
                let structs: Vec<_> = items
                    .iter()
                    .filter_map(|symbol| match symbol.data(db) {
                        SymbolData::StructSymbol(StructSymbol::Local(symbol)) => Some(symbol),
                        _ => None,
                    })
                    .collect();
                let impls: Vec<_> = items
                    .iter()
                    .filter_map(|symbol| match symbol.data(db) {
                        SymbolData::ImplSymbol(ImplSymbol::Local(symbol)) => Some(symbol),
                        _ => None,
                    })
                    .collect();

                let (attr_stash, attrs) = structs[0].attrs(db);
                let attr_debug: Vec<_> = attrs
                    .iter()
                    .map(|attr| String::from_utf8_lossy(&attr_stash[attr.args]).into_owned())
                    .collect();

                assert_eq!(structs.len(), 1, "derive must preserve the source item");
                assert_eq!(
                    impls.len(),
                    1,
                    "only represented derives append impls; attrs={attr_debug:?}"
                );
                let signature = impls[0].sig(db);
                let trait_ref = signature.root().value.trait_ref.expect("Clone trait ref");
                assert!(matches!(
                    trait_ref.trait_sym,
                    sage_ir::symbol::TraitSymbol::Ext(_)
                ));
                let ParseSource::Derive(expansion) = impls[0].span(db).source else {
                    panic!("derived impl must retain generated-source provenance");
                };
                assert_eq!(expansion.derive_name(db).text(db), "Clone");
                assert_eq!(expansion.origin(db), structs[0].span(db));
                assert!(
                    expansion
                        .text(db)
                        .is_some_and(|text| text.starts_with("impl ::core::clone::Clone for Db"))
                );
            },
        );
    }

    #[test]
    fn external_clone_contract_proves_the_derived_impl() {
        use sage_ir::check::infer::egraph::VersionedEGraph;
        use sage_ir::check::infer::version::{Universe, Version};
        use sage_ir::check::solve::{
            Assumption, Atom, Goal, GoalQuery, QueryResultData, canonicalize_goal,
        };
        use sage_ir::scope::ScopeSymbol;
        use sage_ir::symbol::TraitSymbol;
        use sage_ir::ty::{TraitRef, Ty};

        with_test_crate_files_using_db(
            Database::new(BuiltinDeriveTcx::default()),
            &[("lib.rs", "#[derive(Clone)]\nstruct Db { shared: bool }")],
            |db, root| {
                let items = root.expanded_module_items(db);
                let strukt = items
                    .iter()
                    .find_map(|symbol| match symbol.data(db) {
                        SymbolData::StructSymbol(StructSymbol::Local(strukt)) => Some(strukt),
                        _ => None,
                    })
                    .expect("derived source struct");
                let clone_trait = items
                    .iter()
                    .find_map(|symbol| match symbol.data(db) {
                        SymbolData::ImplSymbol(ImplSymbol::Local(local_impl)) => {
                            local_impl.sig(db).root().value.trait_ref
                        }
                        _ => None,
                    })
                    .map(|trait_ref| trait_ref.trait_sym)
                    .expect("derived Clone impl");
                assert!(matches!(clone_trait, TraitSymbol::Ext(_)));
                let ScopeSymbol::Crate(krate) = strukt.scope(db) else {
                    panic!("root struct must be crate-scoped")
                };

                let mut stash = Stash::new();
                let egraph = VersionedEGraph::new();
                let args = stash.alloc_slice(&[]);
                let self_ty = stash.alloc(Ty::Adt(strukt.into(), args));
                let trait_args = stash.alloc_slice(&[]);
                let assumptions = stash.alloc_slice::<Assumption>(&[]);
                let canonical = canonicalize_goal(
                    db,
                    &stash,
                    &egraph,
                    Version::ROOT,
                    krate,
                    Universe::ROOT,
                    true,
                    assumptions,
                    Goal::Atom(Atom::TraitImpl {
                        self_ty,
                        trait_ref: TraitRef {
                            trait_sym: clone_trait,
                            args: trait_args,
                        },
                    }),
                );
                let result = GoalQuery::new(db, canonical.data).prove(db);
                assert!(matches!(result.root().value, QueryResultData::Yes { .. }));

                let mut stash = Stash::new();
                let egraph = VersionedEGraph::new();
                let self_ty = stash.alloc(Ty::Bool);
                let trait_args = stash.alloc_slice(&[]);
                let assumptions = stash.alloc_slice::<Assumption>(&[]);
                let canonical = canonicalize_goal(
                    db,
                    &stash,
                    &egraph,
                    Version::ROOT,
                    krate,
                    Universe::ROOT,
                    true,
                    assumptions,
                    Goal::Atom(Atom::TraitImpl {
                        self_ty,
                        trait_ref: TraitRef {
                            trait_sym: clone_trait,
                            args: trait_args,
                        },
                    }),
                );
                let result = GoalQuery::new(db, canonical.data).prove(db);
                assert!(
                    matches!(result.root().value, QueryResultData::Maybe { .. }),
                    "missing external impl enumeration cannot prove that bool: Clone is false"
                );
            },
        );
    }

    #[test]
    fn external_clone_items_and_method_signature_are_typed_metadata() {
        use sage_ir::symbol::Symbol;
        use sage_ir::ty::{MethodReceiver, SolverEligibility, TraitItemDef, Ty};

        with_test_crate_files_using_db(
            Database::new(BuiltinDeriveTcx::default()),
            &[("lib.rs", "#[derive(Clone)]\nstruct Db { shared: bool }")],
            |db, root| {
                let clone_trait = root
                    .expanded_module_items(db)
                    .iter()
                    .find_map(|symbol| match symbol.data(db) {
                        SymbolData::ImplSymbol(ImplSymbol::Local(local_impl)) => {
                            local_impl.sig(db).root().value.trait_ref
                        }
                        _ => None,
                    })
                    .map(|trait_ref| trait_ref.trait_sym)
                    .expect("derived Clone impl");

                let items = clone_trait.items(db).expect("external Clone items");
                let [TraitItemDef::Function(clone_fn)] = items.stash()[items.root().value] else {
                    panic!("Clone must expose exactly one represented function")
                };
                assert_eq!(Symbol::from(clone_fn).name(db).unwrap().0.text(db), "clone");

                let signature = clone_fn.sig(db).expect("external clone signature");
                let value = signature.root().value;
                assert_eq!(
                    value.receiver.map(|receiver| receiver.form),
                    Some(MethodReceiver::Ref {
                        mutability: sage_ir::cst::Mutability::Shared,
                    })
                );
                assert!(matches!(signature.stash()[value.ret], Ty::Param(_)));
                assert_eq!(
                    value.method_candidate_eligibility,
                    SolverEligibility::Eligible
                );
            },
        );
    }

    #[test]
    fn clone_method_call_is_elaborated_to_a_resolved_trait_call() {
        use sage_ir::symbol::{FnSymbol, Symbol};
        use sage_ir::ty::{TraitItemDef, Ty};
        use sage_ir::tytree::{CallDispatch, FieldOwner, PathResolution, TyExprData};

        with_test_crate_files_using_db(
            Database::new(BuiltinDeriveTcx::default()),
            &[(
                "lib.rs",
                "#[derive(Clone)]\nstruct Db { shared: bool }\nstruct DbDropGuard { db: Db }\nimpl DbDropGuard { fn db(&self) -> Db { self.db.clone() } }",
            )],
            |db, root| {
                let mut db_method = None;
                for symbol in root.expanded_module_items(db) {
                    let SymbolData::ImplSymbol(ImplSymbol::Local(local_impl)) = symbol.data(db)
                    else {
                        continue;
                    };
                    let items = local_impl.items(db);
                    for item in &items.stash()[items.root().value] {
                        let TraitItemDef::Function(FnSymbol::Local(function)) = *item else {
                            continue;
                        };
                        if function.name(db).text(db) == "db" {
                            db_method = Some(function);
                        }
                    }
                }
                let db_method = db_method.expect("DbDropGuard::db method");

                // ANCHOR: example_assert_elaborated_clone_body
                let checked = db_method.body(db);
                assert!(checked.diagnostics.is_empty(), "{:?}", checked.diagnostics);
                let (stash, body) = checked.body.open_deref();
                let TyExprData::Block(_, Some(tail)) = stash[body.root].data else {
                    panic!("expected method body block")
                };
                let TyExprData::ResolvedCall(target, arguments) = stash[tail].data else {
                    panic!("method syntax must be consumed by completed IR")
                };
                assert_eq!(
                    Symbol::from(target.function).name(db).unwrap().0.text(db),
                    "clone"
                );
                let CallDispatch::StaticTrait { self_ty, trait_ref } = target.dispatch else {
                    panic!("Clone::clone must be statically trait-dispatched")
                };
                assert_eq!(
                    Symbol::from(trait_ref.trait_sym)
                        .name(db)
                        .unwrap()
                        .0
                        .text(db),
                    "Clone"
                );
                let Ty::Adt(_, _) = stash[self_ty] else {
                    panic!("Clone Self must be Db")
                };

                let [receiver] = &stash[arguments] else {
                    panic!("Clone::clone must receive one elaborated receiver")
                };
                let TyExprData::Ref(field, sage_ir::cst::Mutability::Shared) =
                    stash[*receiver].data
                else {
                    panic!("&self adjustment must be an explicit shared reference")
                };
                let Ty::Ref(_, _, sage_ir::ty::Lifetime::Dummy) = stash[stash[*receiver].ty] else {
                    panic!("synthesized reference must use Lifetime::Dummy")
                };
                let TyExprData::Field(base, resolved_field) = stash[field].data else {
                    panic!("receiver must contain a resolved field")
                };
                assert_eq!(stash[field].ty, self_ty);
                let FieldOwner::Struct(field_owner) = resolved_field.owner else {
                    panic!("Db field must be owned by a struct")
                };
                assert_eq!(
                    Symbol::from(field_owner).name(db).unwrap().0.text(db),
                    "DbDropGuard"
                );
                let TyExprData::Deref(local) = stash[base].data else {
                    panic!("field receiver must contain explicit dereference")
                };
                assert!(matches!(
                    stash[local].data,
                    TyExprData::Path(PathResolution::Local(_))
                ));
                // ANCHOR_END: example_assert_elaborated_clone_body
            },
        );
    }

    #[test]
    fn an_unrelated_resolved_trait_import_does_not_block_method_selection() {
        with_test_crate_files_using_db(
            Database::new(BuiltinDeriveTcx::default()),
            &[(
                "lib.rs",
                "use core::fmt::Debug;\n#[derive(Clone)]\nstruct Db { shared: bool }\nstruct DbDropGuard { db: Db }\nimpl DbDropGuard { fn db(&self) -> Db { self.db.clone() } }",
            )],
            |db, root| {
                let checked = root
                    .expanded_module_items(db)
                    .iter()
                    .find_map(|symbol| {
                        let SymbolData::ImplSymbol(ImplSymbol::Local(local_impl)) = symbol.data(db)
                        else {
                            return None;
                        };
                        let items = local_impl.items(db);
                        items.stash()[items.root().value].iter().find_map(|item| {
                            let sage_ir::ty::TraitItemDef::Function(FnSymbol::Local(function)) =
                                *item
                            else {
                                return None;
                            };
                            (function.name(db).text(db) == "db").then(|| function.body(db))
                        })
                    })
                    .expect("DbDropGuard::db method");
                assert!(checked.diagnostics.is_empty(), "{:?}", checked.diagnostics);
            },
        );
    }

    #[test]
    fn unhandled_inherent_provider_prevents_trait_method_selection() {
        with_test_crate_files_using_db(
            Database::new(BuiltinDeriveTcx::default()),
            &[(
                "lib.rs",
                "#[derive(Clone)]\nstruct Db { shared: bool }\nimpl Db { fn clone(&self) -> Db { Db { shared: self.shared } } }\nstruct DbDropGuard { db: Db }\nimpl DbDropGuard { fn db(&self) -> Db { self.db.clone() } }",
            )],
            |db, root| {
                let checked = root
                    .expanded_module_items(db)
                    .iter()
                    .filter_map(|symbol| {
                        let SymbolData::ImplSymbol(ImplSymbol::Local(local_impl)) = symbol.data(db)
                        else {
                            return None;
                        };
                        Some(local_impl.items(db))
                    })
                    .flat_map(|items| {
                        items.stash()[items.root().value]
                            .iter()
                            .copied()
                            .collect::<Vec<_>>()
                    })
                    .find_map(|item| {
                        let sage_ir::ty::TraitItemDef::Function(FnSymbol::Local(function)) = item
                        else {
                            return None;
                        };
                        (function.name(db).text(db) == "db").then(|| function.body(db))
                    })
                    .expect("DbDropGuard::db method");
                assert!(checked.diagnostics.iter().any(|diagnostic| {
                    diagnostic
                        .message
                        .contains("incomplete candidate information")
                }));
            },
        );
    }

    #[test]
    fn unresolved_item_macro_keeps_method_scope_incomplete() {
        with_test_crate_files_using_db(
            Database::new(BuiltinDeriveTcx::default()),
            &[(
                "lib.rs",
                "unknown_items!();\n#[derive(Clone)]\nstruct Db { shared: bool }\nstruct DbDropGuard { db: Db }\nimpl DbDropGuard { fn db(&self) -> Db { self.db.clone() } }",
            )],
            |db, root| {
                let checked = root
                    .expanded_module_items(db)
                    .iter()
                    .filter_map(|symbol| {
                        let SymbolData::ImplSymbol(ImplSymbol::Local(local_impl)) = symbol.data(db)
                        else {
                            return None;
                        };
                        Some(local_impl.items(db))
                    })
                    .flat_map(|items| {
                        items.stash()[items.root().value]
                            .iter()
                            .copied()
                            .collect::<Vec<_>>()
                    })
                    .find_map(|item| {
                        let sage_ir::ty::TraitItemDef::Function(FnSymbol::Local(function)) = item
                        else {
                            return None;
                        };
                        (function.name(db).text(db) == "db").then(|| function.body(db))
                    })
                    .expect("DbDropGuard::db method");
                assert!(checked.diagnostics.iter().any(|diagnostic| {
                    diagnostic
                        .message
                        .contains("incomplete candidate information")
                }));
            },
        );
    }

    #[test]
    fn explicit_bound_provider_keeps_method_scope_incomplete() {
        with_test_crate_files_using_db(
            Database::new(BuiltinDeriveTcx::default()),
            &[(
                "lib.rs",
                "mod hidden { trait Ext { fn clone(&self) -> super::Db; } }\n#[derive(Clone)]\nstruct Db { shared: bool }\nstruct DbDropGuard { db: Db }\nimpl DbDropGuard { fn db(&self) -> Db where Db: hidden::Ext { self.db.clone() } }",
            )],
            |db, root| {
                let checked = root
                    .expanded_module_items(db)
                    .iter()
                    .filter_map(|symbol| {
                        let SymbolData::ImplSymbol(ImplSymbol::Local(local_impl)) = symbol.data(db)
                        else {
                            return None;
                        };
                        Some(local_impl.items(db))
                    })
                    .flat_map(|items| {
                        items.stash()[items.root().value]
                            .iter()
                            .copied()
                            .collect::<Vec<_>>()
                    })
                    .find_map(|item| {
                        let sage_ir::ty::TraitItemDef::Function(FnSymbol::Local(function)) = item
                        else {
                            return None;
                        };
                        (function.name(db).text(db) == "db").then(|| function.body(db))
                    })
                    .expect("DbDropGuard::db method");
                assert!(checked.diagnostics.iter().any(|diagnostic| {
                    diagnostic
                        .message
                        .contains("incomplete candidate information")
                }));
            },
        );
    }

    #[test]
    fn a_trait_from_another_editions_prelude_is_not_a_provider() {
        with_test_crate_files_using_db(
            Database::new(BuiltinDeriveTcx {
                edition_only_clone_trait: true,
                ..BuiltinDeriveTcx::default()
            }),
            &[(
                "lib.rs",
                "#[derive(Clone)]\nstruct Db { shared: bool }\nimpl std::prelude::rust_2024::EditionClone for Db {}\nstruct DbDropGuard { db: Db }\nimpl DbDropGuard { fn db(&self) -> Db { self.db.clone() } }",
            )],
            |db, root| {
                let checked = root
                    .expanded_module_items(db)
                    .iter()
                    .filter_map(|symbol| {
                        let SymbolData::ImplSymbol(ImplSymbol::Local(local_impl)) = symbol.data(db)
                        else {
                            return None;
                        };
                        Some(local_impl.items(db))
                    })
                    .flat_map(|items| {
                        items.stash()[items.root().value]
                            .iter()
                            .copied()
                            .collect::<Vec<_>>()
                    })
                    .find_map(|item| {
                        let sage_ir::ty::TraitItemDef::Function(FnSymbol::Local(function)) = item
                        else {
                            return None;
                        };
                        (function.name(db).text(db) == "db").then(|| function.body(db))
                    })
                    .expect("DbDropGuard::db method");
                assert!(checked.diagnostics.is_empty(), "{:?}", checked.diagnostics);
            },
        );
    }

    #[test]
    fn a_local_glob_with_trait_reexports_keeps_method_scope_incomplete() {
        with_test_crate_files_using_db(
            Database::new(BuiltinDeriveTcx {
                edition_only_clone_trait: true,
                ..BuiltinDeriveTcx::default()
            }),
            &[(
                "lib.rs",
                "mod providers { pub use std::prelude::rust_2024::EditionClone; }\n\
                 use providers::*;\n\
                 #[derive(Clone)]\nstruct Db { shared: bool }\n\
                 impl std::prelude::rust_2024::EditionClone for Db {}\n\
                 struct DbDropGuard { db: Db }\n\
                 impl DbDropGuard { fn db(&self) -> Db { self.db.clone() } }",
            )],
            |db, root| {
                let checked = root
                    .expanded_module_items(db)
                    .iter()
                    .filter_map(|symbol| match symbol.data(db) {
                        SymbolData::ImplSymbol(ImplSymbol::Local(local_impl)) => {
                            Some(local_impl.items(db))
                        }
                        SymbolData::FnSymbol(_)
                        | SymbolData::StructSymbol(_)
                        | SymbolData::EnumSymbol(_)
                        | SymbolData::VariantSymbol(_)
                        | SymbolData::VariantCtorSymbol(_)
                        | SymbolData::TraitSymbol(_)
                        | SymbolData::TypeAliasSymbol(_)
                        | SymbolData::ConstSymbol(_)
                        | SymbolData::StaticSymbol(_)
                        | SymbolData::ImplSymbol(ImplSymbol::Ext(_))
                        | SymbolData::ModSymbol(_)
                        | SymbolData::MacroDefSymbol(_)
                        | SymbolData::UseSymbol(_)
                        | SymbolData::IntrinsicTypeSymbol(_)
                        | SymbolData::MacroInvocationSymbol(_) => None,
                    })
                    .flat_map(|items| {
                        items.stash()[items.root().value]
                            .iter()
                            .copied()
                            .collect::<Vec<_>>()
                    })
                    .find_map(|item| {
                        let sage_ir::ty::TraitItemDef::Function(FnSymbol::Local(function)) = item
                        else {
                            return None;
                        };
                        (function.name(db).text(db) == "db").then(|| function.body(db))
                    })
                    .expect("DbDropGuard::db method");
                assert!(checked.diagnostics.iter().any(|diagnostic| {
                    diagnostic
                        .message
                        .contains("incomplete candidate information")
                }));
            },
        );
    }

    #[test]
    fn an_explicitly_imported_renamed_external_trait_is_a_provider() {
        with_test_crate_files_using_db(
            Database::new(BuiltinDeriveTcx {
                edition_only_clone_trait: true,
                ..BuiltinDeriveTcx::default()
            }),
            &[(
                "lib.rs",
                "use std::RenamedEditionClone;\n\
                 #[derive(Clone)]\nstruct Db { shared: bool }\n\
                 impl std::prelude::rust_2024::EditionClone for Db {}\n\
                 struct DbDropGuard { db: Db }\n\
                 impl DbDropGuard { fn db(&self) -> Db { self.db.clone() } }",
            )],
            |db, root| {
                let checked = root
                    .expanded_module_items(db)
                    .iter()
                    .filter_map(|symbol| match symbol.data(db) {
                        SymbolData::ImplSymbol(ImplSymbol::Local(local_impl)) => {
                            Some(local_impl.items(db))
                        }
                        SymbolData::FnSymbol(_)
                        | SymbolData::StructSymbol(_)
                        | SymbolData::EnumSymbol(_)
                        | SymbolData::VariantSymbol(_)
                        | SymbolData::VariantCtorSymbol(_)
                        | SymbolData::TraitSymbol(_)
                        | SymbolData::TypeAliasSymbol(_)
                        | SymbolData::ConstSymbol(_)
                        | SymbolData::StaticSymbol(_)
                        | SymbolData::ImplSymbol(ImplSymbol::Ext(_))
                        | SymbolData::ModSymbol(_)
                        | SymbolData::MacroDefSymbol(_)
                        | SymbolData::UseSymbol(_)
                        | SymbolData::IntrinsicTypeSymbol(_)
                        | SymbolData::MacroInvocationSymbol(_) => None,
                    })
                    .flat_map(|items| {
                        items.stash()[items.root().value]
                            .iter()
                            .copied()
                            .collect::<Vec<_>>()
                    })
                    .find_map(|item| {
                        let sage_ir::ty::TraitItemDef::Function(FnSymbol::Local(function)) = item
                        else {
                            return None;
                        };
                        (function.name(db).text(db) == "db").then(|| function.body(db))
                    })
                    .expect("DbDropGuard::db method");
                assert!(
                    checked.diagnostics.iter().any(|diagnostic| {
                        diagnostic.message.contains("method call is ambiguous")
                    }),
                    "{:?}",
                    checked.diagnostics
                );
            },
        );
    }

    #[test]
    fn erroneous_method_receiver_does_not_report_lookup_error() {
        assert_receiver_error_has_no_secondary_lookup("fn f() { missing.clone(); }");
    }

    #[test]
    fn reference_wrapped_receiver_error_does_not_report_lookup_error() {
        assert_receiver_error_has_no_secondary_lookup("fn f() { (&missing).clone(); }");
    }

    #[test]
    fn composite_receiver_error_does_not_report_lookup_error() {
        assert_receiver_error_has_no_secondary_lookup("fn f() { (missing,).clone(); }");
    }

    #[test]
    fn inference_bound_receiver_error_does_not_report_lookup_error() {
        assert_receiver_error_has_no_secondary_lookup(
            "fn f() { let x = (missing,); let y = (x,); y.clone(); }",
        );
    }

    fn assert_receiver_error_has_no_secondary_lookup(source: &str) {
        with_test_crate_files_using_db(
            Database::new(BuiltinDeriveTcx::default()),
            &[("lib.rs", source)],
            |db, root| {
                let checked = root
                    .expanded_module_items(db)
                    .iter()
                    .find_map(|symbol| match symbol.data(db) {
                        SymbolData::FnSymbol(FnSymbol::Local(function)) => Some(function.body(db)),
                        _ => None,
                    })
                    .expect("f body");
                assert_eq!(checked.diagnostics.len(), 1, "{:?}", checked.diagnostics);
                assert!(checked.diagnostics.iter().all(|diagnostic| {
                    !diagnostic.message.contains("method lookup")
                        && !diagnostic.message.contains("candidate information")
                }));
            },
        );
    }

    fn force_db_method(db: &dyn Db, source_file: SourceFile) {
        use sage_ir::symbol::FnSymbol;
        use sage_ir::ty::TraitItemDef;

        let (_, root) = setup_root_module(db, source_file);
        for symbol in root.expanded_module_items(db) {
            let SymbolData::ImplSymbol(ImplSymbol::Local(local_impl)) = symbol.data(db) else {
                continue;
            };
            let items = local_impl.items(db);
            for item in &items.stash()[items.root().value] {
                let TraitItemDef::Function(FnSymbol::Local(function)) = *item else {
                    continue;
                };
                if function.name(db).text(db) == "db" {
                    let checked = function.body(db);
                    assert!(checked.diagnostics.is_empty(), "{:?}", checked.diagnostics);
                    return;
                }
            }
        }
        panic!("DbDropGuard::db method not found");
    }

    #[test]
    fn clone_method_body_has_a_narrow_reusable_semantic_query_trace() {
        let (tcx, tcx_calls) = BuiltinDeriveTcx::tracing();
        let mut database = Database::new(tcx);
        let source_file = database.add_source_file(
            "lib.rs".to_owned(),
            "#[derive(Clone)]\nstruct Db { shared: bool }\nstruct DbDropGuard { db: Db }\nimpl DbDropGuard { fn db(&self) -> Db { self.db.clone() } }"
                .to_owned(),
        );

        database.attach(|db| force_db_method(db, source_file));
        let first_salsa_trace = database.take_query_log();
        let first_tcx_trace = std::mem::take(&mut *tcx_calls.lock().unwrap());
        let mut semantic_calls: Vec<_> = first_tcx_trace
            .iter()
            .filter(|call| {
                call.starts_with("associated_items")
                    || call.starts_with("trait_signature")
                    || call.starts_with("fn_signature")
            })
            .cloned()
            .collect();
        semantic_calls.sort();
        assert_eq!(
            semantic_calls,
            [
                "associated_items(2,2)",
                "associated_items(2,3)",
                "fn_signature(2,7)",
                "trait_signature(2,2)",
                "trait_signature(2,5)",
            ]
        );
        assert!(
            first_tcx_trace.iter().all(|call| !call.contains("body")),
            "callee bodies must not be metadata dependencies: {first_tcx_trace:?}"
        );
        assert!(
            first_tcx_trace.iter().any(|call| call == "item_name(2,7)"),
            "external associated-item names are currently separate metadata dependencies: {first_tcx_trace:?}"
        );
        assert!(
            first_salsa_trace.contains("LocalFnSym < 'db >::body_"),
            "the requested body query must execute: {first_salsa_trace}"
        );

        database.attach(|db| force_db_method(db, source_file));
        let second_salsa_trace = database.take_query_log();
        let second_tcx_trace = std::mem::take(&mut *tcx_calls.lock().unwrap());
        assert!(
            second_tcx_trace.is_empty(),
            "unchanged body reuse must not reread metadata: {second_tcx_trace:?}"
        );
        assert!(
            !second_salsa_trace.contains("LocalFnSym < 'db >::body_"),
            "unchanged body query must be reused: {second_salsa_trace}"
        );
    }

    #[test]
    fn unrelated_body_edit_exposes_current_body_invalidation() {
        use salsa::Setter as _;

        let (tcx, tcx_calls) = BuiltinDeriveTcx::tracing();
        let mut database = Database::new(tcx);
        let before = "#[derive(Clone)]\nstruct Db { shared: bool }\nstruct DbDropGuard { db: Db }\nimpl DbDropGuard { fn db(&self) -> Db { self.db.clone() } }\nfn unrelated() -> bool { false }";
        let source_file = database.add_source_file("lib.rs".to_owned(), before.to_owned());

        database.attach(|db| force_db_method(db, source_file));
        database.take_query_log();
        tcx_calls.lock().unwrap().clear();

        source_file
            .set_text(&mut database)
            .to(before.replace("{ false }", "{ true }"));
        database.attach(|db| force_db_method(db, source_file));
        let salsa_trace = database.take_query_log();
        let metadata_trace = std::mem::take(&mut *tcx_calls.lock().unwrap());

        assert!(
            salsa_trace.contains("LocalFnSym < 'db >::body_"),
            "record this limitation until DbDropGuard::db is reused: {salsa_trace}"
        );
        assert!(
            metadata_trace.iter().all(|call| {
                !call.starts_with("associated_items")
                    && !call.starts_with("trait_signature")
                    && !call.starts_with("fn_signature")
            }),
            "cached callee interfaces should survive the coarse body invalidation: {metadata_trace:?}"
        );
    }

    #[test]
    fn external_adt_default_and_predicate_have_one_narrow_reusable_dependency() {
        use sage_ir::generic_param::GenericParamKind;
        use sage_ir::ty::{BinderExt, SolverEligibility, TraitRef, Ty};

        fn force_pair_signature(db: &dyn Db, source_file: SourceFile) {
            let (_, root) = setup_root_module(db, source_file);
            let function = root
                .expanded_module_items(db)
                .iter()
                .find_map(|symbol| match symbol.data(db) {
                    SymbolData::FnSymbol(FnSymbol::Local(function))
                        if function.name(db).text(db) == "use_pair" =>
                    {
                        Some(function)
                    }
                    SymbolData::FnSymbol(FnSymbol::Local(_))
                    | SymbolData::FnSymbol(FnSymbol::Ext(_))
                    | SymbolData::StructSymbol(_)
                    | SymbolData::EnumSymbol(_)
                    | SymbolData::VariantSymbol(_)
                    | SymbolData::VariantCtorSymbol(_)
                    | SymbolData::TraitSymbol(_)
                    | SymbolData::TypeAliasSymbol(_)
                    | SymbolData::ConstSymbol(_)
                    | SymbolData::StaticSymbol(_)
                    | SymbolData::ImplSymbol(_)
                    | SymbolData::ModSymbol(_)
                    | SymbolData::MacroDefSymbol(_)
                    | SymbolData::UseSymbol(_)
                    | SymbolData::IntrinsicTypeSymbol(_)
                    | SymbolData::MacroInvocationSymbol(_) => None,
                })
                .expect("use_pair function");
            let signature = function.sig(db);
            assert!(
                signature
                    .iter_symbols()
                    .all(|generic| generic.kind(db) != GenericParamKind::Const)
            );
            let (stash, binder) = signature.open();
            let [parameter] = &stash[binder.value.params] else {
                panic!("expected one parameter")
            };
            let Ty::Adt(_, arguments) = stash[*parameter] else {
                panic!("expected external Pair")
            };
            let [first, second] = &stash[arguments] else {
                panic!("defaulted Pair should have two arguments")
            };
            assert_eq!(stash[*first], Ty::Bool);
            assert_eq!(stash[*second], Ty::Bool);

            assert_eq!(
                binder.value.parameter_env.solver_eligibility,
                SolverEligibility::Eligible
            );
            let [predicate] = &stash[binder.value.parameter_env.where_clauses] else {
                panic!("Pair's ordinary predicate should enter the function contract")
            };
            assert_eq!(stash[predicate.self_ty], Ty::Bool);
            let TraitRef { trait_sym, args } = predicate.trait_ref;
            assert!(stash[args].is_empty());
            assert!(matches!(trait_sym, sage_ir::symbol::TraitSymbol::Ext(_)));
        }

        let (tcx, calls) = BuiltinDeriveTcx::tracing();
        let mut database = Database::new(tcx);
        let source_file = database.add_source_file(
            "lib.rs".to_owned(),
            "fn use_pair(value: std::Pair<bool>) { let _ = value; }".to_owned(),
        );

        database.attach(|db| force_pair_signature(db, source_file));
        let first_calls = std::mem::take(&mut *calls.lock().unwrap());
        let semantic_calls: Vec<_> = first_calls
            .iter()
            .filter(|call| {
                call.starts_with("adt_signature")
                    || call.starts_with("associated_items")
                    || call.starts_with("fn_signature")
                    || call.starts_with("trait_signature")
            })
            .cloned()
            .collect();
        assert_eq!(semantic_calls, ["adt_signature(1,20)"]);

        database.attach(|db| force_pair_signature(db, source_file));
        let second_calls = std::mem::take(&mut *calls.lock().unwrap());
        assert!(
            second_calls.is_empty(),
            "warm signature reuse reread metadata: {second_calls:?}"
        );
    }

    #[test]
    fn external_adt_missing_required_argument_is_not_inferred() {
        use sage_ir::external_syms::{ApplyExternalAdtError, apply_external_adt_signature};
        use sage_ir::symbol::SymExt;
        use sage_ir::ty::Ty;

        let database = Database::new(BuiltinDeriveTcx::default());
        database.attach(|db| {
            let required = SymExt::new(db, CrateNum(1), DefIndex(21), SymExtKind::Struct);
            let mut stash = Stash::new();
            let supplied = stash.alloc(Ty::Bool);
            assert!(matches!(
                apply_external_adt_signature(db, &mut stash, required, &[supplied]),
                Err(ApplyExternalAdtError::IncorrectTypeArgumentCount)
            ));
        });
    }

    #[test]
    fn external_adt_keeps_ordinary_and_deferred_completeness_separate() {
        use sage_ir::external_syms::external_adt_signature;
        use sage_ir::symbol::SymExt;
        use sage_ir::ty::SolverEligibility;

        let database = Database::new(BuiltinDeriveTcx::default());
        database.attach(|db| {
            let pair = SymExt::new(db, CrateNum(1), DefIndex(20), SymExtKind::Struct);
            let signature = external_adt_signature(db, pair).expect("Pair signature metadata");
            let (_, binder) = signature.open();
            assert!(binder.value.ordinary_complete);
            assert!(!binder.value.deferred_complete);
            assert_eq!(
                binder.value.parameter_env.solver_eligibility,
                SolverEligibility::Eligible
            );
        });
    }

    #[test]
    fn external_adt_predicates_in_struct_fields_are_retained() {
        use sage_ir::ty::{SolverEligibility, Ty};

        let mut database = Database::new(BuiltinDeriveTcx::default());
        let source_file = database.add_source_file(
            "lib.rs".to_owned(),
            "struct Holder { pair: std::Pair<bool> }".to_owned(),
        );
        database.attach(|db| {
            let (_, root) = setup_root_module(db, source_file);
            let holder = root
                .expanded_module_items(db)
                .iter()
                .find_map(|symbol| match symbol.data(db) {
                    SymbolData::StructSymbol(sage_ir::symbol::StructSymbol::Local(local))
                        if local.name(db).text(db) == "Holder" =>
                    {
                        Some(local)
                    }
                    SymbolData::StructSymbol(sage_ir::symbol::StructSymbol::Local(_))
                    | SymbolData::StructSymbol(sage_ir::symbol::StructSymbol::Ext(_))
                    | SymbolData::FnSymbol(_)
                    | SymbolData::EnumSymbol(_)
                    | SymbolData::VariantSymbol(_)
                    | SymbolData::VariantCtorSymbol(_)
                    | SymbolData::TraitSymbol(_)
                    | SymbolData::TypeAliasSymbol(_)
                    | SymbolData::ConstSymbol(_)
                    | SymbolData::StaticSymbol(_)
                    | SymbolData::ImplSymbol(_)
                    | SymbolData::ModSymbol(_)
                    | SymbolData::MacroDefSymbol(_)
                    | SymbolData::UseSymbol(_)
                    | SymbolData::IntrinsicTypeSymbol(_)
                    | SymbolData::MacroInvocationSymbol(_) => None,
                })
                .expect("Holder struct");
            let fields = holder.fields(db);
            let (stash, fields) = fields.open();
            assert_eq!(
                fields.parameter_env.solver_eligibility,
                SolverEligibility::Eligible
            );
            let [predicate] = &stash[fields.parameter_env.where_clauses] else {
                panic!("field type predicate should remain attached to the fields")
            };
            assert_eq!(stash[predicate.self_ty], Ty::Bool);
        });
    }

    #[test]
    fn external_adt_with_unrepresented_const_identity_is_not_formed() {
        use sage_ir::external_syms::{ApplyExternalAdtError, apply_external_adt_signature};
        use sage_ir::symbol::SymExt;
        use sage_ir::ty::Ty;

        let database = Database::new(BuiltinDeriveTcx::default());
        database.attach(|db| {
            let const_generic = SymExt::new(db, CrateNum(1), DefIndex(22), SymExtKind::Struct);
            let mut stash = Stash::new();
            let supplied = stash.alloc(Ty::Bool);
            assert!(matches!(
                apply_external_adt_signature(db, &mut stash, const_generic, &[supplied]),
                Err(ApplyExternalAdtError::MetadataUnavailable)
            ));
        });
    }

    #[test]
    fn unsupported_external_default_is_metadata_unavailable_not_an_arity_error() {
        use sage_ir::external_syms::{ApplyExternalAdtError, apply_external_adt_signature};
        use sage_ir::symbol::SymExt;

        let database = Database::new(BuiltinDeriveTcx::default());
        database.attach(|db| {
            let unsupported_default =
                SymExt::new(db, CrateNum(1), DefIndex(23), SymExtKind::Struct);
            let mut stash = Stash::new();
            assert!(matches!(
                apply_external_adt_signature(db, &mut stash, unsupported_default, &[]),
                Err(ApplyExternalAdtError::MetadataUnavailable)
            ));
            let supplied = stash.alloc(sage_ir::ty::Ty::Bool);
            assert!(
                apply_external_adt_signature(db, &mut stash, unsupported_default, &[supplied])
                    .is_ok(),
                "an explicit argument does not need the unsupported default"
            );
        });
    }

    #[test]
    fn unsupported_tuple_clone_derive_does_not_append_invalid_impl_source() {
        with_test_crate_files_using_db(
            Database::new(BuiltinDeriveTcx::default()),
            &[("lib.rs", "#[derive(Clone)]\nstruct Pair(bool, bool);")],
            |db, root| {
                assert!(root.expanded_module_items(db).iter().all(|symbol| {
                    !matches!(
                        symbol.data(db),
                        SymbolData::ImplSymbol(ImplSymbol::Local(_))
                    )
                }));
            },
        );
    }

    #[test]
    fn active_attribute_prevents_derive_from_publishing_a_definite_impl() {
        use sage_ir::local_syms::impls::local_impl_candidates;
        use sage_ir::scope::local_crate;
        use sage_ir::symbol::{ModSymbol, SymExt, TraitSymbol};

        with_test_crate_files_using_db(
            Database::new(BuiltinDeriveTcx::default()),
            &[(
                "lib.rs",
                "#[cfg(any())]\n#[derive(Clone)]\nstruct Db { shared: bool }",
            )],
            |db, root| {
                assert!(root.expanded_module_items(db).iter().all(|symbol| {
                    !matches!(
                        symbol.data(db),
                        SymbolData::ImplSymbol(ImplSymbol::Local(_))
                    )
                }));

                let ModSymbol::Local(root) = root else {
                    unreachable!()
                };
                let clone_trait =
                    TraitSymbol::Ext(SymExt::new(db, CrateNum(2), DefIndex(2), SymExtKind::Trait));
                let candidates = local_impl_candidates(db, local_crate(db, root), clone_trait);
                assert!(candidates.impls.is_empty());
                assert!(
                    !candidates.complete,
                    "the unexpanded active attribute can still change the item set"
                );
            },
        );
    }

    #[test]
    fn duplicate_derive_occurrences_have_distinct_generated_source_identity() {
        with_test_crate_files_using_db(
            Database::new(BuiltinDeriveTcx::default()),
            &[(
                "lib.rs",
                "#[derive(Clone, Clone)]\nstruct Db { shared: bool }",
            )],
            |db, root| {
                let expansions: Vec<_> = root
                    .expanded_module_items(db)
                    .iter()
                    .filter_map(|symbol| match symbol.data(db) {
                        SymbolData::ImplSymbol(ImplSymbol::Local(impl_sym)) => {
                            let ParseSource::Derive(expansion) = impl_sym.span(db).source else {
                                return None;
                            };
                            Some(expansion)
                        }
                        _ => None,
                    })
                    .collect();
                assert_eq!(expansions.len(), 2);
                assert_ne!(expansions[0], expansions[1]);
                assert_eq!(expansions[0].derive_index(db), 0);
                assert_eq!(expansions[1].derive_index(db), 1);
            },
        );
    }

    #[test]
    fn unresolved_derive_keeps_trait_keyed_impl_search_incomplete() {
        use sage_ir::local_syms::impls::local_impl_candidates;
        use sage_ir::scope::local_crate;
        use sage_ir::symbol::{ModSymbol, TraitSymbol};

        with_test_crate(
            "#[derive(custom::Custom)]\nstruct Db { shared: bool }\ntrait Marker {}",
            |db, root| {
                let marker: TraitSymbol<'_> = root
                    .expanded_module_items(db)
                    .iter()
                    .find_map(|symbol| match symbol.data(db) {
                        SymbolData::TraitSymbol(TraitSymbol::Local(marker)) => Some(marker.into()),
                        _ => None,
                    })
                    .expect("Marker trait");
                let ModSymbol::Local(root) = root else {
                    unreachable!()
                };
                let candidates = local_impl_candidates(db, local_crate(db, root), marker);
                assert!(candidates.impls.is_empty());
                assert!(
                    !candidates.complete,
                    "an ignored derive must not make ground impl search exhaustive"
                );
            },
        );
    }

    #[test]
    fn macro_generated_unresolved_derive_keeps_impl_search_incomplete() {
        use sage_ir::local_syms::impls::local_impl_candidates;
        use sage_ir::scope::local_crate;
        use sage_ir::symbol::{ModSymbol, TraitSymbol};

        with_test_crate(
            "macro_rules! make { () => { #[derive(Custom)] struct Generated { value: bool } } }\nmake!();\ntrait Marker {}",
            |db, root| {
                let marker: TraitSymbol<'_> = root
                    .expanded_module_items(db)
                    .iter()
                    .find_map(|symbol| match symbol.data(db) {
                        SymbolData::TraitSymbol(TraitSymbol::Local(marker)) => Some(marker.into()),
                        _ => None,
                    })
                    .expect("Marker trait");
                let ModSymbol::Local(root) = root else {
                    unreachable!()
                };
                let candidates = local_impl_candidates(db, local_crate(db, root), marker);
                assert!(!candidates.complete);
            },
        );
    }

    #[test]
    fn moving_source_item_preserves_derive_expansion_identity() {
        use salsa::Setter as _;
        use salsa::plumbing::AsId as _;

        let mut db = Database::new(BuiltinDeriveTcx::default());
        let source = "#[derive(Clone)]\nstruct Db { shared: bool }";
        let file = db.add_source_file("lib.rs".to_owned(), source.to_owned());
        let expansion = |db: &Database| {
            let (_, root) = setup_root_module(db, file);
            root.expanded_module_items(db)
                .iter()
                .find_map(|symbol| match symbol.data(db) {
                    SymbolData::ImplSymbol(ImplSymbol::Local(impl_sym)) => {
                        let ParseSource::Derive(expansion) = impl_sym.span(db).source else {
                            return None;
                        };
                        Some((expansion.as_id(), expansion.origin(db).start))
                    }
                    _ => None,
                })
                .expect("derived impl")
        };

        let before = db.attach(expansion);
        file.set_text(&mut db).to(format!("\n{source}"));
        let after = db.attach(expansion);
        assert_eq!(before.0, after.0, "offset changes must preserve identity");
        assert_ne!(before.1, after.1, "origin coordinates must still update");
    }
}

#[cfg(test)]
mod trait_system_tests {
    use super::*;
    use sage_ir::local_syms::impls::local_impls;
    use sage_ir::scope::ScopeSymbol;
    use sage_ir::symbol::{Symbol, SymbolData, TraitSymbol};
    use sage_ir::ty::{Lifetime, SolverEligibility, Ty};

    #[test]
    fn trait_and_impl_signatures_share_complete_generic_binders() {
        with_test_crate(
            "trait Bound {}\ntrait PairBound<T> where T: Bound {}\nimpl<T: Bound> PairBound<T> for (T, T) {}",
            |db, root| {
                let items = root.expanded_module_items(db);
                let pair_trait = items
                    .iter()
                    .find_map(|symbol| match symbol.data(db) {
                        SymbolData::TraitSymbol(TraitSymbol::Local(local))
                            if local.name(db).text(db) == "PairBound" =>
                        {
                            Some(local)
                        }
                        _ => None,
                    })
                    .unwrap();
                let signature = pair_trait.sig(db);
                let (stash, binder) = signature.open();
                assert_eq!(stash[binder.generics].len(), 2); // Self, T
                assert_eq!(binder.value.solver_eligibility, SolverEligibility::Eligible);
                assert_eq!(stash[binder.value.where_clauses].len(), 1);

                let ScopeSymbol::Crate(krate) = pair_trait.scope(db) else {
                    panic!("root item should be crate-scoped")
                };
                let impls = local_impls(db, krate);
                assert_eq!(impls.len(), 1);
                let impl_signature = impls[0].sig(db);
                let (stash, binder) = impl_signature.open();
                assert_eq!(stash[binder.generics].len(), 1);
                assert_eq!(binder.value.solver_eligibility, SolverEligibility::Eligible);
                let Ty::Tuple(elements) = stash[binder.value.self_ty] else {
                    panic!("expected tuple impl self type")
                };
                let elements = &stash[elements];
                assert_eq!(elements.len(), 2);
                assert_eq!(stash[elements[0]], stash[elements[1]]);
            },
        );
    }

    #[test]
    fn lifetime_generic_impl_uses_dummy_and_remains_solver_eligible() {
        with_test_crate(
            "trait Bound {}\nimpl<'a, T> Bound for &'a T {}",
            |db, root| {
                let items = root.expanded_module_items(db);
                let local_trait = items
                    .iter()
                    .find_map(|symbol| match symbol.data(db) {
                        SymbolData::TraitSymbol(TraitSymbol::Local(local)) => Some(local),
                        _ => None,
                    })
                    .unwrap();
                let ScopeSymbol::Crate(krate) = local_trait.scope(db) else {
                    panic!("root item should be crate-scoped")
                };
                let signature = local_impls(db, krate)[0].sig(db);
                assert_eq!(
                    signature.root().value.solver_eligibility,
                    SolverEligibility::Eligible
                );
                let (stash, binder) = signature.open();
                assert!(matches!(
                    stash[binder.value.self_ty],
                    Ty::Ref(_, _, Lifetime::Dummy)
                ));
            },
        );
    }

    #[test]
    fn deferred_trait_headers_are_preserved_and_ineligible() {
        with_test_crate(
            "trait Bound {}\ntrait Super: Bound {}\ntrait Defaulted<T = i32> {}",
            |db, root| {
                let items = root.expanded_module_items(db);
                for name in ["Super", "Defaulted"] {
                    let local = items
                        .iter()
                        .find_map(|symbol| match symbol.data(db) {
                            SymbolData::TraitSymbol(TraitSymbol::Local(local))
                                if local.name(db).text(db) == name =>
                            {
                                Some(local)
                            }
                            _ => None,
                        })
                        .unwrap();
                    assert_eq!(
                        local.sig(db).root().value.solver_eligibility,
                        SolverEligibility::Unsupported,
                        "{name} must not become an unconditional clause",
                    );
                }
            },
        );
    }

    #[test]
    fn negative_impl_is_ineligible_but_all_reference_lifetimes_are_dummy() {
        with_test_crate(
            "trait Bound {}\nimpl !Bound for bool {}\nimpl Bound for &i32 {}\nimpl Bound for &'static bool {}",
            |db, root| {
                let items = root.expanded_module_items(db);
                let local_trait = items
                    .iter()
                    .find_map(|symbol| match symbol.data(db) {
                        SymbolData::TraitSymbol(TraitSymbol::Local(local)) => Some(local),
                        _ => None,
                    })
                    .unwrap();
                let ScopeSymbol::Crate(krate) = local_trait.scope(db) else {
                    panic!("root item should be crate-scoped")
                };
                let impls = local_impls(db, krate);
                let eligibility: Vec<_> = impls
                    .iter()
                    .map(|local| local.sig(db).root().value.solver_eligibility)
                    .collect();
                assert_eq!(
                    eligibility,
                    [
                        SolverEligibility::Unsupported,
                        SolverEligibility::Eligible,
                        SolverEligibility::Eligible,
                    ]
                );
                for local in &impls[1..] {
                    let signature = local.sig(db);
                    let (stash, binder) = signature.open();
                    assert!(matches!(
                        stash[binder.value.self_ty],
                        Ty::Ref(_, _, Lifetime::Dummy)
                    ));
                }
            },
        );
    }

    #[test]
    fn function_and_adt_signatures_retain_parameter_environments() {
        use sage_ir::symbol::{EnumSymbol, FnSymbol, StructSymbol};

        with_test_crate(
            "trait Bound {}\nfn requires<T: Bound>() {}\nstruct Container<T> where T: Bound { value: T }\nenum Choice<T> where T: Bound { Value { value: T } }",
            |db, root| {
                let items = root.expanded_module_items(db);
                let function = items
                    .iter()
                    .find_map(|symbol| match symbol.data(db) {
                        SymbolData::FnSymbol(FnSymbol::Local(local)) => Some(local),
                        _ => None,
                    })
                    .unwrap();
                let function_sig = function.sig(db);
                let (stash, binder) = function_sig.open();
                assert_eq!(stash[binder.value.parameter_env.where_clauses].len(), 1);
                assert_eq!(
                    binder.value.parameter_env.solver_eligibility,
                    SolverEligibility::Eligible
                );

                let strukt = items
                    .iter()
                    .find_map(|symbol| match symbol.data(db) {
                        SymbolData::StructSymbol(StructSymbol::Local(local)) => Some(local),
                        _ => None,
                    })
                    .unwrap();
                let struct_sig = strukt.sig(db);
                let (stash, binder) = struct_sig.open();
                assert_eq!(stash[binder.value.parameter_env.where_clauses].len(), 1);
                assert_eq!(
                    binder.value.parameter_env.solver_eligibility,
                    SolverEligibility::Eligible
                );

                let enumeration = items
                    .iter()
                    .find_map(|symbol| match symbol.data(db) {
                        SymbolData::EnumSymbol(EnumSymbol::Local(local)) => Some(local),
                        _ => None,
                    })
                    .unwrap();
                let enum_sig = enumeration.sig(db);
                let (stash, binder) = enum_sig.open();
                assert_eq!(stash[binder.value.parameter_env.where_clauses].len(), 1);
                assert_eq!(
                    binder.value.parameter_env.solver_eligibility,
                    SolverEligibility::Eligible
                );
            },
        );
    }

    #[test]
    fn explicit_elided_and_static_lifetimes_collapse_to_dummy() {
        use sage_ir::symbol::FnSymbol;

        with_test_crate(
            "fn refs<'a, T: 'a>(a: &'a T, b: &T) -> &'static T { a }",
            |db, root| {
                let [symbol] = root.expanded_module_items(db) else {
                    panic!("expected exactly one function")
                };
                let function = match symbol.data(db) {
                    SymbolData::FnSymbol(FnSymbol::Local(local)) => local,
                    SymbolData::FnSymbol(FnSymbol::Ext(_))
                    | SymbolData::StructSymbol(_)
                    | SymbolData::EnumSymbol(_)
                    | SymbolData::VariantSymbol(_)
                    | SymbolData::VariantCtorSymbol(_)
                    | SymbolData::TraitSymbol(_)
                    | SymbolData::TypeAliasSymbol(_)
                    | SymbolData::ConstSymbol(_)
                    | SymbolData::StaticSymbol(_)
                    | SymbolData::ImplSymbol(_)
                    | SymbolData::ModSymbol(_)
                    | SymbolData::MacroDefSymbol(_)
                    | SymbolData::IntrinsicTypeSymbol(_)
                    | SymbolData::MacroInvocationSymbol(_)
                    | SymbolData::UseSymbol(_) => panic!("expected a local function"),
                };
                let signature = function.sig(db);
                let (stash, binder) = signature.open();
                assert_eq!(
                    binder.value.parameter_env.solver_eligibility,
                    SolverEligibility::Eligible
                );
                for ty in stash[binder.value.params]
                    .iter()
                    .copied()
                    .chain(std::iter::once(binder.value.ret))
                {
                    assert!(matches!(stash[ty], Ty::Ref(_, _, Lifetime::Dummy)));
                }
            },
        );
    }

    #[test]
    fn associated_items_reuse_owner_generics_and_bodies_resolve_fields() {
        use sage_ir::local_syms::LocalAssociatedOwner;
        use sage_ir::symbol::{FnSymbol, StructSymbol};
        use sage_ir::ty::{MethodReceiver, TraitItemDef};
        use sage_ir::tytree::{FieldOwner, PathResolution, TyExprData};

        with_test_crate(
            "struct Wrap<T> { value: T }\nimpl<T> Wrap<T> { fn get(&self) -> T { self.value } }",
            |db, root| {
                let [struct_symbol, impl_symbol] = root.expanded_module_items(db) else {
                    panic!("expected a struct and an impl")
                };
                let SymbolData::StructSymbol(StructSymbol::Local(strukt)) = struct_symbol.data(db)
                else {
                    panic!("expected a local struct")
                };
                let SymbolData::ImplSymbol(sage_ir::symbol::ImplSymbol::Local(local_impl)) =
                    impl_symbol.data(db)
                else {
                    panic!("expected a local impl")
                };

                let items = local_impl.items(db);
                let (items_stash, binder) = items.open();
                let [TraitItemDef::Function(FnSymbol::Local(method))] = &items_stash[binder.value]
                else {
                    panic!("expected one local method")
                };
                assert_eq!(
                    method.owner(db),
                    Some(LocalAssociatedOwner::Impl(local_impl))
                );
                let method_symbol = sage_ir::local_syms::LocalModItemSym::Function(*method);
                assert_eq!(method_symbol.absolute_span(db), method.span(db));

                let signature = method.sig(db);
                let (sig_stash, signature) = signature.open();
                assert_eq!(sig_stash[signature.generics].len(), 1);
                assert!(sig_stash[signature.value.params].is_empty());
                let receiver = signature.value.receiver.expect("expected receiver");
                assert_eq!(
                    receiver.form,
                    MethodReceiver::Ref {
                        mutability: sage_ir::cst::Mutability::Shared
                    }
                );
                let Ty::Adt(owner, arguments) = sig_stash[receiver.owner_self_ty] else {
                    panic!("expected the opened impl self type")
                };
                assert_eq!(owner, strukt.into());
                let [argument] = &sig_stash[arguments] else {
                    panic!("expected one owner argument")
                };
                assert_eq!(sig_stash[*argument], sig_stash[signature.value.ret]);

                let (method_cst_stash, method_cst) = method.cst(db).open_deref();
                assert_eq!(
                    sage_ir::diagnostic::Span::Relative(method_symbol, method_cst.span).resolve(db),
                    method.span(db)
                );
                let [receiver_cst] = &method_cst_stash[method_cst.params] else {
                    panic!("expected one receiver parameter")
                };
                assert_eq!(
                    receiver_cst
                        .name
                        .expect("receiver must bind `self`")
                        .text(db),
                    "self"
                );

                let checked = method.body(db);
                assert!(checked.diagnostics.is_empty(), "{:?}", checked.diagnostics);
                let (body_stash, body) = checked.body.open_deref();
                let TyExprData::Block(_, Some(tail)) = body_stash[body.root].data else {
                    panic!("expected a block with a tail expression")
                };
                let TyExprData::Field(base, field) = body_stash[tail].data else {
                    panic!("expected a resolved field access")
                };
                assert_eq!(field.owner, FieldOwner::Struct(StructSymbol::Local(strukt)));
                assert_eq!(field.index, 0);
                let TyExprData::Deref(receiver_expr) = body_stash[base].data else {
                    panic!("expected explicit receiver dereference")
                };
                assert!(matches!(
                    body_stash[receiver_expr].data,
                    TyExprData::Path(PathResolution::Local(_))
                ));
            },
        );
    }

    // Evidence: ARC-A1, PAR-A2, SYM-A2, INC-A2, INC-A3.
    #[test]
    fn detail_edits_preserve_local_symbol_query_keys() {
        use salsa::Setter as _;

        struct Case {
            label: &'static str,
            before: &'static str,
            after: &'static str,
            item_index: usize,
        }

        fn force_observer(db: &dyn Db, source_file: SourceFile, item_index: usize) {
            let (_, root) = setup_root_module(db, source_file);
            let ModSymbol::Local(root) = root else {
                panic!("expected a local root module")
            };
            let item = root
                .unexpanded_items(db)
                .get(item_index)
                .copied()
                .expect("expected observed item");
            let symbol: Symbol<'_> = item.into();
            observe_symbol_identity(db, ObservedSymbol::new(db, symbol));
        }

        let cases = [
            Case {
                label: "function",
                before: "fn item() -> bool { false }",
                after: "fn item() -> bool { true }",
                item_index: 0,
            },
            Case {
                label: "struct",
                before: "struct Item { value: bool }",
                after: "struct Item { value: u32 }",
                item_index: 0,
            },
            Case {
                label: "enum",
                before: "enum Item { Value(bool) }",
                after: "enum Item { Value(u32) }",
                item_index: 0,
            },
            Case {
                label: "trait",
                before: "trait Item { fn value() -> bool; }",
                after: "trait Item { fn value() -> u32; }",
                item_index: 0,
            },
            Case {
                label: "impl",
                before: "struct Item; impl Item { fn value() -> bool { false } }",
                after: "struct Item; impl Item { fn value() -> bool { true } }",
                item_index: 1,
            },
            Case {
                label: "type alias",
                before: "type Item = bool;",
                after: "type Item = u32;",
                item_index: 0,
            },
            Case {
                label: "const",
                before: "const ITEM: bool = false;",
                after: "const ITEM: bool = true;",
                item_index: 0,
            },
            Case {
                label: "static",
                before: "static ITEM: bool = false;",
                after: "static ITEM: bool = true;",
                item_index: 0,
            },
            Case {
                label: "module",
                before: "#[doc = \"before\"] mod item {}",
                after: "#[doc = \"after\"] mod item {}",
                item_index: 0,
            },
            Case {
                label: "use",
                before: "use crate::before;",
                after: "use crate::after;",
                item_index: 0,
            },
            Case {
                label: "macro definition",
                before: "macro_rules! item { () => { struct Before; } }",
                after: "macro_rules! item { () => { struct After; } }",
                item_index: 0,
            },
            Case {
                label: "macro invocation",
                before: "item!(before);",
                after: "item!(after);",
                item_index: 0,
            },
        ];

        for case in cases {
            let mut database = Database::default();
            let source_file = database.add_source_file("lib.rs".to_owned(), case.before.to_owned());
            database.attach(|db| force_observer(db, source_file, case.item_index));
            database.take_query_log();

            database.attach(|db| force_observer(db, source_file, case.item_index));
            assert_eq!(
                database.take_query_log(),
                "",
                "{} must have a warm observer",
                case.label
            );

            source_file
                .set_text(&mut database)
                .to(case.after.to_owned());
            database.attach(|db| force_observer(db, source_file, case.item_index));
            let edit_trace = database.take_query_log();
            assert!(
                !edit_trace.contains("observe_symbol_identity"),
                "{} detail edit reminted its symbol:\n{edit_trace}",
                case.label
            );
        }
    }

    // Evidence: ARC-A1, PAR-A2, SYM-A2, INC-A2, INC-A3.
    #[test]
    fn enum_detail_edits_preserve_variant_query_keys() {
        use sage_ir::local_syms::enums::enum_variants;
        use sage_ir::symbol::EnumSymbol;
        use salsa::Setter as _;

        fn force_variant_observers(db: &dyn Db, source_file: SourceFile) {
            let (_, root) = setup_root_module(db, source_file);
            let [symbol] = root.expanded_module_items(db) else {
                panic!("expected one enum")
            };
            let SymbolData::EnumSymbol(EnumSymbol::Local(local_enum)) = symbol.data(db) else {
                panic!("expected a local enum")
            };
            for symbol in enum_variants(db, local_enum) {
                if let SymbolData::VariantSymbol(sage_ir::symbol::VariantSymbol::Local(variant)) =
                    symbol.data(db)
                {
                    assert!(variant.has_fields(db));
                }
                observe_symbol_identity(db, ObservedSymbol::new(db, *symbol));
            }
        }

        let mut database = Database::default();
        let source_file =
            database.add_source_file("lib.rs".to_owned(), "enum Item { Value(bool) }".to_owned());
        database.attach(|db| force_variant_observers(db, source_file));
        database.take_query_log();

        source_file
            .set_text(&mut database)
            .to("enum Item { Value(u32) }".to_owned());
        database.attach(|db| force_variant_observers(db, source_file));
        let edit_trace = database.take_query_log();
        assert!(
            !edit_trace.contains("observe_symbol_identity"),
            "enum detail edit reminted a variant or constructor:\n{edit_trace}"
        );
    }

    // Evidence: SPAN-A1, SPAN-A3, ARC-A1, SYM-A2, INC-A2, INC-A3.
    #[test]
    fn moving_associated_items_preserves_symbol_query_keys() {
        use sage_ir::ty::TraitItemDef;
        use salsa::Setter as _;

        fn force_target_observer(db: &dyn Db, source_file: SourceFile) {
            let (_, root) = setup_root_module(db, source_file);
            let items = root
                .expanded_module_items(db)
                .iter()
                .find_map(|symbol| match symbol.data(db) {
                    SymbolData::TraitSymbol(TraitSymbol::Local(owner)) => Some(owner.items(db)),
                    SymbolData::ImplSymbol(sage_ir::symbol::ImplSymbol::Local(owner)) => {
                        Some(owner.items(db))
                    }
                    _ => None,
                })
                .expect("expected a trait or impl owner");
            let (stash, binder) = items.open();
            let symbol = stash[binder.value]
                .iter()
                .find_map(|item| {
                    let symbol = match *item {
                        TraitItemDef::Function(symbol) => Symbol::from(symbol),
                        TraitItemDef::Type(symbol) => Symbol::from(symbol),
                        TraitItemDef::Const(symbol) => Symbol::from(symbol),
                    };
                    let name = symbol.name(db)?.0.text(db);
                    name.eq_ignore_ascii_case("target").then_some(symbol)
                })
                .expect("expected a target associated item");
            observe_symbol_identity(db, ObservedSymbol::new(db, symbol));
        }

        let cases = [
            (
                "trait function",
                "trait HasItems { fn earlier(); fn target(&self) -> bool { true } }",
                "trait HasItems { fn earlier(); fn added(); fn target(&self) -> bool { true } }",
            ),
            (
                "trait type",
                "trait HasItems { type Earlier; type Target; }",
                "trait HasItems { type Earlier; type Added; type Target; }",
            ),
            (
                "trait const",
                "trait HasItems { const EARLIER: bool; const TARGET: bool; }",
                "trait HasItems { const EARLIER: bool; const ADDED: bool; const TARGET: bool; }",
            ),
            (
                "impl function",
                "struct HasItems; impl HasItems { fn earlier() {} fn target(&self) -> bool { true } }",
                "struct HasItems; impl HasItems { fn earlier() {} fn added() {} fn target(&self) -> bool { true } }",
            ),
            (
                "impl type",
                "struct HasItems; impl HasItems { type Earlier = bool; type Target = bool; }",
                "struct HasItems; impl HasItems { type Earlier = bool; type Added = bool; type Target = bool; }",
            ),
            (
                "impl const",
                "struct HasItems; impl HasItems { const EARLIER: bool = true; const TARGET: bool = true; }",
                "struct HasItems; impl HasItems { const EARLIER: bool = true; const ADDED: bool = true; const TARGET: bool = true; }",
            ),
            (
                "trait function reorder",
                "trait HasItems { fn earlier(); fn target(&self) -> bool { true } }",
                "trait HasItems { fn target(&self) -> bool { true } fn earlier(); }",
            ),
            (
                "trait type reorder",
                "trait HasItems { type Earlier; type Target; }",
                "trait HasItems { type Target; type Earlier; }",
            ),
            (
                "trait const reorder",
                "trait HasItems { const EARLIER: bool; const TARGET: bool; }",
                "trait HasItems { const TARGET: bool; const EARLIER: bool; }",
            ),
            (
                "impl function reorder",
                "struct HasItems; impl HasItems { fn earlier() {} fn target(&self) -> bool { true } }",
                "struct HasItems; impl HasItems { fn target(&self) -> bool { true } fn earlier() {} }",
            ),
            (
                "impl type reorder",
                "struct HasItems; impl HasItems { type Earlier = bool; type Target = bool; }",
                "struct HasItems; impl HasItems { type Target = bool; type Earlier = bool; }",
            ),
            (
                "impl const reorder",
                "struct HasItems; impl HasItems { const EARLIER: bool = true; const TARGET: bool = true; }",
                "struct HasItems; impl HasItems { const TARGET: bool = true; const EARLIER: bool = true; }",
            ),
        ];

        for (label, before, after) in cases {
            let mut database = Database::default();
            let source_file = database.add_source_file("lib.rs".to_owned(), before.to_owned());
            database.attach(|db| force_target_observer(db, source_file));
            database.take_query_log();

            source_file.set_text(&mut database).to(after.to_owned());
            database.attach(|db| force_target_observer(db, source_file));
            let edit_trace = database.take_query_log();
            assert!(
                !edit_trace.contains("observe_symbol_identity"),
                "moving an unchanged associated {label} reminted it:\n{edit_trace}"
            );

            source_file.set_text(&mut database).to(before.to_owned());
            database.attach(|db| force_target_observer(db, source_file));
            let reverse_trace = database.take_query_log();
            assert!(
                !reverse_trace.contains("observe_symbol_identity"),
                "reversing the associated {label} edit reminted it:\n{reverse_trace}"
            );
        }
    }

    // Evidence: SPAN-A1, SPAN-A3, ARC-A1, SYM-A2, INC-A2, INC-A3.
    #[test]
    fn moving_associated_item_reuses_method_signature_and_body() {
        use sage_ir::ty::TraitItemDef;
        use salsa::Setter as _;

        fn force_target_body(db: &dyn Db, source_file: SourceFile) {
            let (_, root) = setup_root_module(db, source_file);
            let items = root
                .expanded_module_items(db)
                .iter()
                .find_map(|symbol| match symbol.data(db) {
                    SymbolData::TraitSymbol(TraitSymbol::Local(owner)) => Some(owner.items(db)),
                    SymbolData::ImplSymbol(sage_ir::symbol::ImplSymbol::Local(owner)) => {
                        Some(owner.items(db))
                    }
                    _ => None,
                })
                .expect("expected a trait or impl owner");
            let (item_stash, binder) = items.open();
            let target = item_stash[binder.value]
                .iter()
                .find_map(|item| match item {
                    TraitItemDef::Function(FnSymbol::Local(function)) => {
                        (function.name(db).text(db) == "target").then_some(*function)
                    }
                    TraitItemDef::Function(FnSymbol::Ext(_))
                    | TraitItemDef::Type(_)
                    | TraitItemDef::Const(_) => None,
                })
                .expect("expected target method");

            let body = target.body(db);
            assert!(body.diagnostics.is_empty(), "{:?}", body.diagnostics);
        }

        fn query_names(trace: &str) -> String {
            let mut names = trace
                .lines()
                .map(|entry| {
                    let entry = entry.strip_prefix("  salsa: ").unwrap_or(entry);
                    entry.split_once("(Id(").map_or(entry, |(name, _)| name)
                })
                .collect::<Vec<_>>();
            names.sort_unstable();
            names.join("\n")
        }

        let cases = [
            (
                "trait",
                "trait HasItems {\n    fn earlier();\n    fn target(&self) -> bool { true }\n}",
                "    fn inserted();\n",
            ),
            (
                "impl",
                "struct HasItems;\nimpl HasItems {\n    fn earlier() {}\n    fn target(&self) -> bool { true }\n}",
                "    fn inserted() {}\n",
            ),
        ];

        for (owner_kind, before, inserted) in cases {
            let after = before.replace("    fn target", &format!("{inserted}    fn target"));
            let mut database = Database::default();
            let source_file = database.add_source_file("lib.rs".to_owned(), before.to_owned());
            database.attach(|db| force_target_body(db, source_file));
            database.take_query_log();

            database.attach(|db| force_target_body(db, source_file));
            assert_eq!(
                database.take_query_log(),
                "",
                "the unchanged {owner_kind} method body should be fully reusable"
            );

            source_file.set_text(&mut database).to(after);
            database.attach(|db| force_target_body(db, source_file));
            let edit_trace = database.take_query_log();

            let expected = match owner_kind {
                "trait" => expect![[r#"
                    LocalTraitSym < 'db >::items_
                    LocalTraitSym < 'db >::sig_
                    local_expanded_module_items
                    setup_root_module"#]],
                "impl" => expect![[r#"
                    LocalImplSym < 'db >::items_
                    LocalImplSym < 'db >::sig_
                    local_expanded_module_items
                    local_impl_associated_item
                    local_impl_associated_item
                    local_impl_associated_item
                    setup_root_module"#]],
                _ => unreachable!(),
            };
            expected.assert_eq(&query_names(&edit_trace));
        }
    }

    #[test]
    fn trait_items_have_stable_symbols_and_owner_identity() {
        use sage_ir::local_syms::LocalAssociatedOwner;
        use sage_ir::symbol::{ConstSymbol, FnSymbol, TypeAliasSymbol};
        use sage_ir::ty::{SolverEligibility, TraitItemDef};

        with_test_crate(
            "trait HasItems { fn method(&self); fn make(); type Output; const VALUE: u32; }",
            |db, root| {
                let [symbol] = root.expanded_module_items(db) else {
                    panic!("expected one trait")
                };
                let SymbolData::TraitSymbol(TraitSymbol::Local(local_trait)) = symbol.data(db)
                else {
                    panic!("expected a local trait")
                };
                let items = local_trait.items(db);
                let (stash, binder) = items.open();
                let [
                    TraitItemDef::Function(FnSymbol::Local(function)),
                    TraitItemDef::Function(FnSymbol::Local(associated_function)),
                    TraitItemDef::Type(TypeAliasSymbol::Local(alias)),
                    TraitItemDef::Const(ConstSymbol::Local(constant)),
                ] = &stash[binder.value]
                else {
                    panic!("expected function, type, and const items")
                };
                let owner = Some(LocalAssociatedOwner::Trait(local_trait));
                assert_eq!(function.owner(db), owner);
                assert_eq!(alias.owner(db), owner);
                assert_eq!(constant.owner(db), owner);
                assert_eq!(stash[binder.generics].len(), 1);

                let method_signature = function.sig(db);
                let (method_stash, method) = method_signature.open();
                assert!(method.value.receiver.is_some());
                assert_eq!(method_stash[method.generics], stash[binder.generics]);

                let associated_signature = associated_function.sig(db);
                let associated = associated_signature.root();
                assert!(associated.value.receiver.is_none());
                assert_eq!(
                    associated.value.method_candidate_eligibility,
                    SolverEligibility::Unsupported
                );
            },
        );
    }

    #[test]
    fn impl_associated_body_restores_self_without_a_receiver() {
        use sage_ir::symbol::{FnSymbol, ImplSymbol};
        use sage_ir::ty::{SolverEligibility, TraitItemDef};

        with_test_crate(
            "struct Wrap<T> { value: T }\nimpl<T> Wrap<T> { fn keep(value: Self) -> Self { let value: Self = value; value } }",
            |db, root| {
                let [_, impl_symbol] = root.expanded_module_items(db) else {
                    panic!("expected a struct and an impl")
                };
                let SymbolData::ImplSymbol(ImplSymbol::Local(local_impl)) = impl_symbol.data(db)
                else {
                    panic!("expected a local impl")
                };
                let items = local_impl.items(db);
                let (stash, items) = items.open();
                let [TraitItemDef::Function(FnSymbol::Local(function))] = &stash[items.value]
                else {
                    panic!("expected one associated function")
                };

                let signature = function.sig(db);
                let signature = signature.root();
                assert!(signature.value.owner_self_ty.is_some());
                assert!(signature.value.receiver.is_none());
                assert_eq!(
                    signature.value.method_candidate_eligibility,
                    SolverEligibility::Unsupported
                );

                let body = function.body(db);
                assert!(body.diagnostics.is_empty(), "{:?}", body.diagnostics);
            },
        );
    }
}

#[cfg(test)]
mod trait_solver_boundary_tests {
    use super::*;
    use sage_ir::check::infer::egraph::VersionedEGraph;
    use sage_ir::check::infer::unify::{UnifyError, try_unify};
    use sage_ir::check::infer::version::{Universe, Version};
    use sage_ir::check::solve::{
        AppliedCertainty, Assumption, Atom, CanonicalVarRole, Goal, GoalOutput, GoalResult,
        QueryResultData, SolverGoal, apply_query_response, canonicalize_goal, extract_query_result,
        extract_query_result_with_output, instantiate_query,
    };
    use sage_ir::generic_param::{AlphaEquivParam, GenericParam, GenericParamKind};
    use sage_ir::scope::ScopeSymbol;
    use sage_ir::symbol::{SymbolData, TraitSymbol};
    use sage_ir::ty::{
        AliasTy, IntTy, NamedAliasTy, OpaqueAliasTy, ProjectionTy, TraitItemDef, TraitRef, Ty,
    };
    use sage_stash::StashCopy;

    fn crate_from_root_items<'db>(
        db: &'db dyn Db,
        root: sage_ir::symbol::ModSymbol<'db>,
    ) -> LocalCrateSymbol<'db> {
        let local_trait = root
            .expanded_module_items(db)
            .iter()
            .find_map(|symbol| match symbol.data(db) {
                SymbolData::TraitSymbol(TraitSymbol::Local(local)) => Some(local),
                _ => None,
            })
            .unwrap();
        let ScopeSymbol::Crate(krate) = local_trait.scope(db) else {
            panic!("root item should be crate-scoped")
        };
        krate
    }

    #[test]
    fn canonical_equality_round_trips_transactionally() {
        with_test_crate("trait Marker {}", |db, root| {
            let krate = crate_from_root_items(db, root);
            let mut caller_stash = Stash::new();
            let mut caller_egraph = VersionedEGraph::new();
            let input_index = caller_egraph.alloc_var(Version::ROOT, Universe(1));
            let input = caller_stash.alloc(Ty::InferVar(input_index));
            let integer = caller_stash.alloc(Ty::Int(IntTy::I32));
            let assumptions = caller_stash.alloc_slice::<Assumption>(&[]);
            let canonical = canonicalize_goal(
                db,
                &caller_stash,
                &caller_egraph,
                Version::ROOT,
                krate,
                Universe(1),
                true,
                assumptions,
                Goal::Atom(Atom::Equals(input, integer)),
            );

            let mut proof = instantiate_query(&canonical.data);
            let SolverGoal::Prove(Goal::Atom(Atom::Equals(left, right))) = proof.goal else {
                panic!("expected equality goal")
            };
            try_unify(
                &mut proof.egraph,
                &mut proof.stash,
                proof.version,
                left,
                right,
            )
            .unwrap();
            let modulo = Goal::true_(&mut proof.stash);
            let response = extract_query_result(db, &proof, GoalResult::Yes { modulo });
            let applied = apply_query_response(
                db,
                &mut caller_stash,
                &mut caller_egraph,
                Version::ROOT,
                &canonical.data,
                &canonical.mapping,
                &response,
            )
            .unwrap();

            assert!(matches!(applied.certainty, AppliedCertainty::Yes { .. }));
            assert_eq!(caller_egraph.find(Version::ROOT, input), integer);
        });
    }

    #[test]
    #[should_panic(expected = "solver operation returned an incompatible output kind")]
    fn canonical_response_rejects_an_output_for_the_wrong_operation() {
        with_test_crate("trait Marker {}", |db, root| {
            let krate = crate_from_root_items(db, root);
            let mut stash = Stash::new();
            let assumptions = stash.alloc_slice::<Assumption>(&[]);
            let goal = Goal::true_(&mut stash);
            let canonical = canonicalize_goal(
                db,
                &stash,
                &VersionedEGraph::new(),
                Version::ROOT,
                krate,
                Universe::ROOT,
                true,
                assumptions,
                goal,
            );
            let mut proof = instantiate_query(&canonical.data);
            let output = proof.stash.alloc(Ty::Bool);
            let modulo = Goal::true_(&mut proof.stash);
            let _ = extract_query_result_with_output(
                db,
                &proof,
                GoalOutput::Type(output),
                GoalResult::Yes { modulo },
            );
        });
    }

    #[test]
    fn alias_variants_copy_fold_and_display_without_erasing_identity() {
        use rustc_hash::FxHashMap;
        use sage_ir::display::TyDisplay;
        use sage_ir::ty_fold::{SubstTarget, Substitute, TyFolder};

        with_test_crate(
            "trait Marker<T> { type Item; }\ntype Named<T> = T;\ntype Hidden<T> = T;",
            |db, root| {
                let mut trait_symbol = None;
                let mut named = None;
                let mut hidden = None;
                for symbol in root.expanded_module_items(db) {
                    match symbol.data(db) {
                        SymbolData::TraitSymbol(symbol) => trait_symbol = Some(symbol),
                        SymbolData::TypeAliasSymbol(symbol) => {
                            let local = match symbol {
                                sage_ir::symbol::TypeAliasSymbol::Local(local) => local,
                                sage_ir::symbol::TypeAliasSymbol::Ext(_) => continue,
                            };
                            let name = local.name(db).text(db);
                            if name == "Named" {
                                named = Some(symbol);
                            } else if name == "Hidden" {
                                hidden = Some(symbol);
                            }
                        }
                        SymbolData::FnSymbol(_)
                        | SymbolData::StructSymbol(_)
                        | SymbolData::EnumSymbol(_)
                        | SymbolData::VariantSymbol(_)
                        | SymbolData::VariantCtorSymbol(_)
                        | SymbolData::ConstSymbol(_)
                        | SymbolData::StaticSymbol(_)
                        | SymbolData::ImplSymbol(_)
                        | SymbolData::ModSymbol(_)
                        | SymbolData::MacroDefSymbol(_)
                        | SymbolData::UseSymbol(_)
                        | SymbolData::IntrinsicTypeSymbol(_)
                        | SymbolData::MacroInvocationSymbol(_) => {}
                    }
                }
                let trait_symbol = trait_symbol.unwrap();
                let items = trait_symbol.items(db).unwrap();
                let (item_stash, binder) = items.open();
                let associated = item_stash[binder.value]
                    .iter()
                    .find_map(|item| match item {
                        TraitItemDef::Type(symbol) => Some(*symbol),
                        TraitItemDef::Function(_) | TraitItemDef::Const(_) => None,
                    })
                    .unwrap();
                let named = named.unwrap();
                let hidden = hidden.unwrap();

                let parameter =
                    GenericParam::AlphaEquiv(AlphaEquivParam::new(db, GenericParamKind::Type, 0));
                let mut source = Stash::new();
                let parameter_ty = source.alloc(Ty::Param(parameter));
                let arguments = source.alloc_slice(&[parameter_ty]);
                let named_ty = source.alloc(Ty::Alias(AliasTy::Named(NamedAliasTy {
                    def: named,
                    args: arguments,
                })));
                let associated_ty = source.alloc(Ty::Alias(AliasTy::Associated(ProjectionTy {
                    associated_ty: associated,
                    self_ty: parameter_ty,
                    trait_ref: TraitRef {
                        trait_sym: trait_symbol,
                        args: arguments,
                    },
                    args: arguments,
                })));
                let opaque_ty = source.alloc(Ty::Alias(AliasTy::Opaque(OpaqueAliasTy {
                    def: hidden,
                    args: arguments,
                })));

                assert_ne!(source[named_ty], source[associated_ty]);
                assert_ne!(source[named_ty], source[opaque_ty]);
                assert_eq!(
                    TyDisplay::new(db, &source, named_ty).to_string(),
                    "Named<?>"
                );
                assert_eq!(
                    TyDisplay::new(db, &source, associated_ty).to_string(),
                    "<? as Marker<?>>::Item<?>"
                );
                assert_eq!(
                    TyDisplay::new(db, &source, opaque_ty).to_string(),
                    "opaque Hidden<?>"
                );

                let mut copied = Stash::new();
                let copied_named = named_ty.stash_copy(&source, &mut copied);
                let copied_associated = associated_ty.stash_copy(&source, &mut copied);
                let copied_opaque = opaque_ty.stash_copy(&source, &mut copied);
                assert!(matches!(copied[copied_named], Ty::Alias(AliasTy::Named(_))));
                assert!(matches!(
                    copied[copied_associated],
                    Ty::Alias(AliasTy::Associated(_))
                ));
                assert!(matches!(
                    copied[copied_opaque],
                    Ty::Alias(AliasTy::Opaque(_))
                ));

                let mut substitution = FxHashMap::default();
                substitution.insert(parameter, SubstTarget::Ty(Ty::Bool));
                let mut folded = Stash::new();
                let mut folder = Substitute::new(&source, &mut folded, substitution);
                let folded_alias = folder.fold_ty(source[associated_ty]);
                let Ty::Alias(AliasTy::Associated(folded_projection)) = folded_alias else {
                    panic!("expected associated alias")
                };
                assert_eq!(folded[folded_projection.self_ty], Ty::Bool);
                assert_eq!(
                    folded[folded_projection.trait_ref.args][0],
                    folded_projection.self_ty
                );
                assert_eq!(folded[folded_projection.args][0], folded_projection.self_ty);

                let mut inference = Stash::new();
                let mut egraph = VersionedEGraph::new();
                let recursive_index = egraph.alloc_var(Version::ROOT, Universe::ROOT);
                let recursive = inference.alloc(Ty::InferVar(recursive_index));
                let recursive_args = inference.alloc_slice(&[recursive]);
                let recursive_alias = inference.alloc(Ty::Alias(AliasTy::Named(NamedAliasTy {
                    def: named,
                    args: recursive_args,
                })));
                assert!(matches!(
                    try_unify(
                        &mut egraph,
                        &mut inference,
                        Version::ROOT,
                        recursive,
                        recursive_alias,
                    ),
                    Err(UnifyError::OccursCheck { variable, .. })
                        if variable == recursive_index
                ));

                let outer_index = egraph.alloc_var(Version::ROOT, Universe::ROOT);
                let outer = inference.alloc(Ty::InferVar(outer_index));
                let inaccessible =
                    GenericParam::AlphaEquiv(AlphaEquivParam::new(db, GenericParamKind::Type, 1));
                egraph.register_placeholder(inaccessible, Universe(1));
                let inaccessible = inference.alloc(Ty::Param(inaccessible));
                let inaccessible_args = inference.alloc_slice(&[inaccessible]);
                let inaccessible_alias = inference.alloc(Ty::Alias(AliasTy::Named(NamedAliasTy {
                    def: named,
                    args: inaccessible_args,
                })));
                assert!(matches!(
                    try_unify(
                        &mut egraph,
                        &mut inference,
                        Version::ROOT,
                        outer,
                        inaccessible_alias,
                    ),
                    Err(UnifyError::UniverseLeak { variable, ceiling })
                        if variable == outer_index && ceiling == Universe::ROOT
                ));
            },
        );
    }

    #[test]
    fn response_extraction_preserves_repeated_local_variable_sharing() {
        with_test_crate("trait Marker {}", |db, root| {
            let krate = crate_from_root_items(db, root);
            let mut caller_stash = Stash::new();
            let mut caller_egraph = VersionedEGraph::new();
            let input_index = caller_egraph.alloc_var(Version::ROOT, Universe::ROOT);
            let input = caller_stash.alloc(Ty::InferVar(input_index));
            let assumptions = caller_stash.alloc_slice::<Assumption>(&[]);
            let canonical = canonicalize_goal(
                db,
                &caller_stash,
                &caller_egraph,
                Version::ROOT,
                krate,
                Universe::ROOT,
                true,
                assumptions,
                Goal::Atom(Atom::Equals(input, input)),
            );
            let mut proof = instantiate_query(&canonical.data);
            let local_index = proof.egraph.alloc_var(proof.version, Universe::ROOT);
            let local = proof.stash.alloc(Ty::InferVar(local_index));
            let elements = proof.stash.alloc_slice(&[local, local]);
            let tuple = proof.stash.alloc(Ty::Tuple(elements));
            try_unify(
                &mut proof.egraph,
                &mut proof.stash,
                proof.version,
                proof.inputs[0].ty,
                tuple,
            )
            .unwrap();
            let modulo = Goal::true_(&mut proof.stash);
            let response = extract_query_result(db, &proof, GoalResult::Yes { modulo });
            let (stash, result) = response.open();
            assert_eq!(stash[result.bound_vars].len(), 1);
            let QueryResultData::Yes { subst, .. } = result.value else {
                panic!("expected yes response")
            };
            let [entry] = &stash[subst] else {
                panic!("expected one substitution")
            };
            let Ty::Tuple(elements) = stash[entry.value] else {
                panic!("expected tuple substitution")
            };
            let elements = &stash[elements];
            assert_eq!(stash[elements[0]], stash[elements[1]]);
        });
    }

    #[test]
    fn aliases_round_trip_through_canonical_query_and_response_stashes() {
        with_test_crate("trait Marker {}\ntype Named<T> = T;", |db, root| {
            let krate = crate_from_root_items(db, root);
            let named = root
                .expanded_module_items(db)
                .iter()
                .find_map(|symbol| match symbol.data(db) {
                    SymbolData::TypeAliasSymbol(symbol) => Some(symbol),
                    SymbolData::FnSymbol(_)
                    | SymbolData::StructSymbol(_)
                    | SymbolData::EnumSymbol(_)
                    | SymbolData::VariantSymbol(_)
                    | SymbolData::VariantCtorSymbol(_)
                    | SymbolData::TraitSymbol(_)
                    | SymbolData::ConstSymbol(_)
                    | SymbolData::StaticSymbol(_)
                    | SymbolData::ImplSymbol(_)
                    | SymbolData::ModSymbol(_)
                    | SymbolData::MacroDefSymbol(_)
                    | SymbolData::UseSymbol(_)
                    | SymbolData::IntrinsicTypeSymbol(_)
                    | SymbolData::MacroInvocationSymbol(_) => None,
                })
                .unwrap();

            let mut caller_stash = Stash::new();
            let mut caller_egraph = VersionedEGraph::new();
            let input_index = caller_egraph.alloc_var(Version::ROOT, Universe::ROOT);
            let input = caller_stash.alloc(Ty::InferVar(input_index));
            let alias_args = caller_stash.alloc_slice(&[input]);
            let alias = caller_stash.alloc(Ty::Alias(AliasTy::Named(NamedAliasTy {
                def: named,
                args: alias_args,
            })));
            let assumptions = caller_stash.alloc_slice::<Assumption>(&[]);
            let canonical_alias = canonicalize_goal(
                db,
                &caller_stash,
                &caller_egraph,
                Version::ROOT,
                krate,
                Universe::ROOT,
                true,
                assumptions,
                Goal::Atom(Atom::Equals(alias, alias)),
            );
            let (canonical_stash, canonical_data) = canonical_alias.data.open();
            let SolverGoal::Prove(Goal::Atom(Atom::Equals(canonical_left, canonical_right))) =
                canonical_data.goal
            else {
                panic!("expected equality")
            };
            assert_eq!(canonical_left, canonical_right);
            let Ty::Alias(AliasTy::Named(canonical_named)) = canonical_stash[canonical_left] else {
                panic!("expected named alias")
            };
            assert!(matches!(
                canonical_stash[canonical_stash[canonical_named.args][0]],
                Ty::Param(GenericParam::AlphaEquiv(_))
            ));

            let canonical_input = canonicalize_goal(
                db,
                &caller_stash,
                &caller_egraph,
                Version::ROOT,
                krate,
                Universe::ROOT,
                true,
                assumptions,
                Goal::Atom(Atom::Equals(input, input)),
            );
            let mut proof = instantiate_query(&canonical_input.data);
            let local_index = proof.egraph.alloc_var(proof.version, Universe(1));
            let local = proof.stash.alloc(Ty::InferVar(local_index));
            let local_args = proof.stash.alloc_slice(&[local]);
            let local_alias = proof.stash.alloc(Ty::Alias(AliasTy::Named(NamedAliasTy {
                def: named,
                args: local_args,
            })));
            try_unify(
                &mut proof.egraph,
                &mut proof.stash,
                proof.version,
                proof.inputs[0].ty,
                local_alias,
            )
            .unwrap();
            let modulo = Goal::true_(&mut proof.stash);
            let response = extract_query_result(db, &proof, GoalResult::Yes { modulo });
            let (response_stash, result) = response.open();
            assert_eq!(response_stash[result.bound_vars].len(), 1);
            let QueryResultData::Yes { subst, .. } = result.value else {
                panic!("expected yes response")
            };
            let [entry] = &response_stash[subst] else {
                panic!("expected alias substitution")
            };
            assert!(matches!(
                response_stash[entry.value],
                Ty::Alias(AliasTy::Named(_))
            ));

            apply_query_response(
                db,
                &mut caller_stash,
                &mut caller_egraph,
                Version::ROOT,
                &canonical_input.data,
                &canonical_input.mapping,
                &response,
            )
            .unwrap();
            let applied = caller_egraph.find(Version::ROOT, input);
            let Ty::Alias(AliasTy::Named(applied_alias)) = caller_stash[applied] else {
                panic!("expected imported named alias")
            };
            let imported_variable = caller_stash[applied_alias.args][0];
            let Ty::InferVar(imported_index) = caller_stash[imported_variable] else {
                panic!("expected imported response variable")
            };
            // Unifying this variable beneath the root-universe input lowers it
            // before response extraction; importing the alias preserves that
            // universe constraint.
            assert_eq!(
                caller_egraph.current_universe(Version::ROOT, imported_index),
                Universe::ROOT
            );
        });
    }

    #[test]
    fn canonical_roles_and_relative_universes_remain_distinct() {
        with_test_crate("trait Marker {}", |db, root| {
            let krate = crate_from_root_items(db, root);
            let mut stash = Stash::new();
            let mut egraph = VersionedEGraph::new();
            let input_index = egraph.alloc_var(Version::ROOT, Universe(1));
            let input = stash.alloc(Ty::InferVar(input_index));
            let rigid =
                GenericParam::AlphaEquiv(AlphaEquivParam::new(db, GenericParamKind::Type, 99));
            egraph.register_placeholder(rigid, Universe(2));
            let rigid_ty = stash.alloc(Ty::Param(rigid));
            let assumptions = stash.alloc_slice::<Assumption>(&[]);
            let canonical = canonicalize_goal(
                db,
                &stash,
                &egraph,
                Version::ROOT,
                krate,
                Universe(2),
                true,
                assumptions,
                Goal::Atom(Atom::Equals(input, rigid_ty)),
            );
            let (stash, data) = canonical.data.open();
            let variables = &stash[data.canonical_vars];
            assert_eq!(data.canonical_universe, 1);
            assert_eq!(variables[0].role, CanonicalVarRole::ExistentialInput);
            assert_eq!(variables[0].relative_universe, 0);
            assert_eq!(variables[1].role, CanonicalVarRole::RigidPlaceholder);
            assert_eq!(variables[1].relative_universe, 1);
        });
    }
}

#[cfg(test)]
mod trait_solver_proof_tests {
    use super::*;
    use sage_ir::check::infer::egraph::VersionedEGraph;
    use sage_ir::check::infer::version::{Universe, Version};
    use sage_ir::check::solve::{
        AppliedCertainty, Assumption, Atom, Goal, GoalOutput, GoalQuery, QueryResultData,
        SolverGoal, apply_query_response, canonicalize_goal, canonicalize_solver_goal,
    };
    use sage_ir::scope::ScopeSymbol;
    use sage_ir::symbol::{StructSymbol, SymbolData, TraitSymbol};
    use sage_ir::ty::{AliasTy, IntTy, ProjectionTy, TraitItemDef, TraitRef, Ty};

    fn trait_and_crate<'db>(
        db: &'db dyn Db,
        root: sage_ir::symbol::ModSymbol<'db>,
        name: &str,
    ) -> (TraitSymbol<'db>, LocalCrateSymbol<'db>) {
        let local = root
            .expanded_module_items(db)
            .iter()
            .find_map(|symbol| match symbol.data(db) {
                SymbolData::TraitSymbol(TraitSymbol::Local(local))
                    if local.name(db).text(db) == name =>
                {
                    Some(local)
                }
                _ => None,
            })
            .unwrap();
        let ScopeSymbol::Crate(krate) = local.scope(db) else {
            panic!("root trait should be crate-scoped")
        };
        (TraitSymbol::Local(local), krate)
    }

    fn first_associated_type<'db>(
        db: &'db dyn Db,
        trait_symbol: TraitSymbol<'db>,
    ) -> sage_ir::symbol::TypeAliasSymbol<'db> {
        let items = trait_symbol.items(db).unwrap();
        let (stash, items) = items.open();
        stash[items.value]
            .iter()
            .find_map(|item| match item {
                TraitItemDef::Type(item) => Some(*item),
                TraitItemDef::Function(_) | TraitItemDef::Const(_) => None,
            })
            .unwrap()
    }

    #[test]
    fn local_associated_type_normalization_produces_a_type_output() {
        with_test_crate(
            "trait Iterable { type Item; }\n\
             struct Wrap<T> { value: T }\n\
             impl<T> Iterable for Wrap<T> { type Item = T; }",
            |db, root| {
                let (iterable, krate) = trait_and_crate(db, root, "Iterable");
                let wrap = root
                    .expanded_module_items(db)
                    .iter()
                    .find_map(|symbol| match symbol.data(db) {
                        SymbolData::StructSymbol(StructSymbol::Local(item))
                            if item.name(db).text(db) == "Wrap" =>
                        {
                            Some(item)
                        }
                        SymbolData::StructSymbol(_)
                        | SymbolData::FnSymbol(_)
                        | SymbolData::EnumSymbol(_)
                        | SymbolData::VariantSymbol(_)
                        | SymbolData::VariantCtorSymbol(_)
                        | SymbolData::TraitSymbol(_)
                        | SymbolData::TypeAliasSymbol(_)
                        | SymbolData::ConstSymbol(_)
                        | SymbolData::StaticSymbol(_)
                        | SymbolData::ImplSymbol(_)
                        | SymbolData::ModSymbol(_)
                        | SymbolData::MacroDefSymbol(_)
                        | SymbolData::UseSymbol(_)
                        | SymbolData::IntrinsicTypeSymbol(_)
                        | SymbolData::MacroInvocationSymbol(_) => None,
                    })
                    .unwrap();
                let items = iterable.items(db).unwrap();
                let (item_stash, items) = items.open();
                let associated_ty = item_stash[items.value]
                    .iter()
                    .find_map(|item| match item {
                        TraitItemDef::Type(item) => Some(*item),
                        TraitItemDef::Function(_) | TraitItemDef::Const(_) => None,
                    })
                    .unwrap();

                let mut stash = Stash::new();
                let bool_ty = stash.alloc(Ty::Bool);
                let self_args = stash.alloc_slice(&[bool_ty]);
                let self_ty = stash.alloc(Ty::Adt(wrap.into(), self_args));
                let trait_args = stash.alloc_slice(&[]);
                let projection = AliasTy::Associated(ProjectionTy {
                    associated_ty,
                    self_ty,
                    trait_ref: TraitRef {
                        trait_sym: iterable,
                        args: trait_args,
                    },
                    args: stash.alloc_slice(&[]),
                });
                let assumptions = stash.alloc_slice::<Assumption>(&[]);
                let egraph = VersionedEGraph::new();
                let canonical = canonicalize_solver_goal(
                    db,
                    &stash,
                    &egraph,
                    Version::ROOT,
                    krate,
                    Universe::ROOT,
                    true,
                    assumptions,
                    SolverGoal::Normalize(projection),
                );
                let result = GoalQuery::new(db, canonical.data).solve(db);
                let (result_stash, result) = result.open();
                let QueryResultData::Yes {
                    output: GoalOutput::Type(output),
                    subst,
                    modulo,
                } = result.value
                else {
                    panic!("normalization should produce a type")
                };
                assert_eq!(result_stash[output], Ty::Bool);
                assert!(result_stash[subst].is_empty());
                assert!(modulo.is_trivially_true(result_stash));
            },
        );
    }

    #[test]
    fn local_normalization_reads_one_keyed_value_without_impl_item_enumeration() {
        use salsa::Setter as _;

        const SOURCE: &str = "trait Iterable<T> { type Item; type Unrelated; }\n\
             impl Iterable<bool> for bool { type Item = i32; type Unrelated = char; }\n\
             impl Iterable<char> for bool { type Item = u32; type Unrelated = bool; }";

        fn force(db: &dyn Db, source_file: SourceFile) {
            let (_, root) = setup_root_module(db, source_file);
            let (iterable, krate) = trait_and_crate(db, root, "Iterable");
            let associated_ty = first_associated_type(db, iterable);
            let mut stash = Stash::new();
            let self_ty = stash.alloc(Ty::Bool);
            let trait_argument = stash.alloc(Ty::Bool);
            let projection = AliasTy::Associated(ProjectionTy {
                associated_ty,
                self_ty,
                trait_ref: TraitRef {
                    trait_sym: iterable,
                    args: stash.alloc_slice(&[trait_argument]),
                },
                args: stash.alloc_slice(&[]),
            });
            let assumptions = stash.alloc_slice::<Assumption>(&[]);
            let canonical = canonicalize_solver_goal(
                db,
                &stash,
                &VersionedEGraph::new(),
                Version::ROOT,
                krate,
                Universe::ROOT,
                true,
                assumptions,
                SolverGoal::Normalize(projection),
            );
            assert!(matches!(
                GoalQuery::new(db, canonical.data).solve(db).root().value,
                QueryResultData::Yes { .. }
            ));
        }

        let mut database = Database::default();
        let source_file = database.add_source_file("lib.rs".to_owned(), SOURCE.to_owned());
        database.attach(|db| force(db, source_file));
        let cold = database.take_query_log();
        assert!(
            cold.contains("local_impl_associated_type_value"),
            "the requested value needs one keyed query:\n{cold}"
        );
        assert_eq!(
            cold.matches("local_impl_associated_type_value").count(),
            1,
            "a header-mismatched candidate must not read its value:\n{cold}"
        );
        assert_eq!(
            cold.matches("local_impl_associated_item").count(),
            1,
            "the requested value must use one canonical keyed item producer:\n{cold}"
        );
        assert!(
            !cold.contains("LocalImplSym < 'db >::items_"),
            "normalization must not enumerate or lower sibling impl items:\n{cold}"
        );

        database.attach(|db| force(db, source_file));
        let warm = database.take_query_log();
        assert!(
            !warm.contains("local_impl_associated_type_value"),
            "the unchanged value query should be reused:\n{warm}"
        );

        source_file
            .set_text(&mut database)
            .to(SOURCE.replace("type Unrelated = char", "type Unrelated = i16"));
        database.attach(|db| force(db, source_file));
        let unrelated_edit = database.take_query_log();
        assert!(
            unrelated_edit.contains("local_impl_associated_item"),
            "the keyed item producer should revalidate the edited impl CST:\n{unrelated_edit}"
        );
        assert!(
            !unrelated_edit.contains("local_impl_associated_type_value"),
            "editing an unrelated associated value must not rerun the requested value query:\n{unrelated_edit}"
        );

        let mut database = Database::default();
        let source_file = database.add_source_file("lib.rs".to_owned(), SOURCE.to_owned());
        database.attach(|db| {
            let (_, root) = setup_root_module(db, source_file);
            let first_impl = root
                .expanded_module_items(db)
                .iter()
                .find_map(|symbol| match symbol.data(db) {
                    SymbolData::ImplSymbol(sage_ir::symbol::ImplSymbol::Local(item)) => Some(item),
                    _ => None,
                })
                .expect("expected a local impl");
            first_impl.items(db);
        });
        database.take_query_log();

        database.attach(|db| force(db, source_file));
        let after_aggregate = database.take_query_log();
        assert!(
            after_aggregate.contains("local_impl_associated_type_value"),
            "normalization itself must be cold in this scenario:\n{after_aggregate}"
        );
        assert!(
            !after_aggregate.contains("local_impl_associated_item"),
            "normalization must reuse the canonical item producer first invoked by impl.items():\n{after_aggregate}"
        );
    }

    #[test]
    fn normalization_assumptions_are_distinct_from_bare_trait_facts() {
        with_test_crate("trait Iterable { type Item; }", |db, root| {
            let (iterable, krate) = trait_and_crate(db, root, "Iterable");
            let associated_ty = first_associated_type(db, iterable);
            let normalize = |value_fact: bool, query_bool: bool, fact_bool: bool| {
                let mut stash = Stash::new();
                let self_ty = stash.alloc(if query_bool {
                    Ty::Bool
                } else {
                    Ty::Int(IntTy::I32)
                });
                let trait_ref = TraitRef {
                    trait_sym: iterable,
                    args: stash.alloc_slice(&[]),
                };
                let projection = AliasTy::Associated(ProjectionTy {
                    associated_ty,
                    self_ty,
                    trait_ref,
                    args: stash.alloc_slice(&[]),
                });
                let assumptions = if value_fact {
                    let ty = stash.alloc(Ty::Int(IntTy::I32));
                    stash.alloc_slice(&[Assumption::NormalizesTo {
                        alias: projection,
                        ty,
                    }])
                } else {
                    let assumption_self = stash.alloc(if fact_bool {
                        Ty::Bool
                    } else {
                        Ty::Int(IntTy::I32)
                    });
                    stash.alloc_slice(&[Assumption::TraitImpl {
                        self_ty: assumption_self,
                        trait_ref,
                    }])
                };
                let egraph = VersionedEGraph::new();
                let canonical = canonicalize_solver_goal(
                    db,
                    &stash,
                    &egraph,
                    Version::ROOT,
                    krate,
                    Universe::ROOT,
                    true,
                    assumptions,
                    SolverGoal::Normalize(projection),
                );
                GoalQuery::new(db, canonical.data).solve(db)
            };

            assert!(matches!(
                normalize(false, true, true).root().value,
                QueryResultData::Maybe { .. }
            ));
            assert!(matches!(
                normalize(false, false, true).root().value,
                QueryResultData::No
            ));
            let result = normalize(true, true, true);
            let (stash, result) = result.open();
            let QueryResultData::Yes {
                output: GoalOutput::Type(output),
                ..
            } = result.value
            else {
                panic!("an explicit normalization fact should supply its value")
            };
            assert_eq!(stash[output], Ty::Int(IntTy::I32));
        });
    }

    #[test]
    fn incompatible_normalization_outputs_are_ambiguous_but_identical_outputs_merge() {
        let check = |second_value: &str, expect_yes: bool| {
            with_test_crate(
                &format!(
                    "trait Iterable {{ type Item; }}\n\
                     impl Iterable for bool {{ type Item = bool; }}\n\
                     impl Iterable for bool {{ type Item = {second_value}; }}"
                ),
                |db, root| {
                    let (iterable, krate) = trait_and_crate(db, root, "Iterable");
                    let associated_ty = first_associated_type(db, iterable);
                    let mut stash = Stash::new();
                    let self_ty = stash.alloc(Ty::Bool);
                    let projection = AliasTy::Associated(ProjectionTy {
                        associated_ty,
                        self_ty,
                        trait_ref: TraitRef {
                            trait_sym: iterable,
                            args: stash.alloc_slice(&[]),
                        },
                        args: stash.alloc_slice(&[]),
                    });
                    let assumptions = stash.alloc_slice::<Assumption>(&[]);
                    let egraph = VersionedEGraph::new();
                    let canonical = canonicalize_solver_goal(
                        db,
                        &stash,
                        &egraph,
                        Version::ROOT,
                        krate,
                        Universe::ROOT,
                        true,
                        assumptions,
                        SolverGoal::Normalize(projection),
                    );
                    let result = GoalQuery::new(db, canonical.data).solve(db);
                    assert_eq!(
                        matches!(result.root().value, QueryResultData::Yes { .. }),
                        expect_yes,
                        "the query has no expected output with which to filter candidates"
                    );
                    assert_eq!(
                        matches!(result.root().value, QueryResultData::Maybe { .. }),
                        !expect_yes
                    );
                },
            );
        };
        check("bool", true);
        check("i32", false);
    }

    #[test]
    fn response_local_type_output_round_trips_with_sharing() {
        with_test_crate(
            "trait Iterable { type Item; }\n\
             impl<T> Iterable for bool { type Item = (T, T); }",
            |db, root| {
                let (iterable, krate) = trait_and_crate(db, root, "Iterable");
                let associated_ty = first_associated_type(db, iterable);
                let mut stash = Stash::new();
                let self_ty = stash.alloc(Ty::Bool);
                let projection = AliasTy::Associated(ProjectionTy {
                    associated_ty,
                    self_ty,
                    trait_ref: TraitRef {
                        trait_sym: iterable,
                        args: stash.alloc_slice(&[]),
                    },
                    args: stash.alloc_slice(&[]),
                });
                let assumptions = stash.alloc_slice::<Assumption>(&[]);
                let mut egraph = VersionedEGraph::new();
                let canonical = canonicalize_solver_goal(
                    db,
                    &stash,
                    &egraph,
                    Version::ROOT,
                    krate,
                    Universe::ROOT,
                    true,
                    assumptions,
                    SolverGoal::Normalize(projection),
                );
                let response = GoalQuery::new(db, canonical.data.clone()).solve(db);
                assert_eq!(response.stash()[response.root().bound_vars].len(), 1);
                let applied = apply_query_response(
                    db,
                    &mut stash,
                    &mut egraph,
                    Version::ROOT,
                    &canonical.data,
                    &canonical.mapping,
                    &response,
                )
                .unwrap();
                let AppliedCertainty::Yes {
                    output: GoalOutput::Type(output),
                    modulo,
                } = applied.certainty
                else {
                    panic!("expected a caller-local type output")
                };
                assert!(modulo.is_trivially_true(&stash));
                let Ty::Tuple(elements) = stash[output] else {
                    panic!("expected tuple output")
                };
                let [left, right] = stash[elements] else {
                    panic!("expected pair")
                };
                let Ty::InferVar(left) = stash[left] else {
                    panic!("response witness should be imported as an inference variable")
                };
                let Ty::InferVar(right) = stash[right] else {
                    panic!("response witness should be imported as an inference variable")
                };
                assert_eq!(
                    left, right,
                    "the repeated response witness must retain sharing"
                );
                assert_eq!(egraph.current_universe(Version::ROOT, left), Universe::ROOT);
            },
        );
    }

    #[test]
    fn nested_associated_value_remains_a_normalizable_alias() {
        with_test_crate(
            "trait Inner { type Value; }\ntrait Outer { type Item; }",
            |db, root| {
                let (inner, _) = trait_and_crate(db, root, "Inner");
                let (outer, krate) = trait_and_crate(db, root, "Outer");
                let inner_associated_ty = first_associated_type(db, inner);
                let outer_associated_ty = first_associated_type(db, outer);
                let mut stash = Stash::new();
                let self_ty = stash.alloc(Ty::Bool);
                let inner_projection = AliasTy::Associated(ProjectionTy {
                    associated_ty: inner_associated_ty,
                    self_ty,
                    trait_ref: TraitRef {
                        trait_sym: inner,
                        args: stash.alloc_slice(&[]),
                    },
                    args: stash.alloc_slice(&[]),
                });
                let outer_projection = AliasTy::Associated(ProjectionTy {
                    associated_ty: outer_associated_ty,
                    self_ty,
                    trait_ref: TraitRef {
                        trait_sym: outer,
                        args: stash.alloc_slice(&[]),
                    },
                    args: stash.alloc_slice(&[]),
                });
                let nested_value = stash.alloc(Ty::Alias(inner_projection));
                let concrete_value = stash.alloc(Ty::Int(IntTy::I32));
                let assumptions = stash.alloc_slice(&[
                    Assumption::NormalizesTo {
                        alias: outer_projection,
                        ty: nested_value,
                    },
                    Assumption::NormalizesTo {
                        alias: inner_projection,
                        ty: concrete_value,
                    },
                ]);
                let egraph = VersionedEGraph::new();
                let canonical = canonicalize_solver_goal(
                    db,
                    &stash,
                    &egraph,
                    Version::ROOT,
                    krate,
                    Universe::ROOT,
                    true,
                    assumptions,
                    SolverGoal::Normalize(outer_projection),
                );
                let outer_result = GoalQuery::new(db, canonical.data).solve(db);
                let (outer_stash, outer_result) = outer_result.open();
                let QueryResultData::Yes {
                    output: GoalOutput::Type(output),
                    ..
                } = outer_result.value
                else {
                    panic!("Outer::Item should normalize")
                };
                let Ty::Alias(nested) = outer_stash[output] else {
                    panic!("the associated value should retain its nested projection")
                };
                let AliasTy::Associated(nested) = nested else {
                    panic!("expected an associated projection")
                };
                assert_eq!(nested.associated_ty, inner_associated_ty);

                let canonical = canonicalize_solver_goal(
                    db,
                    &stash,
                    &VersionedEGraph::new(),
                    Version::ROOT,
                    krate,
                    Universe::ROOT,
                    true,
                    assumptions,
                    SolverGoal::Normalize(inner_projection),
                );
                let nested_result = GoalQuery::new(db, canonical.data).solve(db);
                let (nested_stash, nested_result) = nested_result.open();
                let QueryResultData::Yes {
                    output: GoalOutput::Type(output),
                    ..
                } = nested_result.value
                else {
                    panic!("the nested projection should normalize independently")
                };
                assert_eq!(nested_stash[output], Ty::Int(IntTy::I32));
            },
        );
    }

    fn assert_ground_bool_marker_is_maybe<'db>(
        db: &'db dyn Db,
        root: sage_ir::symbol::ModSymbol<'db>,
    ) {
        assert_ground_bool_marker_result(db, root, true);
    }

    fn assert_ground_bool_marker_is_no<'db>(
        db: &'db dyn Db,
        root: sage_ir::symbol::ModSymbol<'db>,
    ) {
        assert_ground_bool_marker_result(db, root, false);
    }

    fn assert_ground_bool_marker_result<'db>(
        db: &'db dyn Db,
        root: sage_ir::symbol::ModSymbol<'db>,
        expect_maybe: bool,
    ) {
        let (marker, krate) = trait_and_crate(db, root, "Marker");
        let mut stash = Stash::new();
        let egraph = VersionedEGraph::new();
        let self_ty = stash.alloc(Ty::Bool);
        let args = stash.alloc_slice(&[]);
        let assumptions = stash.alloc_slice::<Assumption>(&[]);
        let canonical = canonicalize_goal(
            db,
            &stash,
            &egraph,
            Version::ROOT,
            krate,
            Universe::ROOT,
            true,
            assumptions,
            Goal::Atom(Atom::TraitImpl {
                self_ty,
                trait_ref: TraitRef {
                    trait_sym: marker,
                    args,
                },
            }),
        );
        let result = GoalQuery::new(db, canonical.data).prove(db);
        if expect_maybe {
            assert!(matches!(result.root().value, QueryResultData::Maybe { .. }));
        } else {
            assert!(matches!(result.root().value, QueryResultData::No));
        }
    }

    #[test]
    fn environment_fact_proves_bare_flexible_self() {
        with_test_crate("trait Marker {}", |db, root| {
            let (marker, krate) = trait_and_crate(db, root, "Marker");
            let mut stash = Stash::new();
            let mut egraph = VersionedEGraph::new();
            let index = egraph.alloc_var(Version::ROOT, Universe::ROOT);
            let self_ty = stash.alloc(Ty::InferVar(index));
            let args = stash.alloc_slice(&[]);
            let trait_ref = TraitRef {
                trait_sym: marker,
                args,
            };
            let assumptions = stash.alloc_slice(&[Assumption::TraitImpl { self_ty, trait_ref }]);
            let canonical = canonicalize_goal(
                db,
                &stash,
                &egraph,
                Version::ROOT,
                krate,
                Universe::ROOT,
                true,
                assumptions,
                Goal::Atom(Atom::TraitImpl { self_ty, trait_ref }),
            );
            let query = GoalQuery::new(db, canonical.data);
            let result = query.prove(db);
            let (stash, result) = result.open();
            let QueryResultData::Yes { subst, modulo, .. } = result.value else {
                panic!("environment fact should prove goal")
            };
            assert!(stash[subst].is_empty());
            assert!(modulo.is_trivially_true(stash));
        });
    }

    #[test]
    fn generic_impl_where_clause_is_proved_by_nested_impl() {
        with_test_crate(
            "trait Marker {}\ntrait Wrapper {}\nimpl Marker for i32 {}\nimpl<T: Marker> Wrapper for (T,) {}",
            |db, root| {
                let (wrapper, krate) = trait_and_crate(db, root, "Wrapper");
                let mut stash = Stash::new();
                let egraph = VersionedEGraph::new();
                let integer = stash.alloc(Ty::Int(IntTy::I32));
                let elements = stash.alloc_slice(&[integer]);
                let self_ty = stash.alloc(Ty::Tuple(elements));
                let args = stash.alloc_slice(&[]);
                let assumptions = stash.alloc_slice::<Assumption>(&[]);
                let canonical = canonicalize_goal(
                    db,
                    &stash,
                    &egraph,
                    Version::ROOT,
                    krate,
                    Universe::ROOT,
                    true,
                    assumptions,
                    Goal::Atom(Atom::TraitImpl {
                        self_ty,
                        trait_ref: TraitRef {
                            trait_sym: wrapper,
                            args,
                        },
                    }),
                );
                let result = GoalQuery::new(db, canonical.data).prove(db);
                let (stash, result) = result.open();
                let QueryResultData::Yes { modulo, .. } = result.value else {
                    panic!("nested impl obligations should prove")
                };
                assert!(modulo.is_trivially_true(stash));
            },
        );
    }

    #[test]
    fn exhaustive_local_search_without_candidate_is_no() {
        with_test_crate("trait Marker {}", |db, root| {
            let (marker, krate) = trait_and_crate(db, root, "Marker");
            let mut stash = Stash::new();
            let egraph = VersionedEGraph::new();
            let self_ty = stash.alloc(Ty::Bool);
            let args = stash.alloc_slice(&[]);
            let assumptions = stash.alloc_slice::<Assumption>(&[]);
            let canonical = canonicalize_goal(
                db,
                &stash,
                &egraph,
                Version::ROOT,
                krate,
                Universe::ROOT,
                true,
                assumptions,
                Goal::Atom(Atom::TraitImpl {
                    self_ty,
                    trait_ref: TraitRef {
                        trait_sym: marker,
                        args,
                    },
                }),
            );
            let result = GoalQuery::new(db, canonical.data).prove(db);
            assert!(matches!(result.root().value, QueryResultData::No));
        });
    }

    #[test]
    fn inert_crate_inner_attribute_preserves_complete_ground_search() {
        with_test_crate(
            "#![allow(dead_code)]\ntrait Marker {}",
            assert_ground_bool_marker_is_no,
        );
    }

    #[test]
    fn inert_inline_module_inner_attribute_preserves_complete_ground_search() {
        with_test_crate(
            "trait Marker {}\nmod nested { #![warn(dead_code)] }",
            assert_ground_bool_marker_is_no,
        );
    }

    #[test]
    fn active_inner_attribute_keeps_ground_search_incomplete() {
        with_test_crate(
            "#![cfg(feature = \"candidate\")]\ntrait Marker {}",
            assert_ground_bool_marker_is_maybe,
        );
    }

    #[test]
    fn unresolved_derive_prevents_ground_no() {
        with_test_crate(
            "#[derive(Custom)] struct Candidate;\ntrait Marker {}",
            assert_ground_bool_marker_is_maybe,
        );
    }

    #[test]
    fn derive_on_unsupported_item_prevents_ground_no() {
        with_test_crate(
            "#[derive(Custom)] union Candidate { value: bool }\ntrait Marker {}",
            assert_ground_bool_marker_is_maybe,
        );
    }

    #[test]
    fn unexpanded_attribute_macro_prevents_ground_no() {
        with_test_crate(
            "#[external_attr] struct Candidate;\ntrait Marker {}",
            assert_ground_bool_marker_is_maybe,
        );
    }

    #[test]
    fn unexpanded_attribute_on_use_prevents_ground_no() {
        with_test_crate(
            "#[external_attr] use self::Marker;\ntrait Marker {}",
            assert_ground_bool_marker_is_maybe,
        );
    }

    #[test]
    fn active_attribute_use_cannot_redirect_impl_to_false_yes() {
        with_test_crate(
            "trait Marker {}\n#[external_attr] use self::Marker as Target;\nimpl Target for bool {}",
            assert_ground_bool_marker_is_maybe,
        );
    }

    #[test]
    fn active_attribute_impl_cannot_produce_false_yes() {
        with_test_crate(
            "trait Marker {}\n#[cfg(any())] impl Marker for bool {}",
            assert_ground_bool_marker_is_maybe,
        );
    }

    #[test]
    fn active_attribute_module_cannot_expose_child_impl() {
        with_test_crate(
            "trait Marker {}\n#[cfg(any())] mod hidden { impl Marker for bool {} }",
            assert_ground_bool_marker_is_maybe,
        );
    }

    #[test]
    fn active_attribute_macro_definition_cannot_emit_impl() {
        with_test_crate(
            "trait Marker {}\n#[cfg(any())] macro_rules! make { () => { impl Marker for bool {} } }\nmake!();",
            assert_ground_bool_marker_is_maybe,
        );
    }

    #[test]
    fn active_attribute_macro_invocation_cannot_emit_impl() {
        with_test_crate(
            "trait Marker {}\nmacro_rules! make { () => { impl Marker for bool {} } }\n#[cfg(any())] make!();",
            assert_ground_bool_marker_is_maybe,
        );
    }

    #[test]
    fn successful_item_macro_keeps_ground_negative_search_complete() {
        with_test_crate(
            "trait Marker {}\nmacro_rules! make { () => { struct Generated; } }\nmake!();",
            assert_ground_bool_marker_is_no,
        );
    }

    #[test]
    fn unresolved_item_macro_prevents_ground_no() {
        with_test_crate(
            "missing!();\ntrait Marker {}",
            assert_ground_bool_marker_is_maybe,
        );
    }

    #[test]
    fn failed_resolved_item_macro_prevents_ground_no_without_panicking() {
        with_test_crate(
            "macro_rules! make { ($name:ident) => { struct $name; } }\nmake!(Generated);\ntrait Marker {}",
            assert_ground_bool_marker_is_maybe,
        );
    }

    #[test]
    fn malformed_item_macro_output_prevents_ground_no_without_panicking() {
        with_test_crate(
            "macro_rules! bad { () => { impl } }\nbad!();\ntrait Marker {}",
            assert_ground_bool_marker_is_maybe,
        );
    }

    #[test]
    fn recursive_item_macro_hits_expansion_limit_and_returns_maybe() {
        with_test_crate(
            "macro_rules! again { () => { again!(); } }\nagain!();\ntrait Marker {}",
            assert_ground_bool_marker_is_maybe,
        );
    }

    #[test]
    fn unresolved_trait_impl_head_prevents_ground_no() {
        with_test_crate(
            "impl Missing for bool {}\ntrait Marker {}",
            assert_ground_bool_marker_is_maybe,
        );
    }

    #[test]
    fn unconditional_environment_answer_cancels_and_cleans_impl_sibling() {
        with_test_crate("trait Marker {}\nimpl Marker for bool {}", |db, root| {
            let (marker, krate) = trait_and_crate(db, root, "Marker");
            let mut stash = Stash::new();
            let egraph = VersionedEGraph::new();
            let self_ty = stash.alloc(Ty::Bool);
            let args = stash.alloc_slice(&[]);
            let trait_ref = TraitRef {
                trait_sym: marker,
                args,
            };
            let assumptions = stash.alloc_slice(&[Assumption::TraitImpl { self_ty, trait_ref }]);
            let canonical = canonicalize_goal(
                db,
                &stash,
                &egraph,
                Version::ROOT,
                krate,
                Universe::ROOT,
                true,
                assumptions,
                Goal::Atom(Atom::TraitImpl { self_ty, trait_ref }),
            );
            let result = GoalQuery::new(db, canonical.data).prove(db);
            assert!(matches!(result.root().value, QueryResultData::Yes { .. }));
        });
    }

    #[test]
    fn inductive_impl_cycle_is_no() {
        with_test_crate(
            "trait Recursive {}\nimpl<T: Recursive> Recursive for T {}",
            |db, root| {
                let (recursive, krate) = trait_and_crate(db, root, "Recursive");
                let mut stash = Stash::new();
                let egraph = VersionedEGraph::new();
                let self_ty = stash.alloc(Ty::Bool);
                let args = stash.alloc_slice(&[]);
                let assumptions = stash.alloc_slice::<Assumption>(&[]);
                let canonical = canonicalize_goal(
                    db,
                    &stash,
                    &egraph,
                    Version::ROOT,
                    krate,
                    Universe::ROOT,
                    true,
                    assumptions,
                    Goal::Atom(Atom::TraitImpl {
                        self_ty,
                        trait_ref: TraitRef {
                            trait_sym: recursive,
                            args,
                        },
                    }),
                );
                let result = GoalQuery::new(db, canonical.data).prove(db);
                assert!(matches!(result.root().value, QueryResultData::No));
            },
        );
    }

    #[test]
    fn local_trait_defining_predicates_are_candidate_obligations() {
        with_test_crate(
            "trait Bound {}\ntrait Needs<T> where T: Bound {}\nimpl Needs<bool> for i32 {}",
            |db, root| {
                let (needs, krate) = trait_and_crate(db, root, "Needs");
                let mut stash = Stash::new();
                let egraph = VersionedEGraph::new();
                let self_ty = stash.alloc(Ty::Int(IntTy::I32));
                let argument = stash.alloc(Ty::Bool);
                let args = stash.alloc_slice(&[argument]);
                let assumptions = stash.alloc_slice::<Assumption>(&[]);
                let canonical = canonicalize_goal(
                    db,
                    &stash,
                    &egraph,
                    Version::ROOT,
                    krate,
                    Universe::ROOT,
                    true,
                    assumptions,
                    Goal::Atom(Atom::TraitImpl {
                        self_ty,
                        trait_ref: TraitRef {
                            trait_sym: needs,
                            args,
                        },
                    }),
                );
                let result = GoalQuery::new(db, canonical.data).prove(db);
                assert!(matches!(result.root().value, QueryResultData::No));
            },
        );

        with_test_crate(
            "trait Bound {}\ntrait Needs<T> where T: Bound {}\nimpl Bound for bool {}\nimpl Needs<bool> for i32 {}",
            |db, root| {
                let (needs, krate) = trait_and_crate(db, root, "Needs");
                let mut stash = Stash::new();
                let egraph = VersionedEGraph::new();
                let self_ty = stash.alloc(Ty::Int(IntTy::I32));
                let argument = stash.alloc(Ty::Bool);
                let args = stash.alloc_slice(&[argument]);
                let assumptions = stash.alloc_slice::<Assumption>(&[]);
                let canonical = canonicalize_goal(
                    db,
                    &stash,
                    &egraph,
                    Version::ROOT,
                    krate,
                    Universe::ROOT,
                    true,
                    assumptions,
                    Goal::Atom(Atom::TraitImpl {
                        self_ty,
                        trait_ref: TraitRef {
                            trait_sym: needs,
                            args,
                        },
                    }),
                );
                let result = GoalQuery::new(db, canonical.data).prove(db);
                assert!(matches!(result.root().value, QueryResultData::Yes { .. }));
            },
        );
    }

    #[test]
    fn bare_flexible_self_without_environment_is_maybe() {
        with_test_crate("trait Marker {}\nimpl Marker for bool {}", |db, root| {
            let (marker, krate) = trait_and_crate(db, root, "Marker");
            let mut stash = Stash::new();
            let mut egraph = VersionedEGraph::new();
            let variable = egraph.alloc_var(Version::ROOT, Universe::ROOT);
            let self_ty = stash.alloc(Ty::InferVar(variable));
            let args = stash.alloc_slice(&[]);
            let assumptions = stash.alloc_slice::<Assumption>(&[]);
            let canonical = canonicalize_goal(
                db,
                &stash,
                &egraph,
                Version::ROOT,
                krate,
                Universe::ROOT,
                true,
                assumptions,
                Goal::Atom(Atom::TraitImpl {
                    self_ty,
                    trait_ref: TraitRef {
                        trait_sym: marker,
                        args,
                    },
                }),
            );
            let result = GoalQuery::new(db, canonical.data).prove(db);
            assert!(matches!(result.root().value, QueryResultData::Maybe { .. }));
        });
    }

    #[test]
    fn proof_depth_limit_is_maybe() {
        let mut source = String::new();
        for level in 0..70 {
            source.push_str(&format!("trait Level{level} {{}}\n"));
        }
        for level in 0..69 {
            source.push_str(&format!(
                "impl Level{level} for bool where bool: Level{} {{}}\n",
                level + 1
            ));
        }
        with_test_crate(&source, |db, root| {
            let (first, krate) = trait_and_crate(db, root, "Level0");
            let mut stash = Stash::new();
            let egraph = VersionedEGraph::new();
            let self_ty = stash.alloc(Ty::Bool);
            let args = stash.alloc_slice(&[]);
            let assumptions = stash.alloc_slice::<Assumption>(&[]);
            let canonical = canonicalize_goal(
                db,
                &stash,
                &egraph,
                Version::ROOT,
                krate,
                Universe::ROOT,
                true,
                assumptions,
                Goal::Atom(Atom::TraitImpl {
                    self_ty,
                    trait_ref: TraitRef {
                        trait_sym: first,
                        args,
                    },
                }),
            );
            let result = GoalQuery::new(db, canonical.data).prove(db);
            assert!(matches!(result.root().value, QueryResultData::Maybe { .. }));
        });
    }

    #[test]
    fn query_local_crate_controls_impl_discovery_and_identity() {
        let mut database = Database::default();
        let first_file = database.add_source_file(
            "first.rs".to_owned(),
            "trait Marker {}\nimpl Marker for bool {}".to_owned(),
        );
        let second_file = database.add_source_file("second.rs".to_owned(), String::new());
        database.attach(|db| {
            let (first_crate, first_root) = setup_root_module(db, first_file);
            let (second_crate, _) = setup_root_module(db, second_file);
            let marker = first_root
                .expanded_module_items(db)
                .iter()
                .find_map(|symbol| match symbol.data(db) {
                    SymbolData::TraitSymbol(marker) => Some(marker),
                    _ => None,
                })
                .unwrap();

            let make_query = |local_crate| {
                let mut stash = Stash::new();
                let egraph = VersionedEGraph::new();
                let self_ty = stash.alloc(Ty::Bool);
                let args = stash.alloc_slice(&[]);
                let assumptions = stash.alloc_slice::<Assumption>(&[]);
                canonicalize_goal(
                    db,
                    &stash,
                    &egraph,
                    Version::ROOT,
                    local_crate,
                    Universe::ROOT,
                    true,
                    assumptions,
                    Goal::Atom(Atom::TraitImpl {
                        self_ty,
                        trait_ref: TraitRef {
                            trait_sym: marker,
                            args,
                        },
                    }),
                )
            };
            let first = GoalQuery::new(db, make_query(first_crate).data);
            let second = GoalQuery::new(db, make_query(second_crate).data);
            assert_ne!(first, second);
            assert!(matches!(
                first.prove(db).root().value,
                QueryResultData::Yes { .. }
            ));
            assert!(matches!(second.prove(db).root().value, QueryResultData::No));
        });
    }

    #[test]
    #[should_panic(expected = "canonical query contains a caller inference variable")]
    fn public_query_rejects_uncanonicalized_caller_variables() {
        with_test_crate("trait Marker {}", |db, root| {
            let (_, krate) = trait_and_crate(db, root, "Marker");
            let mut stash = Stash::new();
            let variable = stash.alloc(Ty::InferVar(sage_ir::ty::InferVarIndex(0)));
            let assumptions = stash.alloc_slice::<Assumption>(&[]);
            let canonical_vars = stash.alloc_slice(&[]);
            let data = sage_ir::check::solve::GoalQueryData {
                local_crate: krate,
                canonical_universe: 0,
                canonical_vars,
                next_response_param: 0,
                assumptions_complete: true,
                assumptions,
                goal: SolverGoal::Prove(Goal::Atom(Atom::Equals(variable, variable))),
            };
            GoalQuery::new(db, Stashed::new(stash, data)).prove(db);
        });
    }

    #[test]
    fn conjunction_retries_after_a_sibling_pins_its_input() {
        with_test_crate("trait Marker {}\nimpl Marker for bool {}", |db, root| {
            let (marker, krate) = trait_and_crate(db, root, "Marker");
            let mut stash = Stash::new();
            let mut egraph = VersionedEGraph::new();
            let variable = egraph.alloc_var(Version::ROOT, Universe::ROOT);
            let self_ty = stash.alloc(Ty::InferVar(variable));
            let boolean = stash.alloc(Ty::Bool);
            let args = stash.alloc_slice(&[]);
            let goals = stash.alloc_slice(&[
                Goal::Atom(Atom::TraitImpl {
                    self_ty,
                    trait_ref: TraitRef {
                        trait_sym: marker,
                        args,
                    },
                }),
                Goal::Atom(Atom::Equals(self_ty, boolean)),
            ]);
            let assumptions = stash.alloc_slice::<Assumption>(&[]);
            let canonical = canonicalize_goal(
                db,
                &stash,
                &egraph,
                Version::ROOT,
                krate,
                Universe::ROOT,
                true,
                assumptions,
                Goal::All(goals),
            );
            let result = GoalQuery::new(db, canonical.data).prove(db);
            assert!(matches!(result.root().value, QueryResultData::Yes { .. }));
        });
    }

    #[test]
    fn implication_assumptions_do_not_leak_to_siblings() {
        with_test_crate("trait Marker {}", |db, root| {
            let (marker, krate) = trait_and_crate(db, root, "Marker");
            let mut stash = Stash::new();
            let boolean = stash.alloc(Ty::Bool);
            let integer = stash.alloc(Ty::Int(IntTy::I32));
            let args = stash.alloc_slice(&[]);
            let trait_ref = TraitRef {
                trait_sym: marker,
                args,
            };
            let local_assumptions = stash.alloc_slice(&[Assumption::TraitImpl {
                self_ty: boolean,
                trait_ref,
            }]);
            let inner_goal = stash.alloc(Goal::Atom(Atom::TraitImpl {
                self_ty: boolean,
                trait_ref,
            }));
            let goals = stash.alloc_slice(&[
                Goal::Implies(local_assumptions, inner_goal),
                Goal::Atom(Atom::TraitImpl {
                    self_ty: integer,
                    trait_ref,
                }),
            ]);
            let assumptions = stash.alloc_slice::<Assumption>(&[]);
            let egraph = VersionedEGraph::new();
            let canonical = canonicalize_goal(
                db,
                &stash,
                &egraph,
                Version::ROOT,
                krate,
                Universe::ROOT,
                true,
                assumptions,
                Goal::All(goals),
            );
            let result = GoalQuery::new(db, canonical.data).prove(db);
            assert!(matches!(result.root().value, QueryResultData::No));
        });
    }

    #[test]
    fn unused_lifetime_binder_does_not_make_impl_search_incomplete() {
        with_test_crate(
            "trait Marker {}\nimpl<'a> Marker for bool {}",
            |db, root| {
                let (marker, krate) = trait_and_crate(db, root, "Marker");
                let mut stash = Stash::new();
                let self_ty = stash.alloc(Ty::Bool);
                let args = stash.alloc_slice(&[]);
                let assumptions = stash.alloc_slice::<Assumption>(&[]);
                let egraph = VersionedEGraph::new();
                let canonical = canonicalize_goal(
                    db,
                    &stash,
                    &egraph,
                    Version::ROOT,
                    krate,
                    Universe::ROOT,
                    true,
                    assumptions,
                    Goal::Atom(Atom::TraitImpl {
                        self_ty,
                        trait_ref: TraitRef {
                            trait_sym: marker,
                            args,
                        },
                    }),
                );
                let result = GoalQuery::new(db, canonical.data).prove(db);
                assert!(matches!(result.root().value, QueryResultData::Yes { .. }));
            },
        );
    }

    #[test]
    fn repeated_impl_generic_rejects_non_repeated_tuple() {
        with_test_crate("trait Pair {}\nimpl<T> Pair for (T, T) {}", |db, root| {
            let (pair, krate) = trait_and_crate(db, root, "Pair");
            let prove = |left: Ty<'_>, right: Ty<'_>| {
                let mut stash = Stash::new();
                let left = stash.alloc(left);
                let right = stash.alloc(right);
                let elements = stash.alloc_slice(&[left, right]);
                let self_ty = stash.alloc(Ty::Tuple(elements));
                let args = stash.alloc_slice(&[]);
                let assumptions = stash.alloc_slice::<Assumption>(&[]);
                let egraph = VersionedEGraph::new();
                let canonical = canonicalize_goal(
                    db,
                    &stash,
                    &egraph,
                    Version::ROOT,
                    krate,
                    Universe::ROOT,
                    true,
                    assumptions,
                    Goal::Atom(Atom::TraitImpl {
                        self_ty,
                        trait_ref: TraitRef {
                            trait_sym: pair,
                            args,
                        },
                    }),
                );
                match GoalQuery::new(db, canonical.data).prove(db).root().value {
                    QueryResultData::Yes { .. } => 0,
                    QueryResultData::Maybe { .. } => 1,
                    QueryResultData::No => 2,
                }
            };
            assert_eq!(prove(Ty::Bool, Ty::Bool), 0);
            assert_eq!(prove(Ty::Bool, Ty::Int(IntTy::I32)), 2);
        });
    }
}

#[cfg(test)]
mod trait_obligation_tests {
    use super::*;

    #[test]
    fn generic_function_use_checks_its_instantiated_bounds() {
        let diagnostics = collect_diagnostics(
            "trait Bound {}\nfn requires<T: Bound>(value: T) {}\nfn caller() { requires(true); }",
        );
        assert!(diagnostics.iter().any(|diagnostic| {
            diagnostic.line == 3 && diagnostic.message == "trait obligation is not satisfied"
        }));
    }

    #[test]
    fn equivalent_obligations_deduplicate_after_inference() {
        let diagnostics = collect_diagnostics(
            "trait Bound {}\nfn requires<T: Bound>(value: T) {}\nfn caller() { requires(true); requires(true); }",
        );
        let failures = diagnostics
            .iter()
            .filter(|diagnostic| diagnostic.message == "trait obligation is not satisfied")
            .count();
        assert_eq!(failures, 1);
    }

    #[test]
    fn function_bounds_prove_from_impls_and_enclosing_assumptions() {
        TestCrate::in_memory(
            "trait Bound {}\nimpl Bound for bool {}\nfn requires<T: Bound>(value: T) {}\nfn concrete() { requires(true); }\nfn generic<T: Bound>(value: T) { requires(value); }",
        )
        .check_ok();
    }

    #[test]
    fn body_environment_elaborates_local_trait_defining_predicates() {
        TestCrate::in_memory(
            "trait Bound {}\ntrait Needs<T> where T: Bound {}\nfn need_bound<T: Bound>(value: T) {}\nfn forward<X, Y>(x: X, y: Y) where X: Needs<Y> { need_bound(y); }",
        )
        .check_ok();
    }

    #[test]
    fn struct_construction_checks_its_instantiated_bounds() {
        let diagnostics = collect_diagnostics(
            "trait Bound {}\nstruct Container<T: Bound> { value: T }\nfn caller() { let value = Container { value: true }; }",
        );
        assert!(diagnostics.iter().any(|diagnostic| {
            diagnostic.line == 3 && diagnostic.message == "trait obligation is not satisfied"
        }));
    }

    #[test]
    fn struct_bounds_converge_after_field_inference() {
        TestCrate::in_memory(
            "trait Bound {}\nimpl Bound for bool {}\nstruct Container<T: Bound> { value: T }\nfn caller() { let value = Container { value: true }; }",
        )
        .check_ok();
    }

    #[test]
    fn enum_variant_construction_checks_instantiated_bounds() {
        let diagnostics = collect_diagnostics(
            "trait Bound {}\nenum Choice<T: Bound> { Tuple(T), Record { value: T } }\nfn caller() { let a = Choice::Tuple(true); let b = Choice::Record { value: true }; }",
        );
        assert!(diagnostics.iter().any(|diagnostic| {
            diagnostic.line == 3 && diagnostic.message == "trait obligation is not satisfied"
        }));
    }

    #[test]
    fn enum_variant_bounds_converge_after_argument_and_field_inference() {
        TestCrate::in_memory(
            "trait Bound {}\nimpl Bound for bool {}\nenum Choice<T: Bound> { Tuple(T), Record { value: T } }\nfn caller() { let a = Choice::Tuple(true); let b = Choice::Record { value: true }; }",
        )
        .check_ok();
    }

    #[test]
    fn explicit_adt_type_use_checks_struct_and_enum_bounds() {
        let diagnostics = collect_diagnostics(
            "trait Bound {}\nstruct Container<T: Bound> { value: T }\nenum Choice<T: Bound> { Value { value: T } }\nfn caller(value: bool) { let a = value as Container<bool>; let b = value as Choice<bool>; }",
        );
        let failures = diagnostics
            .iter()
            .filter(|diagnostic| diagnostic.message == "trait obligation is not satisfied")
            .count();
        assert_eq!(failures, 1, "equivalent ADT obligations should deduplicate");
    }

    #[test]
    fn lifetime_binder_does_not_make_parameter_environment_incomplete() {
        TestCrate::in_memory(
            "trait Bound {}\nimpl Bound for bool {}\nfn requires<'a, T: Bound>(value: T) {}\nfn caller() { requires(true); }",
        )
        .check_ok();
    }
}
