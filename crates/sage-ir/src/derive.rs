pub mod builtins;

use crate::Db;
use crate::item::*;
use crate::module::Module;
use crate::name::Name;
use crate::resolve::{MacroKind, Namespace, SourceRoot, resolve_name};
use crate::symbol::Symbol;
use crate::types::{AttrKind, TokenTree};

/// Result of expanding a single derive.
#[derive(Clone, PartialEq, Eq)]
pub enum DeriveResult<'db> {
    Builtin { impls: Vec<ImplItem<'db>> },
    ProcMacro { symbol: Symbol<'db> },
}

/// Resolve and expand all derives on an item.
pub fn expand_derives<'db>(
    db: &'db dyn Db,
    module: Module<'db>,
    source_root: SourceRoot,
    crate_root: Module<'db>,
    item: Item<'db>,
) -> Vec<DeriveResult<'db>> {
    let attrs = match item {
        Item::Struct(s) => s.attrs(db).to_vec(),
        Item::Enum(e) => e.attrs(db).to_vec(),
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
                crate_root,
                derive_name,
                Namespace::Macro(MacroKind::Derive),
            ) {
                Ok(symbol) => {
                    let (cn, di) = match symbol.source(db) {
                        crate::symbol::SymbolSource::External(cn, di) => (cn, di),
                        _ => {
                            // Local derive — treat as proc-macro stub
                            results.push(DeriveResult::ProcMacro { symbol });
                            continue;
                        }
                    };
                    if db.tcx().is_builtin_derive(cn, di) {
                        let impls = builtins::expand_builtin(db, derive_name, item).clone();
                        results.push(DeriveResult::Builtin { impls });
                    } else {
                        results.push(DeriveResult::ProcMacro { symbol });
                    }
                }
                Err(_) => {
                    // Unresolved derive — skip for now.
                    // In a real compiler this would be an error.
                }
            }
        }
    }
    results
}

/// Extract individual derive names from a `#[derive(A, B, C)]` attribute's args.
///
/// The args text looks like `(Debug, Clone, Default)`.
pub fn parse_derive_args<'db>(db: &'db dyn Db, args: TokenTree<'db>) -> Vec<Name<'db>> {
    let text = args.text(db);
    // Strip outer parens
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
