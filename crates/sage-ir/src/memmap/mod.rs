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

use sage_stash::{Slice, Stash, Stashed};

use crate::Db;
use crate::local_syms::mods::LocalModSym;
use crate::symbol::ModSymbol;
use crate::resolve::SourceRoot;

/// The Minimally Expanded Member Map (MEM-map) for a single module.
#[salsa::tracked(debug)]
pub struct ExpandedModule<'db> {
    #[returns(ref)]
    pub memmap: Memmap<'db>,
}

impl<'db> ExpandedModule<'db> {
    pub fn stash(self, db: &'db dyn Db) -> &'db Stash {
        self.memmap(db).stash()
    }

    pub fn entries(self, db: &'db dyn Db) -> Slice<MemmapEntry<'db>> {
        *self.memmap(db).root()
    }
}

/// Compute the MEM-map for a local module. Seeds from `parse_source_file`
/// (for file-backed modules) or from the LocalModSym's inline `items` field
/// (for `mod foo { ... }`), then resolves and expands macros.
///
/// Keyed on `LocalModSym` — external modules don't have memmaps.
#[salsa::tracked(returns(ref), cycle_initial = expanded_module_initial)]
pub fn expanded_module<'db>(
    db: &'db dyn Db,
    module: LocalModSym<'db>,
    source_root: SourceRoot,
) -> ExpandedModule<'db> {
    let items = module.unexpanded_items(db);
    let mut stash = Stash::new();
    let root = seed::seed_from_items(db, &items, &mut stash);

    expand::resolve_and_expand_macros(db, ModSymbol::ast(module), source_root, &mut stash, root);

    let memmap = Stashed::new(stash, root);
    ExpandedModule::new(db, memmap)
}

/// Cycle recovery initial value: empty MEM-map.
fn expanded_module_initial<'db>(
    db: &'db dyn Db,
    _id: salsa::Id,
    _module: LocalModSym<'db>,
    _source_root: SourceRoot,
) -> ExpandedModule<'db> {
    let mut stash = Stash::new();
    let root: Slice<MemmapEntry<'db>> = stash.alloc_slice(&[]);
    let memmap = Stashed::new(stash, root);
    ExpandedModule::new(db, memmap)
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
    match module {
        ModSymbol::Ast(ast) => *expanded_module(db, ast, source_root),
        ModSymbol::Ext(_) => {
            let mut stash = Stash::new();
            let root = stash.alloc_slice(&[]);
            let memmap = Stashed::new(stash, root);
            ExpandedModule::new(db, memmap)
        }
    }
}
