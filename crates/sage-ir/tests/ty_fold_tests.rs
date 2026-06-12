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
use sage_ir::ty_fold::*;
use sage_ir::types::Mutability;
use sage_stash::{Stash, StashCopy};
use salsa::Database as _;

#[test]
fn identity_fold() {
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

        let mut stash_a = Stash::new();
        let i32_ty = Ty {
            data: TyData::Int(IntTy::I32),
        };
        let i32_ptr = stash_a.alloc(i32_ty);
        let args = stash_a.alloc_slice(&[i32_ptr]);
        let adt = Ty {
            data: TyData::Adt(sym, args),
        };
        let inner = stash_a.alloc(adt);
        let ref_ty = Ty {
            data: TyData::Ref(inner, Mutability::Shared, Lifetime::Erased),
        };

        let mut stash_b = Stash::new();
        let result = ref_ty.stash_copy(&stash_a, &mut stash_b);

        match result.data {
            TyData::Ref(inner, Mutability::Shared, Lifetime::Erased) => match stash_b[inner].data {
                TyData::Adt(s, a) => {
                    assert_eq!(s, sym);
                    assert_eq!(stash_b[a].len(), 1);
                    assert!(matches!(
                        stash_b[stash_b[a][0]].data,
                        TyData::Int(IntTy::I32)
                    ));
                }
                _ => panic!("expected Adt"),
            },
            _ => panic!("expected Ref"),
        }
    });
}

#[test]
fn instantiate_identity_fn() {
    let db = Database::default();
    db.attach(|db| {
        let file = SourceFile::new(
            db,
            "lib.rs".to_owned(),
            "fn identity<T>(x: T) -> T {}".to_owned(),
        );
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

        let mut src = Stash::new();
        let generics = src.alloc_slice(&[gp]);

        let param_ty = Ty {
            data: TyData::Param(gp),
        };
        let param_ptr = src.alloc(param_ty);
        let params = src.alloc_slice(&[param_ptr]);
        let ret = src.alloc(param_ty);
        let fn_sig = FnSig { params, ret };
        let binder = Binder::new(fn_sig, generics);

        let i32_ty = Ty {
            data: TyData::Int(IntTy::I32),
        };
        let mut dst = Stash::new();
        let result = instantiate_fn_sig(&src, &mut dst, &binder, vec![i32_ty]);

        let result_params = &dst[result.params];
        assert_eq!(result_params.len(), 1);
        assert!(matches!(
            dst[result_params[0]].data,
            TyData::Int(IntTy::I32)
        ));

        let result_ret = &dst[result.ret];
        assert!(matches!(result_ret.data, TyData::Int(IntTy::I32)));
    });
}

#[test]
fn instantiate_struct_sig() {
    let db = Database::default();
    db.attach(|db| {
        let file = SourceFile::new(
            db,
            "lib.rs".to_owned(),
            "struct Pair<A, B> { first: A, second: B }".to_owned(),
        );
        let source_root = SourceRoot::new(db, vec![file]);
        let root_module = ModSymbol::ast(ModAst::crate_root(db, file));
        let scope = ScopeSymbol::Module(root_module, source_root);
        let items = parse_source_file(db, file);
        let struct_sym = Symbol::local(items[0], scope);

        let name_first = Name::new(db, "first".to_owned());
        let name_second = Name::new(db, "second".to_owned());

        let param_a = ExtGenericParam::new(
            db,
            GenericParamKind::Type,
            Some(Name::new(db, "A".to_owned())),
            struct_sym,
            0,
        );
        let param_b = ExtGenericParam::new(
            db,
            GenericParamKind::Type,
            Some(Name::new(db, "B".to_owned())),
            struct_sym,
            1,
        );
        let gp_a = GenericParam::Ext(param_a);
        let gp_b = GenericParam::Ext(param_b);

        let mut src = Stash::new();
        let generics = src.alloc_slice(&[gp_a, gp_b]);

        let ty_a = Ty {
            data: TyData::Param(gp_a),
        };
        let ty_b = Ty {
            data: TyData::Param(gp_b),
        };

        let ty0 = src.alloc(ty_a);
        let ty1 = src.alloc(ty_b);
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
        let binder = Binder::new(sig, generics);

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
        let file2 = SourceFile::new(db, "fn.rs".to_owned(), "fn foo<K, V>() {}".to_owned());
        let source_root = SourceRoot::new(db, vec![file, file2]);
        let root_module = ModSymbol::ast(ModAst::crate_root(db, file));
        let scope = ScopeSymbol::Module(root_module, source_root);
        let items = parse_source_file(db, file);
        let hashmap_sym = Symbol::local(items[0], scope);
        let vec_sym = Symbol::local(items[1], scope);

        let fn_items = parse_source_file(db, file2);
        let fn_sym = Symbol::local(fn_items[0], scope);

        let param_k = ExtGenericParam::new(
            db,
            GenericParamKind::Type,
            Some(Name::new(db, "K".to_owned())),
            fn_sym,
            0,
        );
        let param_v = ExtGenericParam::new(
            db,
            GenericParamKind::Type,
            Some(Name::new(db, "V".to_owned())),
            fn_sym,
            1,
        );
        let gp_k = GenericParam::Ext(param_k);
        let gp_v = GenericParam::Ext(param_v);

        let mut src = Stash::new();
        let generics = src.alloc_slice(&[gp_k, gp_v]);

        let ty_k = Ty {
            data: TyData::Param(gp_k),
        };
        let ty_v = Ty {
            data: TyData::Param(gp_v),
        };

        // Adt(Vec, [Param(V)])
        let ty_v_ptr = src.alloc(ty_v);
        let v_slice = src.alloc_slice(&[ty_v_ptr]);
        let vec_v = Ty {
            data: TyData::Adt(vec_sym, v_slice),
        };

        // Adt(HashMap, [Param(K), Adt(Vec, [Param(V)])])
        let ty_k_ptr = src.alloc(ty_k);
        let vec_v_ptr = src.alloc(vec_v);
        let args = src.alloc_slice(&[ty_k_ptr, vec_v_ptr]);
        let hashmap_ty = Ty {
            data: TyData::Adt(hashmap_sym, args),
        };

        let hashmap_ptr = src.alloc(hashmap_ty);
        let params = src.alloc_slice(&[hashmap_ptr]);
        let ret = src.alloc(hashmap_ty);
        let fn_sig = FnSig { params, ret };
        let binder = Binder::new(fn_sig, generics);

        let str_ty = Ty { data: TyData::Str };
        let i32_ty = Ty {
            data: TyData::Int(IntTy::I32),
        };

        let mut dst = Stash::new();
        let result = instantiate_fn_sig(&src, &mut dst, &binder, vec![str_ty, i32_ty]);

        let result_params = &dst[result.params];
        assert_eq!(result_params.len(), 1);
        match dst[result_params[0]].data {
            TyData::Adt(sym, args) => {
                assert_eq!(sym, hashmap_sym);
                let args = &dst[args];
                assert_eq!(args.len(), 2);
                assert!(matches!(dst[args[0]].data, TyData::Str));
                match dst[args[1]].data {
                    TyData::Adt(s, inner_args) => {
                        assert_eq!(s, vec_sym);
                        assert!(matches!(
                            dst[dst[inner_args][0]].data,
                            TyData::Int(IntTy::I32)
                        ));
                    }
                    _ => panic!("expected Vec<i32>"),
                }
            }
            _ => panic!("expected HashMap"),
        }
    });
}

#[test]
fn param_not_in_subst_passes_through() {
    let db = Database::default();
    db.attach(|db| {
        let file = SourceFile::new(db, "lib.rs".to_owned(), "fn foo<T, U>() {}".to_owned());
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
        let param_u = ExtGenericParam::new(
            db,
            GenericParamKind::Type,
            Some(Name::new(db, "U".to_owned())),
            fn_sym,
            1,
        );
        let gp_t = GenericParam::Ext(param_t);
        let gp_u = GenericParam::Ext(param_u);

        let mut src = Stash::new();
        // Only substitute T, leave U unsubstituted (simulates outer impl param)
        let generics = src.alloc_slice(&[gp_t]);

        let ty_u = Ty {
            data: TyData::Param(gp_u),
        };
        let ty_u_ptr = src.alloc(ty_u);
        let params = src.alloc_slice(&[ty_u_ptr]);
        let ret = src.alloc(ty_u);
        let fn_sig = FnSig { params, ret };
        let binder = Binder::new(fn_sig, generics);

        let i32_ty = Ty {
            data: TyData::Int(IntTy::I32),
        };
        let mut dst = Stash::new();
        let result = instantiate_fn_sig(&src, &mut dst, &binder, vec![i32_ty]);

        // U is not in scope of the substitution, so it passes through unchanged
        let result_params = &dst[result.params];
        match dst[result_params[0]].data {
            TyData::Param(p) => assert_eq!(p, gp_u),
            _ => panic!("expected Param(U) to pass through unchanged"),
        }
    });
}
