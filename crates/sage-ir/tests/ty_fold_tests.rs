use sage_ir::db::Database;
use sage_ir::item::ItemAst;
use sage_ir::lower::parse_source_file;
use sage_ir::name::Name;
use sage_ir::source::SourceFile;
use sage_ir::symbol::Symbol;
use sage_ir::ty::*;
use sage_ir::ty_fold::*;
use sage_ir::types::Mutability;
use sage_stash::Stash;
use salsa::Database as _;

#[test]
fn identity_fold() {
    let db = Database::default();
    db.attach(|db| {
        let file = SourceFile::new(db, "lib.rs".to_owned(), "struct Foo;".to_owned());
        let items = parse_source_file(db, file);
        let sym = match items[0] {
            ItemAst::Struct(s) => Symbol::ast(ItemAst::Struct(s)),
            _ => panic!("expected struct"),
        };

        let mut stash_a = Stash::new();
        let i32_ty = Ty {
            data: TyData::Int(IntTy::I32),
        };
        let i32_ptr = stash_a.alloc(i32_ty);
        let args = stash_a.alloc_slice(&[i32_ty]);
        let adt = Ty {
            data: TyData::Adt(sym, args),
        };
        let inner = stash_a.alloc(adt);
        let ref_ty = Ty {
            data: TyData::Ref(inner, Mutability::Shared, Lifetime::Erased),
        };

        let mut stash_b = Stash::new();
        let mut folder = Identity::new(&stash_a, &mut stash_b);
        let result = folder.fold_ty(ref_ty);

        match result.data {
            TyData::Ref(inner, Mutability::Shared, Lifetime::Erased) => match stash_b[inner].data {
                TyData::Adt(s, a) => {
                    assert_eq!(s, sym);
                    assert_eq!(stash_b[a].len(), 1);
                    assert!(matches!(stash_b[a][0].data, TyData::Int(IntTy::I32)));
                }
                _ => panic!("expected Adt"),
            },
            _ => panic!("expected Ref"),
        }

        let _ = i32_ptr;
    });
}

#[test]
fn instantiate_identity_fn() {
    let db = Database::default();
    db.attach(|db| {
        let mut src = Stash::new();

        let bound_vars = src.alloc_slice(&[BoundVarInfo {
            kind: BoundVarKind::Type,
        }]);

        let bv0 = Ty {
            data: TyData::BoundVar(BoundVar {
                binder_index: 0,
                param_index: 0,
            }),
        };
        let params = src.alloc_slice(&[bv0]);
        let ret = src.alloc(bv0);
        let fn_sig = FnSig { params, ret };
        let binder = Binder::new(fn_sig, bound_vars);

        let i32_ty = Ty {
            data: TyData::Int(IntTy::I32),
        };
        let mut dst = Stash::new();
        let result = instantiate_fn_sig(&src, &mut dst, &binder, vec![i32_ty]);

        let result_params = &dst[result.params];
        assert_eq!(result_params.len(), 1);
        assert!(matches!(result_params[0].data, TyData::Int(IntTy::I32)));

        let result_ret = &dst[result.ret];
        assert!(matches!(result_ret.data, TyData::Int(IntTy::I32)));

        let _ = Name::new(db, "test".to_owned());
    });
}

#[test]
fn instantiate_struct_sig() {
    let db = Database::default();
    db.attach(|db| {
        let mut src = Stash::new();
        let name_first = Name::new(db, "first".to_owned());
        let name_second = Name::new(db, "second".to_owned());

        let bound_vars = src.alloc_slice(&[
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

        let ty0 = src.alloc(bv0);
        let ty1 = src.alloc(bv1);
        let fields = src.alloc_slice(&[
            FieldSig {
                name: name_first,
                ty: ty0,
            },
            FieldSig {
                name: name_second,
                ty: ty1,
            },
        ]);
        let sig = StructSig { fields };
        let binder = Binder::new(sig, bound_vars);

        let bool_ty = Ty { data: TyData::Bool };
        let str_ty = Ty { data: TyData::Str };
        let mut dst = Stash::new();
        let result = sage_ir::ty_fold::instantiate_struct_sig(
            &src,
            &mut dst,
            &binder,
            vec![bool_ty, str_ty],
        );

        let result_fields = &dst[result.fields];
        assert_eq!(result_fields.len(), 2);
        assert_eq!(result_fields[0].name.text(db), "first");
        assert!(matches!(dst[result_fields[0].ty].data, TyData::Bool));
        assert_eq!(result_fields[1].name.text(db), "second");
        assert!(matches!(dst[result_fields[1].ty].data, TyData::Str));
    });
}

#[test]
fn instantiate_nested_type_args() {
    let db = Database::default();
    db.attach(|db| {
        let file = SourceFile::new(
            db,
            "lib.rs".to_owned(),
            "struct HashMap;\nstruct Vec;".to_owned(),
        );
        let items = parse_source_file(db, file);
        let hashmap_sym = Symbol::ast(items[0]);
        let vec_sym = Symbol::ast(items[1]);

        let mut src = Stash::new();
        let bound_vars = src.alloc_slice(&[
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

        // Adt(Vec, [BoundVar(0,1)])
        let bv1_slice = src.alloc_slice(&[bv1]);
        let vec_bv1 = Ty {
            data: TyData::Adt(vec_sym, bv1_slice),
        };

        // Adt(HashMap, [BoundVar(0,0), Adt(Vec, [BoundVar(0,1)])])
        let args = src.alloc_slice(&[bv0, vec_bv1]);
        let hashmap_ty = Ty {
            data: TyData::Adt(hashmap_sym, args),
        };

        let params = src.alloc_slice(&[hashmap_ty]);
        let ret = src.alloc(hashmap_ty);
        let fn_sig = FnSig { params, ret };
        let binder = Binder::new(fn_sig, bound_vars);

        let str_ty = Ty { data: TyData::Str };
        let i32_ty = Ty {
            data: TyData::Int(IntTy::I32),
        };

        let mut dst = Stash::new();
        let result = instantiate_fn_sig(&src, &mut dst, &binder, vec![str_ty, i32_ty]);

        let result_params = &dst[result.params];
        assert_eq!(result_params.len(), 1);
        match result_params[0].data {
            TyData::Adt(sym, args) => {
                assert_eq!(sym, hashmap_sym);
                let args = &dst[args];
                assert_eq!(args.len(), 2);
                assert!(matches!(args[0].data, TyData::Str));
                match args[1].data {
                    TyData::Adt(s, inner_args) => {
                        assert_eq!(s, vec_sym);
                        assert!(matches!(dst[inner_args][0].data, TyData::Int(IntTy::I32)));
                    }
                    _ => panic!("expected Vec<i32>"),
                }
            }
            _ => panic!("expected HashMap"),
        }
    });
}

#[test]
fn de_bruijn_shift() {
    let db = Database::default();
    db.attach(|db| {
        let mut src = Stash::new();

        let bound_vars = src.alloc_slice(&[BoundVarInfo {
            kind: BoundVarKind::Type,
        }]);

        // BoundVar { binder_index: 1, param_index: 0 } — references an outer binder
        let outer_ref = Ty {
            data: TyData::BoundVar(BoundVar {
                binder_index: 1,
                param_index: 0,
            }),
        };
        let params = src.alloc_slice(&[outer_ref]);
        let ret = src.alloc(outer_ref);
        let fn_sig = FnSig { params, ret };
        let binder = Binder::new(fn_sig, bound_vars);

        let i32_ty = Ty {
            data: TyData::Int(IntTy::I32),
        };
        let mut dst = Stash::new();
        let result = instantiate_fn_sig(&src, &mut dst, &binder, vec![i32_ty]);

        let result_params = &dst[result.params];
        match result_params[0].data {
            TyData::BoundVar(bv) => {
                assert_eq!(bv.binder_index, 0);
                assert_eq!(bv.param_index, 0);
            }
            _ => panic!("expected shifted BoundVar"),
        }

        let _ = Name::new(db, "test".to_owned());
    });
}
