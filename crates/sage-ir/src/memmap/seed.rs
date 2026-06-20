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
use crate::cst::paths::{Path, PathAnchorKind};
use crate::cst::uses::UseKind;
use crate::local_syms::LocalModItemSym;
use crate::name::Name;

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
                let cst = inv.cst(db);
                let (inv_stash, inv_cst) = cst.open_deref();
                let input_text = inv_cst.input_tokens.text(db).clone();
                let input = MacroInput::new(db, input_text, inv.span(db));
                let names = path_to_names(db, inv_stash, inv_stash[inv_cst.path]);
                let path = stash.alloc_slice(&names);
                let expansions = stash.alloc_slice(&[]);
                entries.push(MemmapEntry::MacroInvocation(MacroInvocation {
                    path,
                    input,
                    expansions,
                }));
            }
            LocalModItemSym::Use(group) => {
                let (imports_stash, imports_root) = group.imports(db).open();
                for import in &imports_stash[imports_root] {
                    let names = path_to_names(db, &imports_stash, imports_stash[import.path]);
                    match import.kind {
                        UseKind::Named(alias) => {
                            let target = stash.alloc_slice(&names);
                            entries.push(MemmapEntry::Redirect {
                                name: alias,
                                target,
                            });
                        }
                        UseKind::Glob => {
                            let path = stash.alloc_slice(&names);
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

/// Flatten a `Path` into a Vec of Names for memmap resolution.
fn path_to_names<'db>(
    db: &'db dyn crate::Db,
    stash: &sage_stash::Stash,
    path: Path<'db>,
) -> Vec<Name<'db>> {
    let mut names = Vec::new();
    match path {
        Path::Anchored(anchor, seg_slice) => {
            collect_anchor_names(db, stash, anchor, &mut names);
            let segs = &stash[seg_slice];
            names.extend(segs.iter().map(|s| s.name));
        }
        Path::Relative(first, rest_slice) => {
            names.push(first.name);
            let rest = &stash[rest_slice];
            names.extend(rest.iter().map(|s| s.name));
        }
    }
    names
}

fn collect_anchor_names<'db>(
    db: &'db dyn crate::Db,
    stash: &sage_stash::Stash,
    anchor: crate::cst::paths::PathAnchor<'db>,
    out: &mut Vec<Name<'db>>,
) {
    match anchor.kind {
        PathAnchorKind::ExternCrate(name) => {
            out.push(Name::new(db, String::new()));
            out.push(name);
        }
        PathAnchorKind::CurrentCrate => {
            out.push(Name::new(db, "crate".to_owned()));
        }
        PathAnchorKind::Self_ => {
            out.push(Name::new(db, "self".to_owned()));
        }
        PathAnchorKind::DollarCrate => {
            out.push(Name::new(db, "$crate".to_owned()));
        }
        PathAnchorKind::Super(inner_ptr) => {
            let inner = stash[inner_ptr];
            collect_anchor_names(db, stash, inner, out);
            out.push(Name::new(db, "super".to_owned()));
        }
    }
}
