//! Phase 1 behavior tests: Item::MacroDef and Item::MacroInvocation variants.
//!
//! Verifies that `file_item_tree` produces the new macro variants,
//! that `item_name` returns None for them (preserving `definition()` semantics),
//! and that Display impls render them correctly.

use sage_ir::db::Database;
use sage_ir::item::Item;
use sage_ir::lower::file_item_tree;
use sage_ir::resolve::item_name;
use sage_ir::source::SourceFile;
use salsa::Database as _;

fn parse<'db>(db: &'db Database, code: &str) -> Vec<Item<'db>> {
    let file = SourceFile::new(db, "lib.rs".to_owned(), code.to_owned());
    file_item_tree(db, file).clone()
}

// ---------------------------------------------------------------------------
// file_item_tree produces Item::MacroDef for macro_rules!
// ---------------------------------------------------------------------------

#[test]
fn file_item_tree_produces_macro_def() {
    let db = Database::default();
    db.attach(|db| {
        let items = parse(db, "macro_rules! m { () => {} }");

        let macro_def = items
            .iter()
            .find_map(|i| match i {
                Item::MacroDef(d) => Some(*d),
                _ => None,
            })
            .expect("expected Item::MacroDef");

        assert_eq!(macro_def.name(db).text(db).as_str(), "m");
    });
}

// ---------------------------------------------------------------------------
// file_item_tree produces Item::MacroInvocation for m!()
// ---------------------------------------------------------------------------

#[test]
fn file_item_tree_produces_macro_invocation() {
    let db = Database::default();
    db.attach(|db| {
        let items = parse(db, "m!();");

        let inv = items
            .iter()
            .find_map(|i| match i {
                Item::MacroInvocation(v) => Some(*v),
                _ => None,
            })
            .expect("expected Item::MacroInvocation");

        let segments = inv.path(db).segments(db);
        assert_eq!(segments.len(), 1);
        assert_eq!(segments[0].text(db).as_str(), "m");
    });
}

// ---------------------------------------------------------------------------
// Multi-segment macro invocation paths are preserved
// ---------------------------------------------------------------------------

#[test]
fn file_item_tree_multi_segment_macro_path() {
    let db = Database::default();
    db.attach(|db| {
        let items = parse(db, "foo::bar::m!();");

        let inv = items
            .iter()
            .find_map(|i| match i {
                Item::MacroInvocation(v) => Some(*v),
                _ => None,
            })
            .expect("expected Item::MacroInvocation");

        let segments = inv.path(db).segments(db);
        let texts: Vec<&str> = segments.iter().map(|s| s.text(db).as_str()).collect();
        assert_eq!(texts, vec!["foo", "bar", "m"]);
    });
}

// ---------------------------------------------------------------------------
// item_name returns None for MacroDef (macros have their own namespace)
// ---------------------------------------------------------------------------

#[test]
fn item_name_returns_none_for_macro_def() {
    let db = Database::default();
    db.attach(|db| {
        let items = parse(db, "macro_rules! m { () => {} }");
        let macro_def_item = items
            .iter()
            .find(|i| matches!(i, Item::MacroDef(_)))
            .copied()
            .expect("expected a MacroDef item");

        assert_eq!(item_name(db, macro_def_item), None);
    });
}

// ---------------------------------------------------------------------------
// item_name returns None for MacroInvocation (invocations introduce no name)
// ---------------------------------------------------------------------------

#[test]
fn item_name_returns_none_for_macro_invocation() {
    let db = Database::default();
    db.attach(|db| {
        let items = parse(db, "m!();");
        let inv_item = items
            .iter()
            .find(|i| matches!(i, Item::MacroInvocation(_)))
            .copied()
            .expect("expected a MacroInvocation item");

        assert_eq!(item_name(db, inv_item), None);
    });
}

// ---------------------------------------------------------------------------
// Display impl for MacroDef renders macro_rules! syntax
// ---------------------------------------------------------------------------

#[test]
fn display_item_macro_def() {
    let db = Database::default();
    db.attach(|db| {
        let items = parse(db, "macro_rules! m { () => { struct X; } }");
        let macro_def_item = items
            .iter()
            .find(|i| matches!(i, Item::MacroDef(_)))
            .copied()
            .expect("expected a MacroDef item");

        let rendered = format!("{}", macro_def_item);
        assert!(
            rendered.starts_with("macro_rules! m"),
            "expected 'macro_rules! m ...', got: {}",
            rendered
        );
        assert!(
            rendered.contains("struct X"),
            "expected body to contain 'struct X', got: {}",
            rendered
        );
    });
}

// ---------------------------------------------------------------------------
// Display impl for MacroInvocation renders as `path!()`
// ---------------------------------------------------------------------------

#[test]
fn display_item_macro_invocation() {
    let db = Database::default();
    db.attach(|db| {
        let items = parse(db, "foo::m!();");
        let inv_item = items
            .iter()
            .find(|i| matches!(i, Item::MacroInvocation(_)))
            .copied()
            .expect("expected a MacroInvocation item");

        let rendered = format!("{}", inv_item);
        assert!(
            rendered.contains("foo") && rendered.contains("m") && rendered.contains("!()"),
            "expected rendering to contain foo::m!(), got: {}",
            rendered
        );
    });
}
