#![feature(register_tool)]
#![register_tool(sage)]

use sage_ir::cst::traits::TraitItemCst;
use sage_ir::db::Database;
use sage_ir::local_syms::LocalModItemSym;
use sage_ir::local_syms::mods::{LocalModSym, ModBodySource};
use sage_ir::name::Name;
use sage_ir::scope::{Edition, ScopeSymbol, local_crate};
use sage_ir::source::SourceFile;
use sage_ir::span::{AbsoluteSpan, ParseSource};
use sage_stash::{Stash, Stashed};

#[salsa::tracked]
fn parse_fixture<'db>(db: &'db dyn sage_ir::Db, file: SourceFile) -> Vec<LocalModItemSym<'db>> {
    let mut stash = Stash::new();
    let attrs = stash.alloc_slice::<sage_ir::cst::attrs::AttrCst>(&[]);
    let source = ParseSource::SourceFile(file);
    let root = LocalModSym::new(
        db,
        Name::new(db, String::new()),
        Edition::Rust2021,
        None,
        ModBodySource::File(file),
        Stashed::new(stash, attrs),
        AbsoluteSpan {
            source,
            start: 0,
            end: file.text(db).len() as u32,
        },
    );
    sage_ir::parse::parse_str_to_cst(
        db,
        source,
        file.text(db),
        ScopeSymbol::Crate(local_crate(db, root)),
    )
}

#[test]
#[sage::test(crates.sage_ir.types.spans.item_origin)]
#[sage::test(crates.sage_ir.types.spans.relative_composition)]
fn associated_item_origin_includes_first_attribute() {
    for owner_prefix in ["trait Example", "impl Example"] {
        for member in ["fn member() {}", "type Item = u8;", "const VALUE: u8 = 0;"] {
            for attributes in ["", "#[allow(dead_code)] #[warn(unused)] "] {
                let db = Database::default();
                let item_text = format!("{attributes}{member}");
                let text =
                    format!("// prefix\n#[allow(dead_code)] {owner_prefix} {{ {item_text} }}");
                let file = SourceFile::new(&db, "item-origin.rs".to_owned(), text.clone());
                let items = parse_fixture(&db, file);
                let (stash, associated, owner_span) = match items[0] {
                    LocalModItemSym::Trait(owner) => {
                        let (stash, data) = owner.cst(&db).open_deref();
                        (stash, &stash[data.items], owner.span(&db))
                    }
                    LocalModItemSym::Impl(owner) => {
                        let (stash, data) = owner.cst(&db).open_deref();
                        (stash, &stash[data.items], owner.span(&db))
                    }
                    _ => panic!("expected trait or impl"),
                };
                assert_eq!(owner_span.start, text.find("#[allow").unwrap() as u32);
                assert_eq!(associated.len(), 1);
                let (placement, span, attrs) = match associated[0] {
                    TraitItemCst::Fn { cst, placement } => {
                        (placement, stash[cst].span, stash[cst].attrs)
                    }
                    TraitItemCst::Type { cst, placement } => {
                        (placement, stash[cst].span, stash[cst].attrs)
                    }
                    TraitItemCst::Const { cst, placement } => {
                        (placement, stash[cst].span, stash[cst].attrs)
                    }
                };
                let item_start = text.rfind(&item_text).unwrap() as u32;
                assert_eq!(owner_span.start + placement.start, item_start);
                assert_eq!(
                    owner_span.start + placement.end,
                    item_start + item_text.len() as u32
                );
                assert_eq!(span.start, 0);
                assert_eq!(span.end, item_text.len() as u32);
                let attrs = &stash[attrs];
                if attributes.is_empty() {
                    assert!(attrs.is_empty());
                } else {
                    assert_eq!(attrs.len(), 2);
                    assert_eq!(attrs[0].span.start, 0);
                    assert_eq!(attrs[0].span.end, "#[allow(dead_code)]".len() as u32);
                    assert_eq!(attrs[1].span.start, "#[allow(dead_code)] ".len() as u32);
                    assert_eq!(attrs[1].span.end, attributes.trim_end().len() as u32);
                }
            }
        }
    }
}
