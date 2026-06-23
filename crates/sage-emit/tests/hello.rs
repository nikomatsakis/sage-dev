use rust_ref::*;
use sage_emit::emit_module;
use sage_ir::Db;
use sage_ir::db::Database;
use sage_ir::local_syms::mods::{LocalModSym, ModBodySource};
use sage_ir::name::Name;
use sage_ir::parse::parse_str_to_cst;
use sage_ir::scope::{ScopeSymbol, local_crate};
use sage_ir::source::SourceFile;
use sage_ir::span::{AbsoluteSpan, ParseSource};
use sage_ir::symbol::ModSymbol;
use sage_stash::{Stash, Stashed};
use salsa::Database as _;

fn analyze_source(source: &str) -> Crate<NormalizedDef> {
    let db = Database::default();
    db.attach(|db| {
        let file = SourceFile::new(db, "lib.rs".to_owned(), source.to_owned());

        let mut empty_stash = Stash::new();
        let empty_slice = empty_stash.alloc_slice::<sage_ir::cst::attrs::AttrCst>(&[]);
        let empty_attrs = Stashed::new(empty_stash, empty_slice);
        let abs_span = AbsoluteSpan {
            source: ParseSource::SourceFile(file),
            start: 0,
            end: source.len() as u32,
        };

        let root_mod = LocalModSym::new(
            db,
            Name::new(db, String::new()),
            None,
            ModBodySource::File(file),
            empty_attrs,
            abs_span,
        );

        let krate = local_crate(db, root_mod);
        let scope = ScopeSymbol::Crate(krate);

        let parsed_source = ParseSource::SourceFile(file);
        let items = parse_str_to_cst(db, parsed_source, file.text(db), scope);
        sage_ir::local_syms::mods::unexpanded_items::specify(db, root_mod, items);

        let root = ModSymbol::Local(root_mod);
        emit_module(db, root)
    })
}

#[test]
fn hello_rs_signatures() {
    let source = r#"
fn identity(x: u32) -> u32 {
    x
}

fn add(a: u32, b: u32) -> u32 {
    a + b
}

struct Point {
    x: u32,
    y: u32,
}

fn origin() -> Point {
    Point { x: 0, y: 0 }
}

fn get_x(p: Point) -> u32 {
    p.x
}
"#;

    let krate = analyze_source(source);

    assert_eq!(krate.root.name, "");

    let items = &krate.root.items;
    assert_eq!(items.len(), 5, "expected 5 items, got {:?}", items);

    // fn identity(x: u32) -> u32
    let Item::Fn(identity) = &items[0] else {
        panic!("expected fn, got {:?}", items[0]);
    };
    assert_eq!(identity.name, "identity");
    assert_eq!(identity.params.len(), 1);
    assert_eq!(identity.params[0].name, "x");
    assert_eq!(identity.params[0].ty, Type::Primitive("u32".to_string()));
    assert_eq!(identity.return_ty, Type::Primitive("u32".to_string()));

    // fn add(a: u32, b: u32) -> u32
    let Item::Fn(add) = &items[1] else {
        panic!("expected fn, got {:?}", items[1]);
    };
    assert_eq!(add.name, "add");
    assert_eq!(add.params.len(), 2);
    assert_eq!(add.params[0].name, "a");
    assert_eq!(add.params[1].name, "b");
    assert_eq!(add.return_ty, Type::Primitive("u32".to_string()));

    // struct Point { x: u32, y: u32 }
    let Item::Struct(point) = &items[2] else {
        panic!("expected struct, got {:?}", items[2]);
    };
    assert_eq!(point.name, "Point");
    assert_eq!(point.fields.len(), 2);
    assert_eq!(point.fields[0].name, "x");
    assert_eq!(point.fields[0].ty, Type::Primitive("u32".to_string()));
    assert_eq!(point.fields[1].name, "y");
    assert_eq!(point.fields[1].ty, Type::Primitive("u32".to_string()));

    // fn origin() -> Point
    let Item::Fn(origin) = &items[3] else {
        panic!("expected fn, got {:?}", items[3]);
    };
    assert_eq!(origin.name, "origin");
    assert_eq!(origin.params.len(), 0);
    match &origin.return_ty {
        Type::Def { target, .. } => match target {
            NormalizedDef::Local(_) => {}
            other => panic!("expected Local def for Point, got {:?}", other),
        },
        other => panic!("expected Def type for origin return, got {:?}", other),
    }

    // fn get_x(p: Point) -> u32
    let Item::Fn(get_x) = &items[4] else {
        panic!("expected fn, got {:?}", items[4]);
    };
    assert_eq!(get_x.name, "get_x");
    assert_eq!(get_x.params.len(), 1);
    assert_eq!(get_x.params[0].name, "p");
    assert_eq!(get_x.return_ty, Type::Primitive("u32".to_string()));
}

#[test]
fn hello_rs_bodies() {
    let source = r#"
fn identity(x: u32) -> u32 {
    x
}

fn add(a: u32, b: u32) -> u32 {
    a + b
}

struct Point {
    x: u32,
    y: u32,
}

fn origin() -> Point {
    Point { x: 0, y: 0 }
}

fn get_x(p: Point) -> u32 {
    p.x
}
"#;

    let krate = analyze_source(source);
    let items = &krate.root.items;

    // fn identity(x: u32) -> u32 { x }
    let Item::Fn(identity) = &items[0] else {
        panic!("expected fn");
    };
    let body = identity.body.as_ref().unwrap();
    match body {
        Expr::Block {
            tail: Some(tail), ..
        } => match tail.as_ref() {
            Expr::Local { name, index } => {
                assert_eq!(name, "x");
                assert_eq!(*index, 0);
            }
            other => panic!("expected Local in identity body, got {:?}", other),
        },
        other => panic!("expected Block for identity body, got {:?}", other),
    }

    // fn add(a: u32, b: u32) -> u32 { a + b }
    let Item::Fn(add) = &items[1] else {
        panic!("expected fn");
    };
    let body = add.body.as_ref().unwrap();
    match body {
        Expr::Block {
            tail: Some(tail), ..
        } => match tail.as_ref() {
            Expr::BinaryOp { op, lhs, rhs, ty } => {
                assert_eq!(*op, BinOp::Add);
                assert_eq!(*ty, Type::Primitive("u32".to_string()));
                match lhs.as_ref() {
                    Expr::Local { name, index } => {
                        assert_eq!(name, "a");
                        assert_eq!(*index, 0);
                    }
                    other => panic!("expected Local for lhs, got {:?}", other),
                }
                match rhs.as_ref() {
                    Expr::Local { name, index } => {
                        assert_eq!(name, "b");
                        assert_eq!(*index, 1);
                    }
                    other => panic!("expected Local for rhs, got {:?}", other),
                }
            }
            other => panic!("expected BinaryOp in add body, got {:?}", other),
        },
        other => panic!("expected Block for add body, got {:?}", other),
    }

    // fn origin() -> Point { Point { x: 0, y: 0 } }
    let Item::Fn(origin) = &items[3] else {
        panic!("expected fn");
    };
    let body = origin.body.as_ref().unwrap();
    match body {
        Expr::Block {
            tail: Some(tail), ..
        } => match tail.as_ref() {
            Expr::StructLit { fields, .. } => {
                assert_eq!(fields.len(), 2);
                assert_eq!(fields[0].name, "x");
                assert_eq!(fields[1].name, "y");
            }
            other => panic!("expected StructLit in origin body, got {:?}", other),
        },
        other => panic!("expected Block for origin body, got {:?}", other),
    }

    // fn get_x(p: Point) -> u32 { p.x }
    let Item::Fn(get_x) = &items[4] else {
        panic!("expected fn");
    };
    let body = get_x.body.as_ref().unwrap();
    match body {
        Expr::Block {
            tail: Some(tail), ..
        } => match tail.as_ref() {
            Expr::Field { field_name, ty, .. } => {
                assert_eq!(field_name, "x");
                assert_eq!(*ty, Type::Primitive("u32".to_string()));
            }
            other => panic!("expected Field in get_x body, got {:?}", other),
        },
        other => panic!("expected Block for get_x body, got {:?}", other),
    }
}
