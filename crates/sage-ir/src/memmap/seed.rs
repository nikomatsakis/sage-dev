//! Seeding MEM-map entries from `file_item_tree` items.
//!
//! This module transforms `Vec<Item>` (from lowering) into `Vec<MemmapEntry>`.
//! It never touches tree-sitter — all parsing happens in `file_item_tree`.
//! This separation provides the incremental firewall: body-only edits don't
//! invalidate the memmap because `file_item_tree` produces the same tracked
//! struct identities when only body fields change.

use crate::Db;
use crate::item::Item;
use crate::module::Module;
use crate::resolve::{
    Namespace, SourceRoot, item_in_namespace, item_name, resolve_use_path_to_module,
};
use crate::types::UseKind;

use super::data::*;

/// Seed MEM-map entries from file_item_tree items (no tree-sitter).
pub(super) fn seed_from_items<'db>(
    db: &'db dyn Db,
    module: Module<'db>,
    source_root: SourceRoot,
    crate_root: Module<'db>,
    items: &[Item<'db>],
) -> Vec<MemmapEntry<'db>> {
    let mut entries = Vec::new();
    for &item in items {
        match item {
            Item::MacroDef(def) => {
                entries.push(MemmapEntry::Named(NamedMember {
                    name: def.name(db),
                    ns: Namespace::Macro(crate::resolve::MacroKind::Bang),
                    kind: NamedMemberKind::MacroDef(def),
                }));
            }
            Item::MacroInvocation(inv) => {
                entries.push(MemmapEntry::MacroUse(MacroUse {
                    path: inv.path(db),
                    state: MacroUseState::Unresolved,
                }));
            }
            Item::Use(group) => {
                for import in group.imports(db) {
                    match import.kind(db) {
                        UseKind::Named(alias) => {
                            entries.push(MemmapEntry::Named(NamedMember {
                                name: alias,
                                ns: Namespace::Type,
                                kind: NamedMemberKind::Redirect {
                                    target: import.path(db),
                                },
                            }));
                        }
                        UseKind::Glob => {
                            if let Ok(target) = resolve_use_path_to_module(
                                db,
                                module,
                                source_root,
                                crate_root,
                                *import,
                            ) {
                                entries.push(MemmapEntry::Glob(GlobStem {
                                    source_module: target,
                                }));
                            }
                        }
                        UseKind::Unnamed => {}
                    }
                }
            }
            Item::Impl(_) => {
                entries.push(MemmapEntry::Anon(item));
            }
            Item::Error(_) => {}
            _ => {
                if let Some(name) = item_name(db, item) {
                    if item_in_namespace(db, item, Namespace::Type) {
                        entries.push(MemmapEntry::Named(NamedMember {
                            name,
                            ns: Namespace::Type,
                            kind: NamedMemberKind::Item(item),
                        }));
                    }
                    if item_in_namespace(db, item, Namespace::Value) {
                        entries.push(MemmapEntry::Named(NamedMember {
                            name,
                            ns: Namespace::Value,
                            kind: NamedMemberKind::Item(item),
                        }));
                    }
                }
            }
        }
    }
    entries
}
