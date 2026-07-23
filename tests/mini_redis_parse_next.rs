#![feature(rustc_private)]

use std::path::PathBuf;

use sage::driver::run_sage_with;
use sage_ir::Db;
use sage_ir::scope::Edition;
use sage_ir::symbol::{FnSymbol, ImplSymbol, Symbol, SymbolData};
use sage_ir::ty::{TraitItemDef, Ty};
use sage_ir::tytree::{CallDispatch, TyExprData};

fn find_parse_next<'db>(
    db: &'db dyn Db,
    root: sage_ir::symbol::ModSymbol<'db>,
) -> sage_ir::local_syms::fns::LocalFnSym<'db> {
    let parse = root
        .expanded_module_items(db)
        .iter()
        .find(|symbol| {
            symbol
                .name(db)
                .is_some_and(|(name, _)| name.text(db) == "parse")
        })
        .and_then(|symbol| symbol.module(db))
        .expect("mini-redis parse module");

    parse
        .expanded_module_items(db)
        .iter()
        .filter_map(|symbol| match symbol.data(db) {
            SymbolData::ImplSymbol(ImplSymbol::Local(local_impl)) => Some(local_impl.items(db)),
            _ => None,
        })
        .flat_map(|items| {
            items.stash()[items.root().value]
                .iter()
                .copied()
                .collect::<Vec<_>>()
        })
        .find_map(|item| {
            let TraitItemDef::Function(FnSymbol::Local(function)) = item else {
                return None;
            };
            (function.name(db).text(db) == "next").then_some(function)
        })
        .expect("Parse::next")
}

fn check_parse_next<'db>(db: &'db dyn Db, root: sage_ir::symbol::ModSymbol<'db>) -> Vec<String> {
    let checked = find_parse_next(db, root).body(db);
    let diagnostics: Vec<_> = checked
        .diagnostics
        .iter()
        .map(|diagnostic| diagnostic.render(db))
        .collect();
    if !diagnostics.is_empty() {
        return diagnostics;
    }

    let (stash, body) = checked.body.open_deref();
    let TyExprData::Block(_, Some(ok_or)) = stash[body.root].data else {
        panic!("Parse::next must end in Option::ok_or")
    };
    let TyExprData::ResolvedCall(ok_or_target, ok_or_arguments) = stash[ok_or].data else {
        return vec![format!(
            "Option::ok_or must be a resolved call: {:#?}",
            stash[ok_or]
        )];
    };
    assert_eq!(
        Symbol::from(ok_or_target.function)
            .name(db)
            .unwrap()
            .0
            .text(db),
        "ok_or"
    );
    assert!(matches!(ok_or_target.dispatch, CallDispatch::Direct));
    let [owner] = &stash[ok_or_target.owner_type_args] else {
        panic!("Option::ok_or must retain T = Frame")
    };
    let Ty::Adt(owner_symbol, owner_arguments) = stash[*owner] else {
        panic!("Option::ok_or owner argument must be Frame")
    };
    assert_eq!(
        owner_symbol.name(db).expect("Frame name").0.text(db),
        "Frame"
    );
    assert!(stash[owner_arguments].is_empty());
    let [method] = &stash[ok_or_target.method_type_args] else {
        panic!("Option::ok_or must retain E = ParseError")
    };
    let Ty::Adt(method_symbol, method_arguments) = stash[*method] else {
        panic!("Option::ok_or method argument must be ParseError")
    };
    assert_eq!(
        method_symbol.name(db).expect("ParseError name").0.text(db),
        "ParseError"
    );
    assert!(stash[method_arguments].is_empty());

    let [next, _error] = &stash[ok_or_arguments] else {
        panic!("Option::ok_or must retain its receiver and error argument")
    };
    let TyExprData::ResolvedCall(next_target, next_arguments) = stash[*next].data else {
        panic!("Iterator::next must be a resolved call")
    };
    assert_eq!(
        Symbol::from(next_target.function)
            .name(db)
            .unwrap()
            .0
            .text(db),
        "next"
    );
    let CallDispatch::StaticTrait { self_ty, .. } = next_target.dispatch else {
        panic!("Iterator::next must retain static trait dispatch")
    };
    assert_eq!(stash[next_target.owner_type_args], [self_ty]);
    assert!(stash[next_target.method_type_args].is_empty());
    let [receiver] = &stash[next_arguments] else {
        panic!("Iterator::next must retain its receiver")
    };
    assert!(matches!(
        stash[*receiver].data,
        TyExprData::Ref(_, sage_ir::cst::Mutability::Mut)
    ));

    diagnostics
}

#[test]
fn pinned_mini_redis_parse_next_uses_real_target_and_narrow_queries() {
    let project = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("test-fixtures/mini-redis");
    run_sage_with(&project, &["mini-redis".to_owned()], |sage| {
        assert_eq!(sage.target.name, "mini_redis");
        assert_eq!(sage.target.edition, Edition::Rust2018);
        assert!(sage.target.enabled_features.is_empty());
        assert!(
            sage.target
                .cfgs
                .iter()
                .any(|cfg| cfg.starts_with("target_arch="))
        );
        assert!(sage.direct_dependencies.iter().any(|name| name == "bytes"));
        assert!(
            !sage
                .direct_dependencies
                .iter()
                .any(|name| name.starts_with("opentelemetry")),
            "the default-feature library target must exclude optional otel dependencies"
        );

        let _ = sage.db.take_query_log();
        let diagnostics = check_parse_next(sage.db, sage.root);
        let cold = sage.db.take_query_log();
        let repeated = check_parse_next(sage.db, sage.root);
        let warm = sage.db.take_query_log();

        assert!(
            diagnostics.is_empty(),
            "{diagnostics:#?}\ncold trace:\n{cold}"
        );
        assert!(repeated.is_empty(), "{repeated:#?}\nwarm trace:\n{warm}");
        assert_eq!(
            cold.matches("tcx::inherent_method_candidates").count(),
            2,
            "one name-keyed inherent lookup per method call:\n{cold}"
        );
        assert_eq!(cold.matches("tcx::fn_signature").count(), 2, "{cold}");
        assert_eq!(
            cold.matches("tcx::associated_type_value").count(),
            1,
            "{cold}"
        );
        assert_eq!(
            cold.matches("tcx::relevant_trait_impls").count(),
            2,
            "{cold}"
        );
        assert_eq!(cold.matches("tcx::impl_signature").count(), 2, "{cold}");
        assert_eq!(
            cold.matches("tcx::adt_is_fundamental").count(),
            2,
            "one exact orphan-rule fact per external/external proof:\n{cold}"
        );
        for line in cold.lines().filter_map(|line| line.strip_prefix("tcx::")) {
            let operation = line
                .split_once('(')
                .map_or(line, |(operation, _)| operation);
            assert!(
                [
                    "extern_crate",
                    "module_children",
                    "item_name",
                    "is_builtin_derive",
                    "adt_signature",
                    "trait_signature",
                    "associated_items",
                    "inherent_method_candidates",
                    "relevant_trait_impls",
                    "impl_signature",
                    "associated_type_value",
                    "adt_is_always_sized",
                    "adt_is_fundamental",
                    "fn_signature",
                ]
                .contains(&operation),
                "unexpected metadata query kind {operation:?}:\n{cold}"
            );
        }
        assert!(
            !cold.contains("local_impl_candidates")
                && !cold.contains("LocalImplSym < 'db >::trait_symbol_"),
            "the exact orphan-rule proof must avoid the entire local impl source for the external Iterator goal:\n{cold}"
        );
        assert_eq!(
            cold.matches("LocalImplSym < 'db >::sig_").count(),
            1,
            "only the containing inherent impl header may be lowered:\n{cold}"
        );
        assert_eq!(
            cold.matches("LocalFnSym < 'db >::body_").count(),
            1,
            "only Parse::next may be type-checked:\n{cold}"
        );
        assert!(
            cold.lines()
                .filter(|line| line.starts_with("tcx::"))
                .all(|line| !line.contains("body")),
            "external callee bodies must not be metadata dependencies:\n{cold}"
        );
        assert!(
            !warm.lines().any(|line| line.starts_with("tcx::"))
                && !warm.contains("LocalImplSym < 'db >::sig_")
                && !warm.contains("LocalFnSym < 'db >::body_"),
            "the unchanged body must reread no semantic metadata:\n{warm}"
        );
    });
}
