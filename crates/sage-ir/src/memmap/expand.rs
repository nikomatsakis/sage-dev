//! Macro resolution and expansion within the MEM-map (Phase 1).
//!
//! Snapshot-based: entries are cloned before mutation so resolution
//! reads the snapshot while expansion mutates live entries.
//!
//! `expand_macro` parses body tokens with tree-sitter to extract names.
//! This is the only place in the memmap module that uses tree-sitter;
//! it operates on the macro body string, not the source file. Phase 4
//! will route expansion through `file_item_tree` for real `Item` values.

use crate::Db;
use crate::body::{Body, Expr, ExprKind, FunctionBody};
use crate::item::{
    ConstItem, EnumItem, FunctionItem, Item, MacroDefItem, ModItem, StaticItem, StructItem,
    TraitItem, TypeAliasItem,
};
use crate::module::Module;
use crate::name::Name;
use crate::resolve::SourceRoot;
use crate::source::SourceFile;
use crate::span::{SpanIndices, SpanTable};
use crate::types::Path;

use sage_stash::{Stash, Stashed};

use super::data::*;
use super::resolve_path::resolve_macro_path;

const DUMMY_SPAN: SpanIndices = SpanIndices { start: 0, end: 0 };

fn dummy_span_table<'db>(db: &'db dyn Db) -> SpanTable<'db> {
    let file = SourceFile::new(db, "<macro-expansion>".to_owned(), String::new());
    SpanTable::new(db, file, vec![0, 0])
}

fn missing_function_body<'db>() -> FunctionBody<'db> {
    let mut stash = Stash::new();
    let body_expr = stash.alloc(Expr {
        kind: ExprKind::Missing,
        span: DUMMY_SPAN,
    });
    let body = stash.alloc(Body {
        root: body_expr,
        span: DUMMY_SPAN,
    });
    Stashed::new(stash, body)
}

/// Maximum macro expansion depth (same as rustc's default).
const MAX_EXPANSION_DEPTH: usize = 128;

/// Resolve and expand all unresolved `MacroUse` entries in `entries`.
pub(super) fn resolve_and_expand_macros<'db>(
    db: &'db dyn Db,
    module: Module<'db>,
    source_root: SourceRoot,
    crate_root: Module<'db>,
    entries: &mut Vec<MemmapEntry<'db>>,
    depth: usize,
) {
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

/// Inner worker: resolve and expand macros in `entries`, using
/// `root_entries` as the resolution context.
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
        // Leave anything unresolved as Unresolved — the validator will
        // report. Current behaviour used a distinct Error variant; the
        // new model collapses that into Unresolved.
        return;
    }

    for i in 0..entries.len() {
        if let MemmapEntry::MacroUse(mu) = &entries[i] {
            if !matches!(mu.state, MacroUseState::Unresolved) {
                continue;
            }
            let path = mu.path;
            let input_tokens = mu.input_tokens.clone();

            let callees =
                resolve_macro_path(db, module, source_root, crate_root, root_entries, path);
            match callees.len() {
                0 => {
                    // Stay Unresolved — next fixpoint iteration may succeed.
                }
                1 => {
                    let callee = callees[0];
                    let def = match callee {
                        MacroCallee::Rules(def) => def,
                        // Phase 1 only produces Rules; Builtin/Proc come in Phase 3.
                        MacroCallee::Builtin(_) | MacroCallee::Proc { .. } => continue,
                    };
                    let mut expanded = expand_macro(db, def, &input_tokens);
                    resolve_with_snapshot(
                        db,
                        module,
                        source_root,
                        crate_root,
                        &mut expanded,
                        root_entries,
                        depth + 1,
                    );
                    let expansion = Expansion {
                        callee,
                        entries: expanded,
                    };
                    entries[i] = MemmapEntry::MacroUse(MacroUse {
                        path,
                        input_tokens,
                        state: MacroUseState::Expanded(vec![expansion]),
                    });
                }
                _ => {
                    // Multiple candidates — record them as Resolved(callees).
                    // The validator reports this as AmbiguousMacro.
                    entries[i] = MemmapEntry::MacroUse(MacroUse {
                        path,
                        input_tokens,
                        state: MacroUseState::Resolved(callees),
                    });
                }
            }
        }
    }
}

/// Expand a no-arg macro: parse body_tokens as items and return
/// MemmapEntry values.
///
/// `input_tokens` is accepted for forward compatibility with real
/// `macro_rules!` matching, but Phase 1 ignores it — the current noop
/// expander just returns the literal body.
pub fn expand_macro<'db>(
    db: &'db dyn Db,
    macro_def: MacroDefItem<'db>,
    _input_tokens: &str,
) -> Vec<MemmapEntry<'db>> {
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
            "struct_item" => {
                if let Some(name_node) = child.child_by_field_name("name") {
                    let name = Name::new(db, body[name_node.byte_range()].to_owned());
                    let item = StructItem::new(
                        db,
                        name,
                        Vec::new(),
                        Vec::new(),
                        dummy_span_table(db),
                        DUMMY_SPAN,
                    );
                    entries.push(MemmapEntry::Item(Item::Struct(item)));
                }
            }
            "enum_item" => {
                if let Some(name_node) = child.child_by_field_name("name") {
                    let name = Name::new(db, body[name_node.byte_range()].to_owned());
                    let item = EnumItem::new(
                        db,
                        name,
                        Vec::new(),
                        Vec::new(),
                        dummy_span_table(db),
                        DUMMY_SPAN,
                    );
                    entries.push(MemmapEntry::Item(Item::Enum(item)));
                }
            }
            "trait_item" => {
                if let Some(name_node) = child.child_by_field_name("name") {
                    let name = Name::new(db, body[name_node.byte_range()].to_owned());
                    let item = TraitItem::new(
                        db,
                        name,
                        Vec::new(),
                        Vec::new(),
                        dummy_span_table(db),
                        DUMMY_SPAN,
                    );
                    entries.push(MemmapEntry::Item(Item::Trait(item)));
                }
            }
            "type_item" => {
                if let Some(name_node) = child.child_by_field_name("name") {
                    let name = Name::new(db, body[name_node.byte_range()].to_owned());
                    let item = TypeAliasItem::new(
                        db,
                        name,
                        Vec::new(),
                        None,
                        dummy_span_table(db),
                        DUMMY_SPAN,
                    );
                    entries.push(MemmapEntry::Item(Item::TypeAlias(item)));
                }
            }
            "mod_item" => {
                if let Some(name_node) = child.child_by_field_name("name") {
                    let name = Name::new(db, body[name_node.byte_range()].to_owned());
                    // Inline mod if the body contains a declaration list;
                    // file-based if it ends with `;`. We don't have access
                    // to the source-level distinction here, so treat as
                    // inline with empty items — good enough for Phase 1.
                    let item = ModItem::new(
                        db,
                        name,
                        Vec::new(),
                        Some(Vec::new()),
                        dummy_span_table(db),
                        DUMMY_SPAN,
                    );
                    entries.push(MemmapEntry::Item(Item::Mod(item)));
                }
            }
            "function_item" => {
                if let Some(name_node) = child.child_by_field_name("name") {
                    let name = Name::new(db, body[name_node.byte_range()].to_owned());
                    let item = FunctionItem::new(
                        db,
                        name,
                        Vec::new(),
                        Vec::new(),
                        None,
                        false,
                        false,
                        missing_function_body(),
                        dummy_span_table(db),
                        DUMMY_SPAN,
                    );
                    entries.push(MemmapEntry::Item(Item::Function(item)));
                }
            }
            "const_item" => {
                if let Some(name_node) = child.child_by_field_name("name") {
                    let name = Name::new(db, body[name_node.byte_range()].to_owned());
                    let item = ConstItem::new(
                        db,
                        name,
                        Vec::new(),
                        None,
                        dummy_span_table(db),
                        DUMMY_SPAN,
                    );
                    entries.push(MemmapEntry::Item(Item::Const(item)));
                }
            }
            "static_item" => {
                if let Some(name_node) = child.child_by_field_name("name") {
                    let name = Name::new(db, body[name_node.byte_range()].to_owned());
                    let item = StaticItem::new(
                        db,
                        name,
                        Vec::new(),
                        None,
                        false,
                        dummy_span_table(db),
                        DUMMY_SPAN,
                    );
                    entries.push(MemmapEntry::Item(Item::Static(item)));
                }
            }
            "impl_item" => {
                entries.push(MemmapEntry::Item(Item::Error(DUMMY_SPAN)));
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
                    entries.push(MemmapEntry::MacroDef(def));
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
                            let input_tokens =
                                crate::ts_helpers::extract_macro_invocation_tokens(invoc, body);
                            entries.push(MemmapEntry::MacroUse(MacroUse {
                                path,
                                input_tokens,
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
