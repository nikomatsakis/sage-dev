#![feature(rustc_private)]

use std::path::PathBuf;

use sage_ir::symbol::{FnSymbol, SymbolData};
use sage_oracle_harness::{Fixture, combined};

#[test]
fn into_iter_iterator_proof_reads_only_relevant_external_headers() {
    let fixture = Fixture::SingleFile(
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../test-fixtures/solver/external_iterator_impl.rs"),
    );

    let (diagnostics, cold, warm) = combined::with_proxy_database(&fixture, |database, files| {
        let refs: Vec<_> = files
            .iter()
            .map(|(path, source)| (path.as_str(), source.as_str()))
            .collect();
        sage_test_harness::with_test_crate_files_twice_using_db(database, &refs, |db, root| {
            let function = root
                .expanded_module_items(db)
                .iter()
                .find_map(|symbol| match symbol.data(db) {
                    SymbolData::FnSymbol(FnSymbol::Local(function))
                        if function.name(db).text(db) == "check" =>
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
                .expect("check function");
            function
                .body(db)
                .diagnostics
                .iter()
                .map(|diagnostic| diagnostic.render(db))
                .collect::<Vec<_>>()
        })
    });

    assert!(
        diagnostics.is_empty(),
        "{diagnostics:#?}\ncold trace:\n{cold}\nwarm trace:\n{warm}"
    );
    let relevant: Vec<_> = cold
        .lines()
        .filter(|line| line.contains("tcx::relevant_trait_impls"))
        .collect();
    assert_eq!(relevant.len(), 2, "cold trace:\n{cold}");
    assert!(
        relevant.iter().all(|line| line.contains("Some(Adt(")),
        "both Iterator and nested Allocator lookup need rigid ADT heads:\n{cold}"
    );
    let headers: Vec<_> = cold
        .lines()
        .filter(|line| line.contains("tcx::impl_signature"))
        .collect();
    assert_eq!(headers.len(), 2, "cold trace:\n{cold}");
    assert!(
        !cold.contains("tcx::associated_items"),
        "truth proof must not read associated items or values:\n{cold}"
    );
    let local_bodies = cold
        .lines()
        .filter(|line| line.contains("LocalFnSym") && line.contains("::body_"))
        .count();
    assert_eq!(
        local_bodies, 1,
        "only the requested `check` body may be read; no callee body is relevant:\n{cold}"
    );
    assert!(
        !warm.contains("tcx::relevant_trait_impls") && !warm.contains("tcx::impl_signature"),
        "unchanged proof must reuse metadata queries:\n{warm}"
    );
}
