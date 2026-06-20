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
//! `expanded_module` is keyed on `LocalModSym` rather than `ModSymbol`.
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

use sage_stash::Stash;

use crate::Db;
use crate::local_syms::mods::LocalModSym;
use crate::symbol::Symbol;

/// Compute the MEM-map for a local module. Seeds from `parse_source_file`
/// (for file-backed modules) or from the LocalModSym's inline `items` field
/// (for `mod foo { ... }`), then resolves and expands macros.
///
/// Keyed on `LocalModSym` — external modules don't have memmaps.
#[salsa::tracked(returns(ref), cycle_initial = expanded_module_initial)]
pub fn local_expanded_module_items<'db>(
    db: &'db dyn Db,
    module: LocalModSym<'db>,
) -> Vec<Symbol<'db>> {
    let items = module.unexpanded_items(db);
    let mut stash = Stash::new();
    let entries = seed::seed_from_items(db, items, &mut stash);

    loop {
        let changed = expand::resolve_expand_pass(db, module, &mut stash, entries, entries, 0);
        if !changed {
            break;
        }
    }

    todo!("flatten out symbols from root")
}

/// Cycle recovery initial value.
fn expanded_module_initial<'db>(
    _db: &'db dyn Db,
    _id: salsa::Id,
    _module: LocalModSym<'db>,
) -> Vec<Symbol<'db>> {
    vec![]
}
