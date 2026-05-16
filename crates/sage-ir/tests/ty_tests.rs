use sage_ir::db::Database;
use sage_ir::item::ItemAst;
use sage_ir::lower::file_item_tree;
use sage_ir::name::Name;
use sage_ir::source::SourceFile;
use sage_ir::symbol::Symbol;
use sage_ir::ty::*;
use sage_stash::{Ptr, Stash};
use salsa::Database as _;

#[test]
fn ty_adt_round_trip() {
    let db = Database::default();
    db.attach(|db| {
        let file = SourceFile::new(db, "lib.rs".to_owned(), "struct Foo;".to_owned());
        let items = file_item_tree(db, file);
        let sym = match items[0] {
            ItemAst::Struct(s) => Symbol::ast(ItemAst::Struct(s)),
            _ => panic!("expected struct"),
        };

        let mut stash = Stash::new();
        let i32_ty = stash.alloc(Ty {
            data: TyData::Int(IntTy::I32),
        });
        let args = stash.alloc_slice(&[stash[i32_ty]]);
        let adt = stash.alloc(Ty {
            data: TyData::Adt(sym, args),
        });

        match stash[adt].data {
            TyData::Adt(s, a) => {
                assert_eq!(s, sym);
                assert_eq!(stash[a].len(), 1);
                assert!(matches!(stash[a][0].data, TyData::Int(IntTy::I32)));
            }
            _ => panic!("expected Adt"),
        }
    });
}

#[test]
fn binder_fn_sig_round_trip() {
    let db = Database::default();
    db.attach(|db| {
        let mut stash = Stash::new();

        let bound_vars = stash.alloc_slice(&[
            BoundVarInfo {
                kind: BoundVarKind::Type,
            },
            BoundVarInfo {
                kind: BoundVarKind::Type,
            },
        ]);

        let bv0 = Ty {
            data: TyData::BoundVar(BoundVar {
                binder_index: 0,
                param_index: 0,
            }),
        };
        let bv1 = Ty {
            data: TyData::BoundVar(BoundVar {
                binder_index: 0,
                param_index: 1,
            }),
        };

        let params = stash.alloc_slice(&[bv0, bv1]);
        let ret = stash.alloc(bv0);
        let fn_sig = FnSig { params, ret };
        let binder: Binder<'_, FnSig<'_>> = Binder::new(fn_sig, bound_vars);
        let binder_ptr = stash.alloc(binder);

        let stored = &stash[binder_ptr];
        assert_eq!(stash[stored.bound_vars].len(), 2);
        assert_eq!(stash[stored.value.params].len(), 2);
        match stash[stored.value.ret].data {
            TyData::BoundVar(bv) => {
                assert_eq!(bv.binder_index, 0);
                assert_eq!(bv.param_index, 0);
            }
            _ => panic!("expected BoundVar"),
        }

        // Name is not used but suppress warning
        let _ = Name::new(db, "test".to_owned());
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
