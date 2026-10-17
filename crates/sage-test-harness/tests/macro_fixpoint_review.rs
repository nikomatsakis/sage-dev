#![feature(rustc_private)]
#![feature(register_tool)]
#![register_tool(sage)]

use sage_ir::local_syms::mods::module_expansion_complete_for_symbol_listing;
use sage_ir::symbol::{ModSymbol, StructSymbol, SymbolData};
use sage_test_harness::with_test_crate;

#[test]
#[sage::test(crates.sage_ir.modules.expand.recursion_and_fixpoint)]
fn directly_emitted_invocations_are_expanded() {
    with_test_crate(
        r#"
            macro_rules! inner { () => { struct Generated; } }
            macro_rules! outer { () => { inner!(); } }
            outer!();
        "#,
        |db, root| {
            let generated = root
                .expanded_module_items(db)
                .iter()
                .filter(|symbol| {
                    matches!(symbol.data(db),
                        SymbolData::StructSymbol(StructSymbol::Local(item))
                            if item.name(db).text(db) == "Generated")
                })
                .count();
            assert_eq!(generated, 1);
        },
    );
}

#[test]
#[sage::test(crates.sage_ir.modules.expand.recursion_and_fixpoint)]
fn converged_expansion_can_remain_incomplete() {
    with_test_crate("missing!();", |db, root| {
        assert!(root.expanded_module_items(db).is_empty());
        assert!(root.expanded_module_items(db).is_empty());
        let ModSymbol::Local(module) = root else {
            panic!("fixture root must be local");
        };
        assert!(!module_expansion_complete_for_symbol_listing(db, module));
    });
}
