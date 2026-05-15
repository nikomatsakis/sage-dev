//! Module MEM-map: Minimal Expanded Members map.
//!
//! The MEM-map is the single source of truth for "what names does a module export?"
//! It performs the minimal work needed to resolve names: macro invocations are expanded
//! only to discover what names they introduce, not to fully lower their contents.
//!
//! # Data flow
//!
//! ```text
//! Source text
//!   → tree-sitter [in parse_source_file only] → Vec<ItemAst>
//!                                               ↓
//!                                       expanded_module (Item → MemmapEntry, resolve, expand)
//!                                               ↓
//!                                       resolve_name / memmap_errors
//! ```
//!
//! # Key design decisions
//!
//! - **No direct tree-sitter in seeding**: `expanded_module` reads only from `parse_source_file`,
//!   which provides the incremental firewall — body-only edits don't invalidate the memmap.
//! - **Snapshot-based expansion**: macros are resolved against a snapshot of entries to avoid
//!   reading while mutating.
//! - **Fixpoint with cycle recovery**: cross-module macro resolution uses salsa's cycle
//!   recovery (initial value = empty memmap) to converge.
//!
//! # Keying
//!
//! `expanded_module` is keyed on `ModAst` rather than `ModSymbol`.
//! External modules don't have memmaps — `ModSymbol::resolve_member`
//! dispatches on `data()` and consults `TcxDb::module_children`
//! directly for the ext arm.

mod data;
mod expand;
mod resolve_path;
mod seed;
mod validate;

pub use data::*;
pub use expand::expand_macro;
pub use validate::{MemmapError, memmap_errors};

use crate::Db;
use crate::item::ModAst;
use crate::module::ModSymbol;
use crate::resolve::SourceRoot;

/// The Minimally Expanded Member Map (MEM-map) for a single module.
#[salsa::tracked(debug)]
pub struct ExpandedModule<'db> {
    #[returns(ref)]
    pub entries: Vec<MemmapEntry<'db>>,
}

/// Compute the MEM-map for a local module. Seeds from `parse_source_file`
/// (for file-backed modules) or from the ModAst's inline `items` field
/// (for `mod foo { ... }`), then resolves and expands macros.
///
/// Keyed on `ModAst` — external modules don't have memmaps.
#[salsa::tracked(returns(ref), cycle_initial = expanded_module_initial)]
pub fn expanded_module<'db>(
    db: &'db dyn Db,
    module: ModAst<'db>,
    source_root: SourceRoot,
) -> ExpandedModule<'db> {
    let items = module.unexpanded_items(db);
    let mut entries = seed::seed_from_items(db, &items);

    expand::resolve_and_expand_macros(db, ModSymbol::ast(module), source_root, &mut entries);

    ExpandedModule::new(db, entries)
}

/// Cycle recovery initial value: empty MEM-map.
fn expanded_module_initial<'db>(
    db: &'db dyn Db,
    _id: salsa::Id,
    _module: ModAst<'db>,
    _source_root: SourceRoot,
) -> ExpandedModule<'db> {
    ExpandedModule::new(db, Vec::new())
}

/// Convenience wrapper: dispatch on `ModSymbol`. For external modules
/// returns an empty placeholder (their contents are queried via
/// `TcxDb` directly, not via the memmap).
#[salsa::tracked]
pub fn module_memmap<'db>(
    db: &'db dyn Db,
    module: ModSymbol<'db>,
    source_root: SourceRoot,
) -> ExpandedModule<'db> {
    match module.data() {
        crate::module::ModSymbolData::Ast(ast) => *expanded_module(db, ast, source_root),
        crate::module::ModSymbolData::Ext(_) => ExpandedModule::new(db, Vec::new()),
    }
}
