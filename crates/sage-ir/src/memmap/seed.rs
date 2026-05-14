//! Seeding MEM-map entries from `file_item_tree` items.
//!
//! Transforms `Vec<ItemAst>` (from lowering) into `Vec<MemmapEntry>`. Never
//! touches tree-sitter — all parsing happens in `file_item_tree`. This
//! separation provides the incremental firewall: body-only edits don't
//! invalidate the memmap because `file_item_tree` produces the same
//! tracked-struct identities when only body fields change.
//!
//! Seeding is a pure transform:
//!   - Items with names go into `MemmapEntry::Item` (namespace
//!     resolved at lookup time via `item_in_namespace`).
//!   - `macro_rules!` definitions become `MemmapEntry::MacroDef`.
//!   - `use foo::bar as alias` becomes a `Redirect { name: alias,
//!     target }` — the target's namespace is resolved at lookup time.
//!   - `use foo::*` becomes a `Glob { path }` — the path is resolved
//!     dynamically, not at seed time, so globs whose target is created
//!     by macro expansion are picked up correctly.
//!   - `m!()` becomes a `MacroUse` in state `Unresolved`, carrying the
//!     invocation's argument tokens forward.
//!   - Anonymous items (impls) stay as `Item` entries — `item_name()`
//!     returns `None` so walkers naturally skip them.
//!   - `ItemAst::Error` and `ItemAst::Use` are never emitted as-is — they're
//!     either dropped or transformed above.

use crate::Db;
use crate::item::ItemAst;
use crate::types::UseKind;

use super::data::*;

/// Seed MEM-map entries from file_item_tree items.
pub(super) fn seed_from_items<'db>(
    db: &'db dyn Db,
    items: &[ItemAst<'db>],
) -> Vec<MemmapEntry<'db>> {
    let mut entries = Vec::new();
    for &item in items {
        match item {
            ItemAst::MacroDef(def) => {
                entries.push(MemmapEntry::MacroDef(def));
            }
            ItemAst::MacroInvocation(inv) => {
                entries.push(MemmapEntry::MacroUse(MacroUse {
                    path: inv.path(db),
                    input_tokens: inv.input_tokens(db).clone(),
                    state: MacroUseState::Unresolved,
                }));
            }
            ItemAst::Use(group) => {
                for import in group.imports(db) {
                    match import.kind(db) {
                        UseKind::Named(alias) => {
                            entries.push(MemmapEntry::Redirect {
                                name: alias,
                                target: import.path(db),
                            });
                        }
                        UseKind::Glob => {
                            entries.push(MemmapEntry::Glob {
                                path: import.path(db),
                            });
                        }
                        UseKind::Unnamed => {}
                    }
                }
            }
            ItemAst::Error(_) => {}
            _ => {
                entries.push(MemmapEntry::Item(item));
            }
        }
    }
    entries
}
