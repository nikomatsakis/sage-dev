#![feature(register_tool)]
#![register_tool(sage)]

use sage_ir::db::Database;
use sage_ir::symbol::{CrateNum, DefIndex, SymExt, SymExtKind, Symbol, SymbolData, UseSymbol};

#[test]
#[sage::test(crates.sage_ir.symbols.kind_wrappers)]
fn external_use_roundtrip_preserves_kind_and_identity() {
    let db = Database::default();
    let external = SymExt::new(&db, CrateNum(1), DefIndex(2), SymExtKind::Use);
    let wrapper = UseSymbol::Ext(external);
    let symbol: Symbol<'_> = wrapper.into();

    assert_eq!(symbol, Symbol::from(external));
    let SymbolData::UseSymbol(recovered) = symbol.data(&db) else {
        panic!("external use must recover as a use wrapper");
    };
    assert_eq!(recovered, wrapper);
    assert_eq!(symbol.module(&db), None);
    assert_eq!(symbol.trait_symbol(&db), None);
}
