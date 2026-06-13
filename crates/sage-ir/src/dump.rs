//! Demo entry point for "expanded module by path" — milestone 1 of the
//! module/symbol-tree RFD.
//!
//! `dump_expanded_module` takes a starting scope and a `::`-separated
//! path string, walks the path through the new IR types, and returns
//! the expanded memmap of the module it names.

use crate::Db;
use crate::memmap::{ExpandedModule, expanded_module};
use crate::module::{ModSymbol, ModSymbolData};
use crate::resolve::{SourceRoot, resolve_module_path};

/// Resolve `path` (e.g. `"crate::foo::bar"`) starting from `root` and
/// return the expanded memmap of the module it names.
///
/// Path syntax: `::`-separated segments. A leading `crate` segment
/// rewrites against `root.crate_root()`; otherwise the path is walked
/// from `root` directly.
///
/// Returns `None` if the path doesn't resolve to a module.
pub fn dump_expanded_module<'db>(
    db: &'db dyn Db,
    root: ModSymbol<'db>,
    source_root: SourceRoot,
    path: &str,
) -> Option<ExpandedModule<'db>> {
    let mut segments: Vec<&str> = path.split("::").filter(|s| !s.is_empty()).collect();
    let start = if segments.first().copied() == Some("crate") {
        segments.remove(0);
        root.crate_root(db)
    } else {
        root
    };
    let module = resolve_module_path(db, start, source_root, &segments)?;
    let ast = match module {
        ModSymbol::Ast(a) => a,
        ModSymbol::Ext(_) => return None,
    };
    Some(*expanded_module(db, ast, source_root))
}
