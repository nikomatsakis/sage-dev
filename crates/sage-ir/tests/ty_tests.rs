use sage_ir::db::Database;
use sage_ir::generic_param::{ExtGenericParam, GenericParam, GenericParamKind};
use sage_ir::item::{ItemAst, ModAst};
use sage_ir::lower::parse_source_file;
use sage_ir::module::ModSymbol;
use sage_ir::name::Name;
use sage_ir::resolve::SourceRoot;
use sage_ir::scope::ScopeSymbol;
use sage_ir::source::SourceFile;
use sage_ir::symbol::Symbol;
use sage_ir::ty::*;
use sage_stash::Stash;
use salsa::Database as _;

#[test]
fn ty_adt_round_trip() {
    let db = Database::default();
    db.attach(|db| {
        let file = SourceFile::new(db, "lib.rs".to_owned(), "struct Foo;".to_owned());
        let source_root = SourceRoot::new(db, vec![file]);
        let root_module = ModSymbol::ast(ModAst::crate_root(db, file));
        let scope = ScopeSymbol::Module(root_module, source_root);
        let items = parse_source_file(db, file);
        let sym = match items[0] {
            ItemAst::Struct(s) => Symbol::local(ItemAst::Struct(s), scope),
            _ => panic!("expected struct"),
        };

        let mut stash = Stash::new();
        let i32_ty = stash.alloc(Ty {
            data: TyData::Int(IntTy::I32),
        });
        let args = stash.alloc_slice(&[i32_ty]);
        let adt = stash.alloc(Ty {
            data: TyData::Adt(sym, args),
        });

        match stash[adt].data {
            TyData::Adt(s, a) => {
                assert_eq!(s, sym);
                assert_eq!(stash[a].len(), 1);
                assert!(matches!(stash[stash[a][0]].data, TyData::Int(IntTy::I32)));
            }
            _ => panic!("expected Adt"),
        }
    });
}

#[test]
fn binder_fn_sig_round_trip() {
    let db = Database::default();
    db.attach(|db| {
        let file = SourceFile::new(db, "lib.rs".to_owned(), "fn foo<T>() {}".to_owned());
        let source_root = SourceRoot::new(db, vec![file]);
        let root_module = ModSymbol::ast(ModAst::crate_root(db, file));
        let scope = ScopeSymbol::Module(root_module, source_root);
        let items = parse_source_file(db, file);
        let fn_sym = Symbol::local(items[0], scope);

        let param_t = ExtGenericParam::new(
            db,
            GenericParamKind::Type,
            Some(Name::new(db, "T".to_owned())),
            fn_sym,
            0,
        );
        let gp = GenericParam::Ext(param_t);

        let mut stash = Stash::new();
        let generics = stash.alloc_slice(&[gp]);

        let param_ty = Ty {
            data: TyData::Param(gp),
        };
        let p0 = stash.alloc(param_ty);
        let p1 = stash.alloc(param_ty);
        let params = stash.alloc_slice(&[p0, p1]);
        let ret = stash.alloc(param_ty);
        let fn_sig = FnSig { params, ret };
        let binder: Binder<'_, FnSig<'_>> = Binder::new(fn_sig, generics);
        let binder_ptr = stash.alloc(binder);

        let stored = &stash[binder_ptr];
        assert_eq!(stash[stored.generics].len(), 1);
        assert_eq!(stash[stored.value.params].len(), 2);
        match stash[stored.value.ret].data {
            TyData::Param(p) => assert_eq!(p, gp),
            _ => panic!("expected Param"),
        }
    });
}

#[test]
fn struct_sig_round_trip() {
    let db = Database::default();
    db.attach(|db| {
        let mut stash = Stash::new();

        let name_first = Name::new(db, "first".to_owned());
        let name_second = Name::new(db, "second".to_owned());

        let bool_ty = stash.alloc(Ty { data: TyData::Bool });
        let str_ty = stash.alloc(Ty { data: TyData::Str });

        let fields = stash.alloc_slice(&[
            FieldSig {
                name: name_first,
                ty: bool_ty,
            },
            FieldSig {
                name: name_second,
                ty: str_ty,
            },
        ]);

        let sig = StructSig { fields };
        let sig_ptr = stash.alloc(sig);

        let stored = &stash[sig_ptr];
        let fs = &stash[stored.fields];
        assert_eq!(fs.len(), 2);
        assert_eq!(fs[0].name.text(db), "first");
        assert!(matches!(stash[fs[0].ty].data, TyData::Bool));
        assert!(matches!(stash[fs[1].ty].data, TyData::Str));
    });
}

#[test]
fn primitive_types() {
    let mut stash = Stash::new();

    let cases = [
        TyData::Bool,
        TyData::Char,
        TyData::Int(IntTy::I8),
        TyData::Int(IntTy::I128),
        TyData::Uint(UintTy::U64),
        TyData::Uint(UintTy::Usize),
        TyData::Float(FloatTy::F32),
        TyData::Float(FloatTy::F64),
        TyData::Str,
        TyData::Never,
        TyData::Error,
    ];

    for data in &cases {
        let ty = stash.alloc(Ty { data: *data });
        assert_eq!(stash[ty].data, *data);
    }
}
