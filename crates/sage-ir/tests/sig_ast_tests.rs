use sage_ir::name::Name;
use sage_ir::sig_ast::*;
use sage_ir::span::RelativeSpan;
use sage_ir::types::Mutability;
use sage_stash::{Ptr, Stash, Stashed};
use salsa::Database as _;

fn span(start: u32, end: u32) -> RelativeSpan {
    RelativeSpan { start, end }
}

fn db() -> sage_ir::db::Database {
    sage_ir::db::Database::default()
}

fn make_simple_path<'db>(
    stash: &mut Stash,
    name: Name<'db>,
    sp: RelativeSpan,
) -> Ptr<PathAst<'db>> {
    let empty_args = stash.alloc_slice::<TypeRefAst<'db>>(&[]);
    let seg = PathSegmentAst {
        name,
        type_args: empty_args,
        span: sp,
    };
    let segs = stash.alloc_slice(&[seg]);
    stash.alloc(PathAst {
        segments: segs,
        span: sp,
    })
}

fn make_path_type<'db>(
    stash: &mut Stash,
    name: Name<'db>,
    sp: RelativeSpan,
) -> Ptr<TypeRefAst<'db>> {
    let path = make_simple_path(stash, name, sp);
    stash.alloc(TypeRefAst {
        kind: TypeRefAstKind::Path(path),
        span: sp,
    })
}

#[test]
fn fn_sig_ast_round_trip() {
    let db = db();
    db.attach(|db| {
        let mut stash = Stash::new();

        let t_name = Name::new(db, "T".to_owned());
        let u_name = Name::new(db, "U".to_owned());

        let generics = stash.alloc_slice(&[
            GenericParam::Type {
                name: t_name,
                span: span(0, 1),
            },
            GenericParam::Type {
                name: u_name,
                span: span(3, 4),
            },
        ]);

        let bool_ty = make_path_type(&mut stash, Name::new(db, "bool".to_owned()), span(20, 24));
        let t_ty = make_path_type(&mut stash, t_name, span(10, 11));

        let params = stash.alloc_slice(&[ParamAst {
            name: Some(Name::new(db, "x".to_owned())),
            ty: t_ty,
            span: span(5, 12),
        }]);

        let root = stash.alloc(FnSigAstData {
            generics,
            params,
            ret_type: Some(bool_ty),
        });
        let stashed = Stashed::new(stash, root);
        let s = stashed.stash();
        let data = &s[*stashed.root()];

        assert_eq!(s[data.generics].len(), 2);
        assert_eq!(s[data.params].len(), 1);
        assert!(data.ret_type.is_some());

        let param = &s[data.params][0];
        assert_eq!(param.name.unwrap().text(db), "x");

        let ret = &s[data.ret_type.unwrap()];
        match ret.kind {
            TypeRefAstKind::Path(path_ptr) => {
                let path = &s[path_ptr];
                assert_eq!(s[path.segments].len(), 1);
                assert_eq!(s[path.segments][0].name.text(db), "bool");
            }
            _ => panic!("expected Path"),
        }
    });
}

#[test]
fn path_segment_with_type_args() {
    let db = db();
    db.attach(|db| {
        let mut stash = Stash::new();

        // Build type args K and V
        let k_ty = TypeRefAst {
            kind: TypeRefAstKind::Path(make_simple_path(
                &mut stash,
                Name::new(db, "K".to_owned()),
                span(8, 9),
            )),
            span: span(8, 9),
        };
        let v_ty = TypeRefAst {
            kind: TypeRefAstKind::Path(make_simple_path(
                &mut stash,
                Name::new(db, "V".to_owned()),
                span(11, 12),
            )),
            span: span(11, 12),
        };

        // HashMap<K, V>
        let type_args = stash.alloc_slice(&[k_ty, v_ty]);
        let hashmap_seg = PathSegmentAst {
            name: Name::new(db, "HashMap".to_owned()),
            type_args,
            span: span(0, 13),
        };

        let seg = stash.alloc(hashmap_seg);
        let stored = &stash[seg];
        let args = &stash[stored.type_args];
        assert_eq!(args.len(), 2);
        assert_eq!(stored.name.text(db), "HashMap");
    });
}

#[test]
fn stashed_equality() {
    let db = db();
    db.attach(|db| {
        let build = |ret_name: &str| {
            let mut stash = Stash::new();
            let generics = stash.alloc_slice::<GenericParam<'_>>(&[]);
            let params = stash.alloc_slice::<ParamAst<'_>>(&[]);
            let ret = make_path_type(
                &mut stash,
                Name::new(db, ret_name.to_owned()),
                span(0, ret_name.len() as u32),
            );
            let root = stash.alloc(FnSigAstData {
                generics,
                params,
                ret_type: Some(ret),
            });
            Stashed::new(stash, root)
        };

        let a = build("bool");
        let b = build("bool");
        let c = build("i32");

        assert_eq!(a, b);
        assert_ne!(a, c);
    });
}

#[test]
fn struct_sig_ast_round_trip() {
    let db = db();
    db.attach(|db| {
        let mut stash = Stash::new();

        let a_name = Name::new(db, "A".to_owned());
        let generics = stash.alloc_slice(&[GenericParam::Type {
            name: a_name,
            span: span(0, 1),
        }]);

        let a_ty = make_path_type(&mut stash, a_name, span(10, 11));
        let fields = stash.alloc_slice(&[FieldDefAst {
            name: Name::new(db, "value".to_owned()),
            ty: a_ty,
            span: span(5, 15),
        }]);

        let root = stash.alloc(StructSigAstData { generics, fields });
        let stashed: StructSigAst<'_> = Stashed::new(stash, root);
        let s = stashed.stash();
        let data = &s[*stashed.root()];

        assert_eq!(s[data.generics].len(), 1);
        assert_eq!(s[data.fields].len(), 1);
        assert_eq!(s[data.fields][0].name.text(db), "value");
    });
}

#[test]
fn type_ref_ast_variants() {
    let db = db();
    db.attach(|db| {
        let mut stash = Stash::new();

        // Reference type: &mut T
        let inner = make_path_type(&mut stash, Name::new(db, "T".to_owned()), span(0, 1));
        let ref_ty = stash.alloc(TypeRefAst {
            kind: TypeRefAstKind::Reference(inner, Mutability::Mut),
            span: span(0, 6),
        });

        let stored = &stash[ref_ty];
        match stored.kind {
            TypeRefAstKind::Reference(inner_ptr, Mutability::Mut) => {
                assert!(matches!(stash[inner_ptr].kind, TypeRefAstKind::Path(_)));
            }
            _ => panic!("expected Reference"),
        }

        // Tuple type: (A, B)
        let a_ty = TypeRefAst {
            kind: TypeRefAstKind::Path(make_simple_path(
                &mut stash,
                Name::new(db, "A".to_owned()),
                span(1, 2),
            )),
            span: span(1, 2),
        };
        let b_ty = TypeRefAst {
            kind: TypeRefAstKind::Path(make_simple_path(
                &mut stash,
                Name::new(db, "B".to_owned()),
                span(4, 5),
            )),
            span: span(4, 5),
        };
        let elems = stash.alloc_slice(&[a_ty, b_ty]);
        let tuple_ty = stash.alloc(TypeRefAst {
            kind: TypeRefAstKind::Tuple(elems),
            span: span(0, 6),
        });
        match stash[tuple_ty].kind {
            TypeRefAstKind::Tuple(elems) => assert_eq!(stash[elems].len(), 2),
            _ => panic!("expected Tuple"),
        }

        // Never type
        let never = stash.alloc(TypeRefAst {
            kind: TypeRefAstKind::Never,
            span: span(0, 1),
        });
        assert!(matches!(stash[never].kind, TypeRefAstKind::Never));

        // Slice type: [T]
        let slice_ty = stash.alloc(TypeRefAst {
            kind: TypeRefAstKind::Slice(inner),
            span: span(0, 3),
        });
        assert!(matches!(stash[slice_ty].kind, TypeRefAstKind::Slice(_)));
    });
}
