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

#[cfg(test)]
mod trait_system_tests {
    use super::*;
    use sage_ir::local_syms::impls::local_impls;
    use sage_ir::scope::ScopeSymbol;
    use sage_ir::symbol::{SymbolData, TraitSymbol};
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
    use sage_ir::check::infer::unify::try_unify;
    use sage_ir::check::infer::version::{Universe, Version};
    use sage_ir::check::solve::{
        AppliedCertainty, Assumption, Atom, CanonicalVarRole, Goal, GoalResult, QueryResultData,
        apply_query_response, canonicalize_goal, extract_query_result, instantiate_query,
    };
    use sage_ir::generic_param::{AlphaEquivParam, GenericParam, GenericParamKind};
    use sage_ir::scope::ScopeSymbol;
    use sage_ir::symbol::{SymbolData, TraitSymbol};
    use sage_ir::ty::{IntTy, Ty};

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
            let Goal::Atom(Atom::Equals(left, right)) = proof.goal else {
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
        Assumption, Atom, Goal, GoalQuery, QueryResultData, canonicalize_goal,
    };
    use sage_ir::scope::ScopeSymbol;
    use sage_ir::symbol::{SymbolData, TraitSymbol};
    use sage_ir::ty::{IntTy, TraitRef, Ty};

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
            let QueryResultData::Yes { subst, modulo } = result.value else {
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
                goal: Goal::Atom(Atom::Equals(variable, variable)),
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
