mod common;

use sage_ir::db::Database;
use sage_ir::item::*;
use sage_ir::lower::parse_source_file;
use sage_ir::sig_ast::*;
use sage_ir::source::SourceFile;
use salsa::Database as _;

#[test]
fn fn_signature_generics_and_params() {
    let db = Database::default();
    db.attach(|db| {
        let file = SourceFile::new(
            db,
            "lib.rs".to_owned(),
            "fn foo<T, U>(x: T, y: &U) -> bool {}".to_owned(),
        );
        let items = parse_source_file(db, file);
        let func = match items[0] {
            ItemAst::Function(f) => f,
            _ => panic!("expected function"),
        };
        let sig = func.signature(db);
        let stash = sig.stash();
        let data = &stash[*sig.root()];

        let generics = &stash[data.generics];
        assert_eq!(generics.len(), 2);
        match &generics[0] {
            GenericParam::Type { name, .. } => assert_eq!(name.text(db), "T"),
            _ => panic!("expected Type param"),
        }
        match &generics[1] {
            GenericParam::Type { name, .. } => assert_eq!(name.text(db), "U"),
            _ => panic!("expected Type param"),
        }

        let params = &stash[data.params];
        assert_eq!(params.len(), 2);
        assert_eq!(params[0].name.unwrap().text(db), "x");
        assert_eq!(params[1].name.unwrap().text(db), "y");

        let ret = data.ret_type.unwrap();
        let ret_data = &stash[ret];
        match ret_data.kind {
            TypeRefAstKind::Path(p) => {
                let path = &stash[p];
                let segs = &stash[path.segments];
                assert_eq!(segs.len(), 1);
                assert_eq!(segs[0].name.text(db), "bool");
            }
            _ => panic!("expected path return type"),
        }
    });
}

#[test]
fn struct_signature_generics_and_fields() {
    let db = Database::default();
    db.attach(|db| {
        let file = SourceFile::new(
            db,
            "lib.rs".to_owned(),
            "struct Pair<A, B> { first: A, second: B }".to_owned(),
        );
        let items = parse_source_file(db, file);
        let s = match items[0] {
            ItemAst::Struct(s) => s,
            _ => panic!("expected struct"),
        };
        let sig = s.signature(db);
        let stash = sig.stash();
        let data = &stash[*sig.root()];

        let generics = &stash[data.generics];
        assert_eq!(generics.len(), 2);

        let fields = &stash[data.fields];
        assert_eq!(fields.len(), 2);
        assert_eq!(fields[0].name.text(db), "first");
        assert_eq!(fields[1].name.text(db), "second");
    });
}

#[test]
fn path_with_type_args() {
    let db = Database::default();
    db.attach(|db| {
        let file = SourceFile::new(
            db,
            "lib.rs".to_owned(),
            "fn bar(m: HashMap<String, Vec<u8>>) {}".to_owned(),
        );
        let items = parse_source_file(db, file);
        let func = match items[0] {
            ItemAst::Function(f) => f,
            _ => panic!("expected function"),
        };
        let sig = func.signature(db);
        let stash = sig.stash();
        let data = &stash[*sig.root()];

        let params = &stash[data.params];
        assert_eq!(params.len(), 1);

        let param_ty = &stash[params[0].ty];
        match param_ty.kind {
            TypeRefAstKind::Path(p) => {
                let path = &stash[p];
                let segs = &stash[path.segments];
                assert_eq!(segs.len(), 1);
                assert_eq!(segs[0].name.text(db), "HashMap");
                let generic_args = &stash[segs[0].generic_args];
                assert_eq!(generic_args.len(), 2);
                match generic_args[1] {
                    GenericArgAst::Type(ty_ref) => match ty_ref.kind {
                        TypeRefAstKind::Path(p2) => {
                            let path2 = &stash[p2];
                            let segs2 = &stash[path2.segments];
                            assert_eq!(segs2[0].name.text(db), "Vec");
                            let inner_args = &stash[segs2[0].generic_args];
                            assert_eq!(inner_args.len(), 1);
                        }
                        _ => panic!("expected path for Vec<u8>"),
                    },
                    _ => panic!("expected Type arg"),
                }
            }
            _ => panic!("expected path type"),
        }
    });
}

#[test]
fn enum_signature_variants() {
    let db = Database::default();
    db.attach(|db| {
        let file = SourceFile::new(
            db,
            "lib.rs".to_owned(),
            "enum Color<T> { Red, Green { value: T }, Blue }".to_owned(),
        );
        let items = parse_source_file(db, file);
        let e = match items[0] {
            ItemAst::Enum(e) => e,
            _ => panic!("expected enum"),
        };
        let sig = e.signature(db);
        let stash = sig.stash();
        let data = &stash[*sig.root()];

        let generics = &stash[data.generics];
        assert_eq!(generics.len(), 1);

        let variants = &stash[data.variants];
        assert_eq!(variants.len(), 3);
        assert_eq!(variants[0].name.text(db), "Red");
        assert_eq!(variants[1].name.text(db), "Green");
        assert_eq!(variants[2].name.text(db), "Blue");

        let green_fields = &stash[variants[1].fields];
        assert_eq!(green_fields.len(), 1);
        assert_eq!(green_fields[0].name.text(db), "value");
    });
}
