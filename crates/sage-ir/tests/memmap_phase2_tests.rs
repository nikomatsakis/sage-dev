//! Phase 2 MEM-map tests: macro expansion in the fixpoint.
//!
//! Tests cover: macro_rules! lowering, expand_macro, resolve_memmap_path,
//! fixpoint convergence, depth limit.

use expect_test::expect;
use sage_ir::db::Database;
use sage_ir::item::ModAst;
use sage_ir::memmap::{MemmapEntry, expand_macro, module_memmap};
use sage_ir::module::ModSymbol;
use sage_ir::name::Name;
use sage_ir::resolve::{Namespace, SourceRoot, resolve_name};
use sage_ir::source::SourceFile;
use sage_ir::span::ParseSource;
use sage_ir::symbol::SymbolData;

use salsa::Database as _;

mod common;
use common::fmt_symbol;

/// Helper: create a multi-file crate.
fn setup_files<'db>(db: &'db Database, files: &[(&str, &str)]) -> (SourceRoot, ModSymbol<'db>) {
    let source_files: Vec<_> = files
        .iter()
        .map(|(path, text)| SourceFile::new(db, path.to_string(), text.to_string()))
        .collect();
    let source_root = SourceRoot::new(db, source_files.clone());
    let lib_file = source_files
        .iter()
        .find(|f| f.path(db) == "lib.rs")
        .copied()
        .expect("must have lib.rs");
    let root_module = ModSymbol::ast(ModAst::crate_root(db, lib_file));
    (source_root, root_module)
}

fn setup_single<'db>(db: &'db Database, code: &str) -> (SourceRoot, ModSymbol<'db>) {
    let file = SourceFile::new(db, "lib.rs".to_owned(), code.to_owned());
    let source_root = SourceRoot::new(db, vec![file]);
    let root_module = ModSymbol::ast(ModAst::crate_root(db, file));
    (source_root, root_module)
}

// ---------------------------------------------------------------------------
// M3: Macro expands to a struct (basic expansion)
// ---------------------------------------------------------------------------

#[test]
fn m3_macro_expands_to_struct() {
    let db = Database::default();
    db.attach(|db| {
        let (source_root, root_module) = setup_single(
            db,
            r#"
macro_rules! m { () => { struct Foo; } }
m!();
"#,
        );

        let name = Name::new(db, "Foo".to_owned());
        let result = resolve_name(db, root_module, source_root, name, Namespace::Type);
        assert!(
            result.is_ok(),
            "Foo should resolve from macro expansion, got {:?}",
            result
        );
        match result.unwrap().data() {
            SymbolData::Ast(_) => {} // good
            _ => panic!("expected local symbol"),
        }
    });
}

// ---------------------------------------------------------------------------
// M5: Macro expansion produces only impl (anonymous)
// ---------------------------------------------------------------------------

#[test]
fn m5_macro_expands_to_impl_only() {
    let db = Database::default();
    db.attach(|db| {
        let (source_root, root_module) = setup_single(
            db,
            r#"
struct Foo;
macro_rules! m { () => { impl Foo { fn bar() {} } } }
m!();
"#,
        );

        // m!() expands to an impl — no new names introduced
        let memmap = module_memmap(db, root_module, source_root);
        let macro_uses: Vec<_> = memmap
            .entries(db)
            .iter()
            .filter_map(|e| match e {
                MemmapEntry::MacroUse(mu) => Some(mu),
                _ => None,
            })
            .collect();
        assert_eq!(macro_uses.len(), 1);
        let exps = &macro_uses[0].expansions;
        assert!(!exps.is_empty(), "expected expansions, got empty");
        // One branch, one Item(Impl-placeholder) entry.
        assert_eq!(exps.len(), 1);
        let entries = &exps[0].entries;
        assert_eq!(entries.len(), 1);
        // Phase 1: impls are synthesized as Item(Error) placeholders;
        // Phase 4 will route through parse_source_file for real impls.
        assert!(
            matches!(
                entries[0],
                MemmapEntry::Item(sage_ir::item::ItemAst::Error(..))
                    | MemmapEntry::Item(sage_ir::item::ItemAst::Impl(_))
            ),
            "expected anonymous impl entry, got {:?}",
            entries[0]
        );
    });
}

// ---------------------------------------------------------------------------
// M6: Empty macro expansion
// ---------------------------------------------------------------------------

#[test]
fn m6_empty_macro_expansion() {
    let db = Database::default();
    db.attach(|db| {
        let (source_root, root_module) = setup_single(
            db,
            r#"
macro_rules! m { () => {} }
m!();
"#,
        );

        let memmap = module_memmap(db, root_module, source_root);
        let macro_uses: Vec<_> = memmap
            .entries(db)
            .iter()
            .filter_map(|e| match e {
                MemmapEntry::MacroUse(mu) => Some(mu),
                _ => None,
            })
            .collect();
        assert_eq!(macro_uses.len(), 1);
        let exps = &macro_uses[0].expansions;
        assert!(!exps.is_empty(), "expected expansions, got empty");
        assert_eq!(exps.len(), 1);
        assert!(
            exps[0].entries.is_empty(),
            "empty macro should expand to nothing"
        );
    });
}

// ---------------------------------------------------------------------------
// R1: self:: path for macro resolution
// ---------------------------------------------------------------------------

#[test]
fn r1_self_path_macro_resolution() {
    let db = Database::default();
    db.attach(|db| {
        let (source_root, root_module) = setup_single(
            db,
            r#"
macro_rules! m { () => { struct Foo; } }
self::m!();
"#,
        );

        let name = Name::new(db, "Foo".to_owned());
        let result = resolve_name(db, root_module, source_root, name, Namespace::Type);
        assert!(
            result.is_ok(),
            "Foo should resolve via self::m!(), got {:?}",
            result
        );
    });
}

// ---------------------------------------------------------------------------
// R6: Multi-segment path through nested modules
// ---------------------------------------------------------------------------

#[test]
fn r6_multi_segment_path_macro() {
    let db = Database::default();
    db.attach(|db| {
        let (source_root, root_module) = setup_files(
            db,
            &[
                (
                    "lib.rs",
                    r#"
mod a;
a::b::m!();
"#,
                ),
                (
                    "a.rs",
                    r#"
pub mod b;
"#,
                ),
                (
                    "a/b.rs",
                    r#"
macro_rules! m { () => { struct Deep; } }
pub(crate) use m;
"#,
                ),
            ],
        );

        let name = Name::new(db, "Deep".to_owned());
        let result = resolve_name(db, root_module, source_root, name, Namespace::Type);
        assert!(
            result.is_ok(),
            "Deep should resolve via a::b::m!(), got {:?}",
            result
        );
    });
}

// ---------------------------------------------------------------------------
// Provenance: expanded items carry ParseSource::MacroExpansion
// ---------------------------------------------------------------------------

#[test]
fn expanded_items_carry_macro_expansion_provenance() {
    let db = Database::default();
    db.attach(|db| {
        let (source_root, root_module) = setup_single(
            db,
            r#"
macro_rules! m { () => { struct FromMacro; } }
m!();
"#,
        );

        let memmap = module_memmap(db, root_module, source_root);
        let macro_uses: Vec<_> = memmap
            .entries(db)
            .iter()
            .filter_map(|e| match e {
                MemmapEntry::MacroUse(mu) => Some(mu),
                _ => None,
            })
            .collect();
        assert_eq!(macro_uses.len(), 1);
        let exps = &macro_uses[0].expansions;
        assert_eq!(exps.len(), 1);

        for entry in &exps[0].entries {
            if let MemmapEntry::Item(item) = entry {
                let span = item.absolute_span(db);
                assert!(
                    matches!(span.source, ParseSource::MacroExpansion(_)),
                    "items from macro expansion should have MacroExpansion provenance, got {:?}",
                    span.source
                );
            }
        }
    });
}

// ---------------------------------------------------------------------------
// Provenance: real file items carry ParseSource::SourceFile
// ---------------------------------------------------------------------------

#[test]
fn real_file_items_carry_source_file_provenance() {
    let db = Database::default();
    db.attach(|db| {
        let (source_root, root_module) = setup_single(db, "struct Real;");

        let memmap = module_memmap(db, root_module, source_root);
        for entry in memmap.entries(db) {
            if let MemmapEntry::Item(item) = entry {
                let span = item.absolute_span(db);
                assert!(
                    matches!(span.source, ParseSource::SourceFile(_)),
                    "items from real files should have SourceFile provenance, got {:?}",
                    span.source
                );
            }
        }
    });
}

// ---------------------------------------------------------------------------
// Memoization: same macro + same callee = same MacroExpansion identity
// ---------------------------------------------------------------------------

#[test]
fn expand_macro_memoization() {
    let db = Database::default();
    db.attach(|db| {
        let (source_root, root_module) = setup_single(
            db,
            r#"
macro_rules! m { () => { struct Foo; } }
m!();
m!();
"#,
        );

        let memmap = module_memmap(db, root_module, source_root);
        let macro_uses: Vec<_> = memmap
            .entries(db)
            .iter()
            .filter_map(|e| match e {
                MemmapEntry::MacroUse(mu) => Some(mu),
                _ => None,
            })
            .collect();
        assert_eq!(macro_uses.len(), 2, "should have two macro invocations");

        // Both should be expanded
        assert!(!macro_uses[0].expansions.is_empty());
        assert!(!macro_uses[1].expansions.is_empty());

        // Both expansions should use the same callee
        let callee0 = macro_uses[0].expansions[0].callee;
        let callee1 = macro_uses[1].expansions[0].callee;
        assert_eq!(callee0, callee1);

        // Call expand_macro directly with same callee+input — should return
        // the same MacroExpansion identity (memoized by salsa)
        let exp0 = expand_macro(db, callee0, macro_uses[0].input);
        let exp0_again = expand_macro(db, callee0, macro_uses[0].input);
        assert_eq!(
            exp0, exp0_again,
            "same callee+input should give same MacroExpansion"
        );
    });
}

// ---------------------------------------------------------------------------
// ParseSource::text() works for both variants
// ---------------------------------------------------------------------------

#[test]
fn parse_source_text_both_variants() {
    let db = Database::default();
    db.attach(|db| {
        let file = SourceFile::new(db, "test.rs".to_owned(), "struct A;".to_owned());
        let ps = ParseSource::SourceFile(file);
        assert_eq!(ps.text(db), "struct A;");

        let (source_root, root_module) = setup_single(
            db,
            r#"
macro_rules! m { () => { struct B; } }
m!();
"#,
        );

        let memmap = module_memmap(db, root_module, source_root);
        let mu = memmap
            .entries(db)
            .iter()
            .find_map(|e| match e {
                MemmapEntry::MacroUse(mu) if !mu.expansions.is_empty() => Some(mu),
                _ => None,
            })
            .expect("should have an expanded macro use");

        let callee = mu.expansions[0].callee;
        let expansion = expand_macro(db, callee, mu.input);
        let ps = ParseSource::MacroExpansion(expansion);
        assert!(
            !ps.text(db).is_empty(),
            "macro expansion text should not be empty"
        );
    });
}

// ---------------------------------------------------------------------------
// Nested macros: macro that expands to code containing another macro
// ---------------------------------------------------------------------------

#[test]
fn nested_macro_expansion() {
    let db = Database::default();
    db.attach(|db| {
        let (source_root, root_module) = setup_single(
            db,
            r#"
macro_rules! inner { () => { struct Nested; } }
macro_rules! outer { () => { inner!(); } }
outer!();
"#,
        );

        let name = Name::new(db, "Nested".to_owned());
        let result = resolve_name(db, root_module, source_root, name, Namespace::Type);
        assert!(
            result.is_ok(),
            "Nested should resolve from nested macro expansion, got {:?}",
            result
        );
    });
}

// ---------------------------------------------------------------------------
// Fixpoint: macro expansion makes another macro's path resolvable
// ---------------------------------------------------------------------------

#[test]
fn fixpoint_expansion_enables_sibling_resolution() {
    let db = Database::default();
    db.attach(|db| {
        // `define_m!()` expands to a macro_rules definition for `m`.
        // `m!()` can only resolve after `define_m!()` has expanded.
        // The old single-pass approach would fail this because the
        // snapshot is taken before any expansion.
        let (source_root, root_module) = setup_single(
            db,
            r#"
macro_rules! define_m { () => { macro_rules! m { () => { struct Created; } } } }
define_m!();
m!();
"#,
        );

        let name = Name::new(db, "Created".to_owned());
        let result = resolve_name(db, root_module, source_root, name, Namespace::Type);
        let rendered = match &result {
            Ok(sym) => fmt_symbol(db, *sym),
            Err(e) => format!("{e:?}"),
        };
        expect![[r#"<local Struct Created>"#]].assert_eq(&rendered);
    });
}

// ---------------------------------------------------------------------------
// E3: Depth limit exceeded (recursive macro)
// ---------------------------------------------------------------------------

#[test]
fn e3_depth_limit_exceeded() {
    let db = Database::default();
    db.attach(|db| {
        let (source_root, root_module) = setup_single(
            db,
            r#"
macro_rules! m { () => { m!(); } }
m!();
"#,
        );

        let memmap = module_memmap(db, root_module, source_root);
        // The recursive expansion should hit the depth limit somewhere
        // in the tree. Phase 1 collapsed the old `Error` variant into
        // `Unresolved` — a MacroUse that remains Unresolved after
        // expansion has finished is the signal we're looking for.
        fn has_unresolved_inside_expansion(entries: &[MemmapEntry]) -> bool {
            for entry in entries {
                if let MemmapEntry::MacroUse(mu) = entry {
                    if mu.expansions.is_empty() {
                        return true;
                    }
                    for exp in &mu.expansions {
                        if has_unresolved_inside_expansion(&exp.entries) {
                            return true;
                        }
                    }
                }
            }
            false
        }
        assert!(
            has_unresolved_inside_expansion(memmap.entries(db)),
            "recursive macro should hit depth limit and leave some MacroUse Unresolved"
        );
    });
}

// ---------------------------------------------------------------------------
// Three levels of "expansion enables resolution"
// ---------------------------------------------------------------------------

#[test]
fn three_level_expansion_chain() {
    let db = Database::default();
    db.attach(|db| {
        // define_b!() → macro_rules! define_c, define_c!() → macro_rules! c,
        // c!() → struct ThreeDeep. Each level must expand before the next resolves.
        let (source_root, root_module) = setup_single(
            db,
            r#"
macro_rules! define_b { () => { macro_rules! define_c { () => { macro_rules! c { () => { struct ThreeDeep; } } } } } }
define_b!();
define_c!();
c!();
"#,
        );

        let name = Name::new(db, "ThreeDeep".to_owned());
        let result = resolve_name(db, root_module, source_root, name, Namespace::Type);
        let rendered = match &result {
            Ok(sym) => fmt_symbol(db, *sym),
            Err(e) => format!("{e:?}"),
        };
        expect![[r#"<local Struct ThreeDeep>"#]].assert_eq(&rendered);
    });
}

// ---------------------------------------------------------------------------
// Macro expansion introduces a glob that makes another macro resolvable
//
// Known limitation: glob entries inside expansion output are not visible
// to resolve_macro_path during the fixpoint loop. The glob from
// setup_glob!() lives inside MacroUse.expansions, not at the top level
// of the memmap entries. Fixing this requires resolve_macro_path to
// walk expansion entries for globs/redirects.
// ---------------------------------------------------------------------------

#[test]
fn expansion_introduces_glob_enabling_resolution() {
    let db = Database::default();
    db.attach(|db| {
        // setup_glob!() expands to `use inner::*`. `inner` contains macro `m`.
        // `m!()` can only resolve after `setup_glob!()` brings `m` into scope.
        // Currently unresolved — see known limitation above.
        let (source_root, root_module) = setup_files(
            db,
            &[
                (
                    "lib.rs",
                    r#"
mod inner;
macro_rules! setup_glob { () => { use inner::*; } }
setup_glob!();
m!();
"#,
                ),
                (
                    "inner.rs",
                    r#"
macro_rules! m { () => { struct ViaGlob; } }
pub(crate) use m;
"#,
                ),
            ],
        );

        let name = Name::new(db, "ViaGlob".to_owned());
        let result = resolve_name(db, root_module, source_root, name, Namespace::Type);
        let rendered = match &result {
            Ok(sym) => fmt_symbol(db, *sym),
            Err(e) => format!("{e:?}"),
        };
        expect![[r#"Unresolved"#]].assert_eq(&rendered);
    });
}

// ---------------------------------------------------------------------------
// Order independence: same macros in different source order
// ---------------------------------------------------------------------------

#[test]
fn order_independence_macros_resolve_regardless_of_source_order() {
    let db = Database::default();
    db.attach(|db| {
        // Invocation before definition — should still resolve.
        let (source_root, root_module) = setup_single(
            db,
            r#"
m!();
macro_rules! m { () => { struct OrderTest; } }
"#,
        );

        let name = Name::new(db, "OrderTest".to_owned());
        let result = resolve_name(db, root_module, source_root, name, Namespace::Type);
        let rendered = match &result {
            Ok(sym) => fmt_symbol(db, *sym),
            Err(e) => format!("{e:?}"),
        };
        expect![[r#"<local Struct OrderTest>"#]].assert_eq(&rendered);
    });
}
