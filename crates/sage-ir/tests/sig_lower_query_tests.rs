mod common;

use sage_ir::db::Database;
use sage_ir::item::*;
use sage_ir::lower::file_item_tree;
use sage_ir::module::ModSymbol;
use sage_ir::resolve::SourceRoot;
use sage_ir::sig_lower::*;
use sage_ir::source::SourceFile;
use sage_ir::ty::*;
use salsa::Database as _;

fn setup<'db>(db: &'db Database, src: &str) -> (SourceRoot, ModSymbol<'db>, Vec<ItemAst<'db>>) {
    let file = SourceFile::new(db, "lib.rs".to_owned(), src.to_owned());
    let source_root = SourceRoot::new(db, vec![file]);
    let root = ModSymbol::ast(ModAst::crate_root(db, file));
    let items = file_item_tree(db, file).clone();
    (source_root, root, items)
}

#[test]
fn fn_identity_generic() {
    let db = Database::default();
    db.attach(|db| {
        let (source_root, module, items) = setup(db, "fn identity<T>(x: T) -> T {}");
        let fn_ast = match items[0] {
            ItemAst::Function(f) => f,
            _ => panic!("expected function"),
        };

        let sig = fn_signature(db, fn_ast, module, source_root);
        let stash = sig.stash();
        let binder = sig.root();

        let bound_vars = &stash[binder.bound_vars];
        assert_eq!(bound_vars.len(), 1);
        assert!(matches!(bound_vars[0].kind, BoundVarKind::Type));

        let fn_sig = &binder.value;
        let params = &stash[fn_sig.params];
        assert_eq!(params.len(), 1);
        assert!(matches!(
            params[0].data,
            TyData::BoundVar(BoundVar {
                binder_index: 0,
                param_index: 0
            })
        ));

        let ret = &stash[fn_sig.ret];
        assert!(matches!(
            ret.data,
            TyData::BoundVar(BoundVar {
                binder_index: 0,
                param_index: 0
            })
        ));
    });
}

#[test]
fn fn_add_primitives() {
    let db = Database::default();
    db.attach(|db| {
        let (source_root, module, items) = setup(db, "fn add(a: i32, b: i32) -> i32 {}");
        let fn_ast = match items[0] {
            ItemAst::Function(f) => f,
            _ => panic!("expected function"),
        };

        let sig = fn_signature(db, fn_ast, module, source_root);
        let stash = sig.stash();
        let binder = sig.root();

        assert!(stash[binder.bound_vars].is_empty());

        let fn_sig = &binder.value;
        let params = &stash[fn_sig.params];
        assert_eq!(params.len(), 2);
        assert!(matches!(params[0].data, TyData::Int(IntTy::I32)));
        assert!(matches!(params[1].data, TyData::Int(IntTy::I32)));

        let ret = &stash[fn_sig.ret];
        assert!(matches!(ret.data, TyData::Int(IntTy::I32)));
    });
}

#[test]
fn struct_pair_generic() {
    let db = Database::default();
    db.attach(|db| {
        let (source_root, module, items) = setup(db, "struct Pair<A, B> { first: A, second: B }");
        let struct_ast = match items[0] {
            ItemAst::Struct(s) => s,
            _ => panic!("expected struct"),
        };

        let sig = struct_signature(db, struct_ast, module, source_root);
        let stash = sig.stash();
        let binder = sig.root();

        let bound_vars = &stash[binder.bound_vars];
        assert_eq!(bound_vars.len(), 2);

        let fields = &stash[binder.value.fields];
        assert_eq!(fields.len(), 2);
        assert_eq!(fields[0].name.text(db), "first");
        assert!(matches!(
            stash[fields[0].ty].data,
            TyData::BoundVar(BoundVar {
                binder_index: 0,
                param_index: 0
            })
        ));
        assert_eq!(fields[1].name.text(db), "second");
        assert!(matches!(
            stash[fields[1].ty].data,
            TyData::BoundVar(BoundVar {
                binder_index: 0,
                param_index: 1
            })
        ));
    });
}

#[test]
fn fn_takes_ref() {
    let db = Database::default();
    db.attach(|db| {
        let (source_root, module, items) = setup(db, "fn takes_ref(x: &str) -> &[u8] {}");
        let fn_ast = match items[0] {
            ItemAst::Function(f) => f,
            _ => panic!("expected function"),
        };

        let sig = fn_signature(db, fn_ast, module, source_root);
        let stash = sig.stash();
        let fn_sig = &sig.root().value;

        let params = &stash[fn_sig.params];
        assert_eq!(params.len(), 1);
        match params[0].data {
            TyData::Ref(inner, sage_ir::types::Mutability::Shared, Lifetime::Erased) => {
                assert!(matches!(stash[inner].data, TyData::Str));
            }
            _ => panic!("expected &str, got {:?}", params[0].data),
        }

        let ret = &stash[fn_sig.ret];
        match ret.data {
            TyData::Ref(inner, sage_ir::types::Mutability::Shared, Lifetime::Erased) => {
                // tree-sitter-rust 0.24 parses [u8] as array_type in this context
                match stash[inner].data {
                    TyData::Slice(elem) | TyData::Array(elem, _) => {
                        assert!(matches!(stash[elem].data, TyData::Uint(UintTy::U8)));
                    }
                    other => panic!("expected [u8], got {other:?}"),
                }
            }
            other => panic!("expected &[u8], got {other:?}"),
        }
    });
}

#[test]
fn enum_with_fields() {
    let db = Database::default();
    db.attach(|db| {
        let (source_root, module, items) = setup(db, "enum Option<T> { None, Some { value: T } }");
        let enum_ast = match items[0] {
            ItemAst::Enum(e) => e,
            _ => panic!("expected enum"),
        };

        let sig = enum_signature(db, enum_ast, module, source_root);
        let stash = sig.stash();
        let binder = sig.root();

        assert_eq!(stash[binder.bound_vars].len(), 1);

        let variants = &stash[binder.value.variants];
        assert_eq!(variants.len(), 2);
        assert_eq!(variants[0].name.text(db), "None");
        assert!(stash[variants[0].fields].is_empty());

        assert_eq!(variants[1].name.text(db), "Some");
        let some_fields = &stash[variants[1].fields];
        assert_eq!(some_fields.len(), 1);
        assert!(matches!(
            stash[some_fields[0].ty].data,
            TyData::BoundVar(BoundVar {
                binder_index: 0,
                param_index: 0
            })
        ));
    });
}

#[test]
fn fn_no_return_type_is_unit() {
    let db = Database::default();
    db.attach(|db| {
        let (source_root, module, items) = setup(db, "fn noop() {}");
        let fn_ast = match items[0] {
            ItemAst::Function(f) => f,
            _ => panic!("expected function"),
        };

        let sig = fn_signature(db, fn_ast, module, source_root);
        let stash = sig.stash();
        let fn_sig = &sig.root().value;

        let ret = &stash[fn_sig.ret];
        match ret.data {
            TyData::Tuple(elems) => assert!(stash[elems].is_empty()),
            _ => panic!("expected unit tuple for no return type"),
        }
    });
}
