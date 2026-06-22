mod common;

use sage_ir::Db;
use sage_ir::db::Database;
use sage_ir::generic_param::GenericParamKind;
use sage_ir::item::*;
use sage_ir::lower::parse_source_file;
use sage_ir::module::ModSymbol;
use sage_ir::scope::ScopeSymbol;
use sage_ir::sig_lower::*;
use sage_ir::source::SourceFile;
use sage_ir::symbol::{EnumSymbol, FnSymbol, StructSymbol};
use sage_ir::ty::*;
use sage_ir::types::Mutability;
use salsa::Database as _;

fn test_db(src: &str) -> Database {
    let mut db = Database::default();
    db.add_source_file("lib.rs".to_owned(), src.to_owned());
    db
}

fn setup<'db>(db: &'db Database) -> (ModSymbol<'db>, Vec<LocalModItemSym<'db>>) {
    let file = db.source_file("lib.rs").unwrap();
    let root = ModSymbol::ast(ModAst::crate_root(db, file));
    let items = parse_source_file(db, file).clone();
    (root, items)
}

#[test]
fn fn_identity_generic() {
    let db = test_db("fn identity<T>(x: T) -> T {}");
    db.attach(|db| {
        let (module, items) = setup(db);
        let scope = ScopeSymbol::Module(module);
        let fn_ast = match items[0] {
            LocalModItemSym::Function(f) => f,
            _ => panic!("expected function"),
        };

        let sig = fn_signature(db, FnSymbol::local(fn_ast, scope), scope);
        let stash = sig.stash();
        let binder = sig.root();

        let generics = &stash[binder.generics];
        assert_eq!(generics.len(), 1);
        assert_eq!(generics[0].kind(db), GenericParamKind::Type);

        let fn_sig = &binder.value;
        let params = &stash[fn_sig.params];
        assert_eq!(params.len(), 1);
        match stash[params[0]].data {
            Ty::Param(p) => assert_eq!(p, generics[0]),
            other => panic!("expected Param, got {other:?}"),
        }

        let ret = &stash[fn_sig.ret];
        match ret.data {
            Ty::Param(p) => assert_eq!(p, generics[0]),
            other => panic!("expected Param, got {other:?}"),
        }
    });
}

#[test]
fn fn_add_primitives() {
    let db = test_db("fn add(a: i32, b: i32) -> i32 {}");
    db.attach(|db| {
        let (module, items) = setup(db);
        let scope = ScopeSymbol::Module(module);
        let fn_ast = match items[0] {
            LocalModItemSym::Function(f) => f,
            _ => panic!("expected function"),
        };

        let sig = fn_signature(db, FnSymbol::local(fn_ast, scope), scope);
        let stash = sig.stash();
        let binder = sig.root();

        assert!(stash[binder.generics].is_empty());

        let fn_sig = &binder.value;
        let params = &stash[fn_sig.params];
        assert_eq!(params.len(), 2);
        assert!(matches!(stash[params[0]].data, Ty::Int(IntTy::I32)));
        assert!(matches!(stash[params[1]].data, Ty::Int(IntTy::I32)));

        let ret = &stash[fn_sig.ret];
        assert!(matches!(ret.data, Ty::Int(IntTy::I32)));
    });
}

#[test]
fn struct_pair_generic() {
    let db = test_db("struct Pair<A, B> { first: A, second: B }");
    db.attach(|db| {
        let (module, items) = setup(db);
        let scope = ScopeSymbol::Module(module);
        let struct_ast = match items[0] {
            LocalModItemSym::Struct(s) => s,
            _ => panic!("expected struct"),
        };

        let sig = struct_signature(db, StructSymbol::local(struct_ast, scope), scope);
        let stash = sig.stash();
        let binder = sig.root();

        let generics = &stash[binder.generics];
        assert_eq!(generics.len(), 2);

        let fields = &stash[binder.value.fields];
        assert_eq!(fields.len(), 2);
        assert_eq!(fields[0].name.text(db), "first");
        match stash[fields[0].ty].data {
            Ty::Param(p) => assert_eq!(p, generics[0]),
            other => panic!("expected Param(A), got {other:?}"),
        }
        assert_eq!(fields[1].name.text(db), "second");
        match stash[fields[1].ty].data {
            Ty::Param(p) => assert_eq!(p, generics[1]),
            other => panic!("expected Param(B), got {other:?}"),
        }
    });
}

#[test]
fn fn_takes_ref() {
    let db = test_db("fn takes_ref(x: &str) -> &[u8] {}");
    db.attach(|db| {
        let (module, items) = setup(db);
        let scope = ScopeSymbol::Module(module);
        let fn_ast = match items[0] {
            LocalModItemSym::Function(f) => f,
            _ => panic!("expected function"),
        };

        let sig = fn_signature(db, FnSymbol::local(fn_ast, scope), scope);
        let stash = sig.stash();
        let fn_sig = &sig.root().value;

        let params = &stash[fn_sig.params];
        assert_eq!(params.len(), 1);
        match stash[params[0]].data {
            Ty::Ref(inner, sage_ir::types::Mutability::Shared, Lifetime::Erased) => {
                assert!(matches!(stash[inner].data, Ty::Str));
            }
            _ => panic!("expected &str, got {:?}", stash[params[0]].data),
        }

        let ret = &stash[fn_sig.ret];
        match ret.data {
            Ty::Ref(inner, sage_ir::types::Mutability::Shared, Lifetime::Erased) => {
                match stash[inner].data {
                    Ty::Slice(elem) | Ty::Array(elem, _) => {
                        assert!(matches!(stash[elem].data, Ty::Uint(UintTy::U8)));
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
    let db = test_db("enum Option<T> { None, Some { value: T } }");
    db.attach(|db| {
        let (module, items) = setup(db);
        let scope = ScopeSymbol::Module(module);
        let enum_ast = match items[0] {
            LocalModItemSym::Enum(e) => e,
            _ => panic!("expected enum"),
        };

        let sig = enum_signature(db, EnumSymbol::local(enum_ast, scope), scope);
        let stash = sig.stash();
        let binder = sig.root();

        let generics = &stash[binder.generics];
        assert_eq!(generics.len(), 1);

        let variants = &stash[binder.value.variants];
        assert_eq!(variants.len(), 2);
        assert_eq!(variants[0].name.text(db), "None");
        assert!(stash[variants[0].fields].is_empty());

        assert_eq!(variants[1].name.text(db), "Some");
        let some_fields = &stash[variants[1].fields];
        assert_eq!(some_fields.len(), 1);
        match stash[some_fields[0].ty].data {
            Ty::Param(p) => assert_eq!(p, generics[0]),
            other => panic!("expected Param(T), got {other:?}"),
        }
    });
}

#[test]
fn fn_no_return_type_is_unit() {
    let db = test_db("fn noop() {}");
    db.attach(|db| {
        let (module, items) = setup(db);
        let scope = ScopeSymbol::Module(module);
        let fn_ast = match items[0] {
            LocalModItemSym::Function(f) => f,
            _ => panic!("expected function"),
        };

        let sig = fn_signature(db, FnSymbol::local(fn_ast, scope), scope);
        let stash = sig.stash();
        let fn_sig = &sig.root().value;

        let ret = &stash[fn_sig.ret];
        match ret.data {
            Ty::Tuple(elems) => assert!(stash[elems].is_empty()),
            _ => panic!("expected unit tuple for no return type"),
        }
    });
}

// ---------------------------------------------------------------------------
// Self type resolution in impl blocks
// ---------------------------------------------------------------------------

fn find_impl_method<'db>(
    db: &'db dyn salsa::Database,
    items: &[LocalModItemSym<'db>],
    method_name: &str,
) -> FnAst<'db> {
    for item in items {
        if let LocalModItemSym::Impl(impl_ast) = item {
            for sub in impl_ast.items(db) {
                if let LocalModItemSym::Function(f) = sub {
                    if f.name(db).text(db) == method_name {
                        return *f;
                    }
                }
            }
        }
    }
    panic!("method {method_name} not found in any impl block");
}

#[test]
fn impl_method_self_return_resolves() {
    let db = test_db("struct Foo {} impl Foo { fn make() -> Self {} }");
    db.attach(|db| {
        let (module, items) = setup(db);
        let scope = ScopeSymbol::Module(module);

        let method = find_impl_method(db, &items, "make");

        let foo_struct = match items[0] {
            LocalModItemSym::Struct(s) => s,
            _ => panic!("expected struct"),
        };
        let foo_sym = sage_ir::symbol::Symbol::local(LocalModItemSym::Struct(foo_struct), scope);
        let mut stash = sage_stash::Stash::new();
        let empty_args = stash.alloc_slice::<sage_stash::Ptr<Ty>>(&[]);
        let self_ty = Ty {
            data: Ty::Adt(foo_sym, empty_args),
        };

        let sig = lower_fn_sig(db, method, scope, Some(self_ty), &stash);
        let sig_stash = sig.stash();
        let fn_sig = &sig.root().value;

        let ret = &sig_stash[fn_sig.ret];
        match ret.data {
            Ty::Adt(sym, args) => {
                assert_eq!(sym, foo_sym);
                assert!(sig_stash[args].is_empty());
            }
            other => panic!("expected Adt(Foo, []), got {other:?}"),
        }
    });
}

#[test]
fn impl_method_ref_self_param() {
    let db = test_db("struct Foo {} impl Foo { fn bar(&self) -> Self {} }");
    db.attach(|db| {
        let (module, items) = setup(db);
        let scope = ScopeSymbol::Module(module);

        let method = find_impl_method(db, &items, "bar");

        let foo_struct = match items[0] {
            LocalModItemSym::Struct(s) => s,
            _ => panic!("expected struct"),
        };
        let foo_sym = sage_ir::symbol::Symbol::local(LocalModItemSym::Struct(foo_struct), scope);
        let mut stash = sage_stash::Stash::new();
        let empty_args = stash.alloc_slice::<sage_stash::Ptr<Ty>>(&[]);
        let self_ty = Ty {
            data: Ty::Adt(foo_sym, empty_args),
        };

        let sig = lower_fn_sig(db, method, scope, Some(self_ty), &stash);
        let sig_stash = sig.stash();
        let fn_sig = &sig.root().value;

        let params = &sig_stash[fn_sig.params];
        assert_eq!(params.len(), 1);
        match sig_stash[params[0]].data {
            Ty::Ref(inner, Mutability::Shared, Lifetime::Erased) => match sig_stash[inner].data {
                Ty::Adt(sym, args) => {
                    assert_eq!(sym, foo_sym);
                    assert!(sig_stash[args].is_empty());
                }
                other => panic!("expected Adt(Foo) inside &self, got {other:?}"),
            },
            other => panic!("expected &Foo for &self param, got {other:?}"),
        }

        let ret = &sig_stash[fn_sig.ret];
        match ret.data {
            Ty::Adt(sym, _) => assert_eq!(sym, foo_sym),
            other => panic!("expected Adt(Foo) return, got {other:?}"),
        }
    });
}

#[test]
fn generic_impl_self_resolves_with_params() {
    let db = test_db(
        "struct Wrapper<T> { val: T } impl<T> Wrapper<T> { fn into_self(&self) -> Self {} }",
    );
    db.attach(|db| {
        let (module, items) = setup(db);
        let scope = ScopeSymbol::Module(module);

        let method = find_impl_method(db, &items, "into_self");

        let wrapper_struct = match items[0] {
            LocalModItemSym::Struct(s) => s,
            _ => panic!("expected struct"),
        };
        let wrapper_sym =
            sage_ir::symbol::Symbol::local(LocalModItemSym::Struct(wrapper_struct), scope);

        let struct_sig = struct_signature(db, StructSymbol::local(wrapper_struct, scope), scope);
        let struct_stash = struct_sig.stash();
        let struct_binder = struct_sig.root();
        let struct_generics = &struct_stash[struct_binder.generics];
        assert_eq!(struct_generics.len(), 1);
        let gp = struct_generics[0];

        let mut stash = sage_stash::Stash::new();
        let param_ty = Ty {
            data: Ty::Param(gp),
        };
        let param_ptr = stash.alloc(param_ty);
        let args = stash.alloc_slice(&[param_ptr]);
        let self_ty = Ty {
            data: Ty::Adt(wrapper_sym, args),
        };

        let sig = lower_fn_sig(db, method, scope, Some(self_ty), &stash);
        let sig_stash = sig.stash();
        let fn_sig = &sig.root().value;

        let params = &sig_stash[fn_sig.params];
        assert_eq!(params.len(), 1);
        match sig_stash[params[0]].data {
            Ty::Ref(inner, Mutability::Shared, Lifetime::Erased) => match sig_stash[inner].data {
                Ty::Adt(sym, args) => {
                    assert_eq!(sym, wrapper_sym);
                    let type_args = &sig_stash[args];
                    assert_eq!(type_args.len(), 1);
                    match sig_stash[type_args[0]].data {
                        Ty::Param(p) => assert_eq!(p, gp),
                        other => {
                            panic!("expected Param(T) in wrapper type args, got {other:?}")
                        }
                    }
                }
                other => panic!("expected Adt(Wrapper<T>) in &self, got {other:?}"),
            },
            other => panic!("expected &Wrapper<T>, got {other:?}"),
        }

        let ret = &sig_stash[fn_sig.ret];
        match ret.data {
            Ty::Adt(sym, args) => {
                assert_eq!(sym, wrapper_sym);
                let type_args = &sig_stash[args];
                assert_eq!(type_args.len(), 1);
                match sig_stash[type_args[0]].data {
                    Ty::Param(p) => assert_eq!(p, gp),
                    other => panic!("expected Param(T) in Self return, got {other:?}"),
                }
            }
            other => panic!("expected Adt(Wrapper<Param>) for Self return, got {other:?}"),
        }
    });
}
