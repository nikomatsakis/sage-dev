pub mod builtins;

use crate::Db;
use crate::item::*;
use crate::lower::parse_source_file;
use crate::module::{CrateNum, DefIndex, ModSymbol};
use crate::name::Name;
use crate::resolve::{MacroKind, Namespace, SourceRoot, definition_in_ns, resolve_name};
use crate::source::SourceFile;
use crate::symbol::{Symbol, SymbolData};
use crate::types::{AttrKind, TokenTree};

/// Result of expanding a single derive.
#[derive(Clone, PartialEq, Eq)]
pub enum DeriveResult<'db> {
    Builtin { impls: Vec<ImplAst<'db>> },
    Expanded { items: Vec<ItemAst<'db>> },
    ProcMacro { symbol: Symbol<'db> },
}

/// Resolve and expand all derives on an item.
pub fn expand_derives<'db>(
    db: &'db dyn Db,
    module: ModSymbol<'db>,
    source_root: SourceRoot,
    item: ItemAst<'db>,
) -> Vec<DeriveResult<'db>> {
    let attrs = match item {
        ItemAst::Struct(s) => s.attrs(db).to_vec(),
        ItemAst::Enum(e) => e.attrs(db).to_vec(),
        _ => return Vec::new(),
    };

    let mut results = Vec::new();
    for attr in &attrs {
        if attr.kind(db) != AttrKind::Normal {
            continue;
        }
        let path_segs = attr.path(db).segments(db);
        if path_segs.len() != 1 || path_segs[0].text(db) != "derive" {
            continue;
        }
        let Some(args) = attr.args(db) else { continue };
        let names = parse_derive_args(db, args);
        for derive_name in names {
            match resolve_name(
                db,
                module,
                source_root,
                derive_name,
                Namespace::Macro(MacroKind::Derive),
            ) {
                Ok(symbol) => {
                    let (cn, di) = match symbol.data() {
                        SymbolData::Ext(ext) => (ext.crate_num, ext.def_index),
                        _ => {
                            results.push(DeriveResult::ProcMacro { symbol });
                            continue;
                        }
                    };
                    if db.tcx().is_builtin_derive(cn, di) {
                        let impls = builtins::expand_builtin(db, derive_name, item).clone();
                        results.push(DeriveResult::Builtin { impls });
                    } else if let Some(result) =
                        try_expand_proc_macro(db, item, cn, di, derive_name)
                    {
                        results.push(result);
                    } else {
                        results.push(DeriveResult::ProcMacro { symbol });
                    }
                }
                Err(_) => {}
            }
        }
    }
    results
}

/// Try to expand a proc-macro derive.
///
/// The resolved DefId may point to a trait (Type namespace) rather than the
/// derive macro (Macro namespace) when a crate re-exports both under the
/// same name (e.g. clap re-exports `Parser` as both a trait from
/// `clap_builder` and a derive from `clap_derive`).
///
/// Strategy:
/// 1. Try the resolved DefId directly.
/// 2. Re-resolve in the Derive namespace via `definition_in_ns` on the
///    extern crate that the name was imported from.
fn try_expand_proc_macro<'db>(
    db: &'db dyn Db,
    item: ItemAst<'db>,
    cn: CrateNum,
    di: DefIndex,
    derive_name: Name<'db>,
) -> Option<DeriveResult<'db>> {
    let item_source = extract_item_source(db, item)?;

    // 1. Try the resolved DefId directly.
    if let Some(expanded) = db.tcx().expand_proc_macro_derive(cn, di, &item_source) {
        return Some(DeriveResult::Expanded {
            items: lower_expanded_source(db, &expanded),
        });
    }

    // 2. The resolved DefId might be a trait. Try to find the derive macro
    //    by looking up the name in the Derive namespace of every extern crate
    //    that has a child named like the derive.
    //    We try the crate that the resolved symbol belongs to first, then
    //    fall back to searching all extern crates that re-export this name.
    let name_text = derive_name.text(db);

    // Try common crate names: the derive name might come from a crate
    // whose name matches a known extern crate.
    // Walk through use imports to find the source crate.
    // Simplest approach: try extern_crate for common patterns.
    for crate_name in potential_crate_names(&name_text) {
        if let Some(ext_cn) = db.tcx().extern_crate(&crate_name) {
            let ext_module = ModSymbol::external(ext_cn, DefIndex(0));
            if let Some(sym) = definition_in_ns(
                db,
                ext_module,
                derive_name,
                Namespace::Macro(MacroKind::Derive),
            ) {
                if let SymbolData::Ext(ext) = sym.data() {
                    if let Some(expanded) = db.tcx().expand_proc_macro_derive(
                        ext.crate_num,
                        ext.def_index,
                        &item_source,
                    ) {
                        return Some(DeriveResult::Expanded {
                            items: lower_expanded_source(db, &expanded),
                        });
                    }
                }
            }
        }
    }

    None
}

/// Generate potential crate names to search for a derive macro.
/// For a derive like `Parser`, we try crates that commonly export derives:
/// all extern crates (we just try the ones we know about).
fn potential_crate_names(derive_name: &str) -> Vec<String> {
    // Common patterns: the derive is often re-exported from a crate
    // whose name we can guess. For now, try lowercase of the derive name
    // and common crate names.
    let lower = derive_name.to_lowercase();
    vec![
        lower.clone(),
        format!("{lower}_derive"),
        // clap-specific: Parser/Subcommand/Args/ValueEnum are in clap
        "clap".to_owned(),
        "serde".to_owned(),
        "tokio".to_owned(),
    ]
}

/// Get the source text for a struct/enum item.
fn extract_item_source<'db>(db: &'db dyn Db, item: ItemAst<'db>) -> Option<String> {
    let span = match item {
        ItemAst::Struct(s) => s.span(db),
        ItemAst::Enum(e) => e.span(db),
        _ => return None,
    };
    let text = span.source.text(db);
    let start = span.start as usize;
    let end = span.end as usize;
    if end <= text.len() {
        Some(text[start..end].to_owned())
    } else {
        None
    }
}

/// Lower expanded source text through tree-sitter into IR items.
fn lower_expanded_source<'db>(db: &'db dyn Db, text: &str) -> Vec<ItemAst<'db>> {
    let file = SourceFile::new(db, "<proc-macro-expansion>".to_owned(), text.to_owned());
    parse_source_file(db, file).clone()
}

/// Extract individual derive names from a `#[derive(A, B, C)]` attribute's args.
pub fn parse_derive_args<'db>(db: &'db dyn Db, args: TokenTree<'db>) -> Vec<Name<'db>> {
    let text = args.text(db);
    let inner = text
        .trim()
        .strip_prefix('(')
        .and_then(|s| s.strip_suffix(')'))
        .unwrap_or(text);
    inner
        .split(',')
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .map(|s| Name::new(db, s.to_owned()))
        .collect()
}
