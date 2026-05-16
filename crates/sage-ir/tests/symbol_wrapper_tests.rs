use sage_ir::db::Database;
use sage_ir::item::*;
use sage_ir::lower::file_item_tree;
use sage_ir::module::{CrateNum, DefIndex};
use sage_ir::source::SourceFile;
use sage_ir::symbol::*;
use salsa::Database as _;

#[test]
fn fn_symbol_from_ast() {
    let db = Database::default();
    db.attach(|db| {
        let file = SourceFile::new(db, "lib.rs".to_owned(), "fn foo() {}".to_owned());
        let items = file_item_tree(db, file);
        let fn_ast = match items[0] {
            ItemAst::Function(f) => f,
            _ => panic!("expected function"),
        };
        let fn_sym = FnSymbol::from(fn_ast);
        assert_eq!(fn_sym.as_ast(), Some(fn_ast));
        assert_eq!(fn_sym.as_ext(), None);
    });
}

#[test]
fn struct_symbol_from_ast() {
    let db = Database::default();
    db.attach(|db| {
        let file = SourceFile::new(db, "lib.rs".to_owned(), "struct Foo;".to_owned());
        let items = file_item_tree(db, file);
        let struct_ast = match items[0] {
            ItemAst::Struct(s) => s,
            _ => panic!("expected struct"),
        };
        let struct_sym = StructSymbol::from(struct_ast);
        assert_eq!(struct_sym.as_ast(), Some(struct_ast));
        assert_eq!(struct_sym.as_ext(), None);
    });
}

#[test]
fn enum_symbol_from_ext() {
    let ext = SymExt::new(CrateNum(1), DefIndex(42));
    let enum_sym = EnumSymbol::from(ext);
    assert_eq!(enum_sym.as_ast(), None);
    assert_eq!(enum_sym.as_ext(), Some(ext));
}

#[test]
fn trait_symbol_round_trip() {
    let db = Database::default();
    db.attach(|db| {
        let file = SourceFile::new(db, "lib.rs".to_owned(), "trait Foo {}".to_owned());
        let items = file_item_tree(db, file);
        let trait_ast = match items[0] {
            ItemAst::Trait(t) => t,
            _ => panic!("expected trait"),
        };
        let trait_sym = TraitSymbol::from(trait_ast);
        assert_eq!(trait_sym.as_ast(), Some(trait_ast));
    });
}

#[test]
fn kind_symbols_are_copy() {
    let ext = SymExt::new(CrateNum(0), DefIndex(0));
    let a = FnSymbol::ext(ext);
    let b = a;
    assert_eq!(a, b);
}
