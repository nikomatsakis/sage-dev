use sage_ir::db::Database;
use sage_ir::item::*;
use sage_ir::lower::parse_source_file;
use sage_ir::module::{CrateNum, DefIndex, ModSymbol};
use sage_ir::resolve::SourceRoot;
use sage_ir::scope::ScopeSymbol;
use sage_ir::source::SourceFile;
use sage_ir::symbol::*;
use salsa::Database as _;

#[test]
fn fn_symbol_from_ast() {
    let db = Database::default();
    db.attach(|db| {
        let file = SourceFile::new(db, "lib.rs".to_owned(), "fn foo() {}".to_owned());
        let source_root = SourceRoot::new(db, vec![file]);
        let root = ModSymbol::ast(ModAst::crate_root(db, file));
        let scope = ScopeSymbol::Module(root, source_root);
        let items = parse_source_file(db, file);
        let fn_ast = match items[0] {
            LocalModItemSym::Function(f) => f,
            _ => panic!("expected function"),
        };
        let fn_sym = FnSymbol::local(fn_ast, scope);
        assert_eq!(fn_sym.as_ast(), Some(fn_ast));
        assert_eq!(fn_sym.as_ext(), None);
    });
}

#[test]
fn struct_symbol_from_ast() {
    let db = Database::default();
    db.attach(|db| {
        let file = SourceFile::new(db, "lib.rs".to_owned(), "struct Foo;".to_owned());
        let source_root = SourceRoot::new(db, vec![file]);
        let root = ModSymbol::ast(ModAst::crate_root(db, file));
        let scope = ScopeSymbol::Module(root, source_root);
        let items = parse_source_file(db, file);
        let struct_ast = match items[0] {
            LocalModItemSym::Struct(s) => s,
            _ => panic!("expected struct"),
        };
        let struct_sym = StructSymbol::local(struct_ast, scope);
        assert_eq!(struct_sym.as_ast(), Some(struct_ast));
        assert_eq!(struct_sym.as_ext(), None);
    });
}

#[test]
fn enum_symbol_from_ext() {
    let ext = SymExt::new(CrateNum(1), DefIndex(42), SymExtKind::Enum);
    let enum_sym = EnumSymbol::from(ext);
    assert_eq!(enum_sym.as_ast(), None);
    assert_eq!(enum_sym.as_ext(), Some(ext));
}

#[test]
fn trait_symbol_round_trip() {
    let db = Database::default();
    db.attach(|db| {
        let file = SourceFile::new(db, "lib.rs".to_owned(), "trait Foo {}".to_owned());
        let source_root = SourceRoot::new(db, vec![file]);
        let root = ModSymbol::ast(ModAst::crate_root(db, file));
        let scope = ScopeSymbol::Module(root, source_root);
        let items = parse_source_file(db, file);
        let trait_ast = match items[0] {
            LocalModItemSym::Trait(t) => t,
            _ => panic!("expected trait"),
        };
        let trait_sym = TraitSymbol::local(trait_ast, scope);
        assert_eq!(trait_sym.as_ast(), Some(trait_ast));
    });
}

#[test]
fn kind_symbols_are_copy() {
    let ext = SymExt::new(CrateNum(0), DefIndex(0), SymExtKind::Fn);
    let a = FnSymbol::ext(ext);
    let b = a;
    assert_eq!(a, b);
}
