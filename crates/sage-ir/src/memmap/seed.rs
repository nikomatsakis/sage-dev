//! Seeding MEM-map entries from `parse_source_file` items.
//!
//! Transforms `Vec<ItemAst>` (from lowering) into a stash-allocated
//! `Slice<MemmapEntry>`. Never touches tree-sitter — all parsing happens
//! in `parse_source_file`. This separation provides the incremental
//! firewall: body-only edits don't invalidate the memmap because
//! `parse_source_file` produces the same tracked-struct identities when
//! only body fields change.

use sage_stash::{Slice, Stash};

use crate::Db;
use crate::local_syms::LocalModItemSym;
use crate::types::UseKind;

use super::data::*;

/// Seed MEM-map entries from parse_source_file items.
pub(super) fn seed_from_items<'db>(
    db: &'db dyn Db,
    items: &[LocalModItemSym<'db>],
    stash: &mut Stash,
) -> Slice<MemmapEntry<'db>> {
    let mut entries: Vec<MemmapEntry<'db>> = Vec::new();
    for &item in items {
        match item {
            LocalModItemSym::MacroDef(def) => {
                entries.push(MemmapEntry::MacroDef(def));
            }
            LocalModItemSym::MacroInvocation(inv) => {
                let input = MacroInput::new(db, inv.input_tokens(db).clone(), inv.span(db));
                let path = stash.alloc_slice(inv.path(db));
                let expansions = stash.alloc_slice(&[]);
                entries.push(MemmapEntry::MacroUse(MacroUse {
                    path,
                    input,
                    expansions,
                }));
            }
            LocalModItemSym::Use(group) => {
                let imports = group.imports(db);
                let import_stash = imports.stash();
                for import in &import_stash[*imports.root()] {
                    match import.kind {
                        UseKind::Named(alias) => {
                            let target = stash.alloc_slice(&import_stash[import.path]);
                            entries.push(MemmapEntry::Redirect {
                                name: alias,
                                target,
                            });
                        }
                        UseKind::Glob => {
                            let path = stash.alloc_slice(&import_stash[import.path]);
                            entries.push(MemmapEntry::Glob { path });
                        }
                        UseKind::Unnamed => {}
                    }
                }
            }
            LocalModItemSym::Error(..) => {}
            _ => {
                entries.push(MemmapEntry::Item(item));
                // TODO: re-add TupleStructCtor once LocalStructSym tracks struct kind
            }
        }
    }
    stash.alloc_slice(&entries)
}
