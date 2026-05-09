//! Macro resolution and expansion within the MEM-map.
//!
//! Uses a snapshot-based approach: entries are cloned before mutation so that
//! resolution can read the snapshot while expansion mutates the live entries.
//! This avoids the need for interior mutability or multi-pass algorithms.
//!
//! `expand_macro` parses body tokens with tree-sitter to extract names. This is
//! the only place in the memmap module that uses tree-sitter, and it operates on
//! the macro body string — not the source file.

use crate::Db;
use crate::item::{Item, MacroDefItem};
use crate::module::Module;
use crate::name::Name;
use crate::resolve::{Namespace, SourceRoot};
use crate::span::SpanIndices;
use crate::types::Path;

use super::data::*;
use super::resolve_path::{MacroResolution, resolve_memmap_path};

/// Maximum macro expansion depth (same as rustc's default).
/// Rustc makes this configurable via `#![recursion_limit = "N"]`; we hardcode 128.
const MAX_EXPANSION_DEPTH: usize = 128;

/// Resolve and expand all unresolved MacroUse entries in the MEM-map.
///
/// This is the public entry point. It snapshots the entries for resolution,
/// then walks the entries expanding macros. Recursive calls reuse the same
/// snapshot so that macro-expanded subtrees resolve names against the
/// top-level module state.
pub(super) fn resolve_and_expand_macros<'db>(
    db: &'db dyn Db,
    module: Module<'db>,
    source_root: SourceRoot,
    crate_root: Module<'db>,
    entries: &mut Vec<MemmapEntry<'db>>,
    depth: usize,
) {
    // Snapshot once at the outermost call; recursive calls re-use it.
    let snapshot: Vec<MemmapEntry<'db>> = entries.clone();
    resolve_with_snapshot(
        db,
        module,
        source_root,
        crate_root,
        entries,
        &snapshot,
        depth,
    );
}

/// Inner worker: resolve and expand macros in `entries`, using `root_entries`
/// as the resolution context. Unifies what used to be two near-identical
/// functions (one that created the snapshot, one that received it).
fn resolve_with_snapshot<'db>(
    db: &'db dyn Db,
    module: Module<'db>,
    source_root: SourceRoot,
    crate_root: Module<'db>,
    entries: &mut Vec<MemmapEntry<'db>>,
    root_entries: &[MemmapEntry<'db>],
    depth: usize,
) {
    if depth >= MAX_EXPANSION_DEPTH {
        for entry in entries.iter_mut() {
            if let MemmapEntry::MacroUse(mu) = entry {
                if matches!(mu.state, MacroUseState::Unresolved) {
                    mu.state = MacroUseState::Error;
                }
            }
        }
        return;
    }

    for i in 0..entries.len() {
        if let MemmapEntry::MacroUse(mu) = &entries[i] {
            if !matches!(mu.state, MacroUseState::Unresolved) {
                continue;
            }
            let path = mu.path;

            match resolve_memmap_path(db, module, source_root, crate_root, root_entries, path) {
                MacroResolution::Found(macro_def) => {
                    let mut expanded = expand_macro(db, macro_def);
                    resolve_with_snapshot(
                        db,
                        module,
                        source_root,
                        crate_root,
                        &mut expanded,
                        root_entries,
                        depth + 1,
                    );
                    entries[i] = MemmapEntry::MacroUse(MacroUse {
                        path,
                        state: MacroUseState::Expanded(expanded),
                    });
                }
                MacroResolution::Ambiguous => {
                    entries[i] = MemmapEntry::MacroUse(MacroUse {
                        path,
                        state: MacroUseState::Ambiguous,
                    });
                }
                MacroResolution::NotFound => {}
            }
        }
    }
}

/// Expand a no-arg macro: parse body_tokens as items and return MemmapEntry values.
pub fn expand_macro<'db>(db: &'db dyn Db, macro_def: MacroDefItem<'db>) -> Vec<MemmapEntry<'db>> {
    let body = macro_def.body_tokens(db);
    if body.is_empty() {
        return Vec::new();
    }

    let mut parser = tree_sitter::Parser::new();
    parser
        .set_language(&tree_sitter_rust::LANGUAGE.into())
        .expect("failed to set tree-sitter-rust language");
    let tree = match parser.parse(body.as_str(), None) {
        Some(t) => t,
        None => return Vec::new(),
    };

    let root = tree.root_node();
    let mut entries = Vec::new();
    let mut cursor = root.walk();

    for child in root.children(&mut cursor) {
        if !child.is_named() {
            continue;
        }
        match child.kind() {
            "struct_item" | "enum_item" | "trait_item" | "type_item" | "mod_item" => {
                if let Some(name_node) = child.child_by_field_name("name") {
                    let name = Name::new(db, body[name_node.byte_range()].to_owned());
                    entries.push(MemmapEntry::Named(NamedMember {
                        name,
                        ns: Namespace::Type,
                        kind: NamedMemberKind::Item(Item::Error(SpanIndices { start: 0, end: 0 })),
                    }));
                    if child.kind() == "struct_item" {
                        entries.push(MemmapEntry::Named(NamedMember {
                            name,
                            ns: Namespace::Value,
                            kind: NamedMemberKind::Item(Item::Error(SpanIndices {
                                start: 0,
                                end: 0,
                            })),
                        }));
                    }
                }
            }
            "function_item" | "const_item" | "static_item" => {
                if let Some(name_node) = child.child_by_field_name("name") {
                    let name = Name::new(db, body[name_node.byte_range()].to_owned());
                    entries.push(MemmapEntry::Named(NamedMember {
                        name,
                        ns: Namespace::Value,
                        kind: NamedMemberKind::Item(Item::Error(SpanIndices { start: 0, end: 0 })),
                    }));
                }
            }
            "impl_item" => {
                entries.push(MemmapEntry::Anon(Item::Error(SpanIndices {
                    start: 0,
                    end: 0,
                })));
            }
            "macro_definition" => {
                if let Some(name_node) = child.child_by_field_name("name") {
                    let name = Name::new(db, body[name_node.byte_range()].to_owned());
                    let span = SpanIndices {
                        start: child.start_byte() as u32,
                        end: child.end_byte() as u32,
                    };
                    let body_tokens = crate::ts_helpers::extract_macro_body_tokens(child, body);
                    let def = MacroDefItem::new(db, name, body_tokens, span);
                    entries.push(MemmapEntry::Named(NamedMember {
                        name,
                        ns: Namespace::Macro(crate::resolve::MacroKind::Bang),
                        kind: NamedMemberKind::MacroDef(def),
                    }));
                }
            }
            "expression_statement" => {
                if let Some(invoc) = child
                    .named_children(&mut child.walk())
                    .find(|c| c.kind() == "macro_invocation")
                {
                    if let Some(macro_node) = invoc.child_by_field_name("macro") {
                        let segments =
                            crate::ts_helpers::collect_macro_path_segments(db, macro_node, body);
                        if !segments.is_empty() {
                            let span = SpanIndices {
                                start: invoc.start_byte() as u32,
                                end: invoc.end_byte() as u32,
                            };
                            let path = Path::new(db, segments, span);
                            entries.push(MemmapEntry::MacroUse(MacroUse {
                                path,
                                state: MacroUseState::Unresolved,
                            }));
                        }
                    }
                }
            }
            _ => {}
        }
    }

    entries
}
