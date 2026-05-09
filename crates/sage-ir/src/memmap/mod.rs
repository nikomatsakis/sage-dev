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
//!   → tree-sitter [in file_item_tree only] → Vec<Item>
//!                                               ↓
//!                                       module_memmap (Item → MemmapEntry, resolve, expand)
//!                                               ↓
//!                                       resolve_name / memmap_errors
//! ```
//!
//! # Key design decisions
//!
//! - **No direct tree-sitter in seeding**: `module_memmap` reads only from `file_item_tree`,
//!   which provides the incremental firewall — body-only edits don't invalidate the memmap.
//! - **Snapshot-based expansion**: macros are resolved against a snapshot of entries to avoid
//!   reading while mutating.
//! - **Fixpoint with cycle recovery**: cross-module macro resolution uses salsa's cycle
//!   recovery (initial value = empty memmap) to converge.

mod data;
mod expand;
mod resolve_path;
mod seed;
mod validate;

pub use data::*;
pub use expand::expand_macro;
pub use validate::{MemmapError, memmap_errors};

use crate::Db;
use crate::lower::file_item_tree;
use crate::module::{Module, ModuleSource};
use crate::resolve::SourceRoot;

/// The MEM-map for a single module.
#[salsa::tracked(debug)]
pub struct ModuleMemmap<'db> {
    #[returns(ref)]
    pub entries: Vec<MemmapEntry<'db>>,
}

/// Compute the MEM-map for a module. Seeds from file_item_tree, then resolves and
/// expands macros. Uses salsa fixpoint for cross-module convergence.
///
/// # Panics (debug only)
///
/// Panics if called on an external module. External module contents should be
/// queried via `TcxDb` directly.
#[salsa::tracked(returns(ref), cycle_initial = module_memmap_initial)]
pub fn module_memmap<'db>(
    db: &'db dyn Db,
    module: Module<'db>,
    source_root: SourceRoot,
    crate_root: Module<'db>,
) -> ModuleMemmap<'db> {
    debug_assert!(
        !matches!(module.source(db), ModuleSource::External(..)),
        "module_memmap should not be called on external modules"
    );

    let mut entries = match module.source(db) {
        ModuleSource::Local { file, .. } => {
            let items = file_item_tree(db, file);
            seed::seed_from_items(db, module, source_root, crate_root, items)
        }
        ModuleSource::External(..) => Vec::new(),
    };

    expand::resolve_and_expand_macros(db, module, source_root, crate_root, &mut entries, 0);

    ModuleMemmap::new(db, entries)
}

/// Cycle recovery initial value: empty MEM-map.
fn module_memmap_initial<'db>(
    db: &'db dyn Db,
    _id: salsa::Id,
    _module: Module<'db>,
    _source_root: SourceRoot,
    _crate_root: Module<'db>,
) -> ModuleMemmap<'db> {
    ModuleMemmap::new(db, Vec::new())
}
