//! Macro resolution and expansion within the MEM-map.
//!
//! Snapshot-based: entries are cloned before mutation so resolution
//! reads the snapshot while expansion mutates live entries.
//!
//! `expand_macro` parses body tokens with tree-sitter to synthesize
//! proper `Item` tracked structs (not `Item::Error` placeholders). The
//! inline body of an expanded `mod foo { .. }` is recursively parsed
//! so that `resolve_mod(foo)` → `LocalInline` produces a walkable
//! Module with populated items — needed for `use foo::X` etc. to
//! resolve when `foo` is macro-created.

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

/// Maximum macro expansion depth (same as rustc's default).
const MAX_EXPANSION_DEPTH: usize = 128;

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
        // Depth cap: leave anything still Unresolved as-is; validator
        // reports UnresolvedMacro. Phase 3 added explicit overflow
        // caps for finer-grained diagnostics.
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
                        // Phase 1/2 only produce Rules; Builtin/Proc in a later phase.
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

/// Expand a macro's body into `MemmapEntry` values.
///
/// `input_tokens` is accepted for forward compatibility with real
/// `macro_rules!` matching, but the current expander ignores it — the
/// body is used as the verbatim expansion.
pub fn expand_macro<'db>(
    db: &'db dyn Db,
    macro_def: MacroDefItem<'db>,
    _input_tokens: &str,
) -> Vec<MemmapEntry<'db>> {
    let body = macro_def.body_tokens(db);
    if body.is_empty() {
        return Vec::new();
    }

    parse_text_into_entries(db, body.as_str())
}

/// Parse a source-text string into `MemmapEntry`s. Re-parses via
/// tree-sitter on each call — Phase 4 will route through
/// `file_item_tree` for proper caching.
fn parse_text_into_entries<'db>(db: &'db dyn Db, text: &str) -> Vec<MemmapEntry<'db>> {
    let mut parser = tree_sitter::Parser::new();
    parser
        .set_language(&tree_sitter_rust::LANGUAGE.into())
        .expect("failed to set tree-sitter-rust language");
    let tree = match parser.parse(text, None) {
        Some(t) => t,
        None => return Vec::new(),
    };
    let root = tree.root_node();
    let mut entries = Vec::new();
    for child in root.children(&mut root.walk()) {
        if child.is_named() {
            emit_entry_from_node(db, child, text, &mut entries);
        }
    }
    entries
}

/// Emit a `MemmapEntry` for a single top-level AST node, recursing
/// into `mod` bodies.
fn emit_entry_from_node<'db>(
    db: &'db dyn Db,
    child: tree_sitter::Node<'_>,
    text: &str,
    entries: &mut Vec<MemmapEntry<'db>>,
) {
    match child.kind() {
        "struct_item" => {
            if let Some(name_node) = child.child_by_field_name("name") {
                let name = Name::new(db, text[name_node.byte_range()].to_owned());
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
                let name = Name::new(db, text[name_node.byte_range()].to_owned());
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
                let name = Name::new(db, text[name_node.byte_range()].to_owned());
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
                let name = Name::new(db, text[name_node.byte_range()].to_owned());
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
                let name = Name::new(db, text[name_node.byte_range()].to_owned());
                // Look for an inline body (`declaration_list`); if
                // present, recursively parse its items. If absent
                // (i.e. `mod foo;`), leave items as None so
                // resolve_mod can find the file.
                let inline_items = child
                    .child_by_field_name("body")
                    .filter(|n| n.kind() == "declaration_list")
                    .map(|body_node| {
                        let raw = &text[body_node.byte_range()];
                        // Strip the outer braces and recurse.
                        let inner = raw
                            .strip_prefix('{')
                            .and_then(|s| s.strip_suffix('}'))
                            .unwrap_or(raw);
                        parse_text_into_item_vec(db, inner)
                    });
                let item = ModItem::new(
                    db,
                    name,
                    Vec::new(),
                    inline_items,
                    dummy_span_table(db),
                    DUMMY_SPAN,
                );
                entries.push(MemmapEntry::Item(Item::Mod(item)));
            }
        }
        "function_item" => {
            if let Some(name_node) = child.child_by_field_name("name") {
                let name = Name::new(db, text[name_node.byte_range()].to_owned());
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
                let name = Name::new(db, text[name_node.byte_range()].to_owned());
                let item =
                    ConstItem::new(db, name, Vec::new(), None, dummy_span_table(db), DUMMY_SPAN);
                entries.push(MemmapEntry::Item(Item::Const(item)));
            }
        }
        "static_item" => {
            if let Some(name_node) = child.child_by_field_name("name") {
                let name = Name::new(db, text[name_node.byte_range()].to_owned());
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
        "use_declaration" => {
            // Parse a `use` item from expansion output into
            // Redirect / Glob entries. This is best-effort: we only
            // handle simple `use path::Name;`, `use path::Name as X;`,
            // and `use path::*;` forms.
            emit_use_declaration(db, child, text, entries);
        }
        "macro_definition" => {
            if let Some(name_node) = child.child_by_field_name("name") {
                let name = Name::new(db, text[name_node.byte_range()].to_owned());
                let span = SpanIndices {
                    start: child.start_byte() as u32,
                    end: child.end_byte() as u32,
                };
                let body_tokens = crate::ts_helpers::extract_macro_body_tokens(child, text);
                let def = MacroDefItem::new(db, name, body_tokens, span);
                entries.push(MemmapEntry::MacroDef(def));
            }
        }
        "expression_statement" => {
            if let Some(invoc) = child
                .named_children(&mut child.walk())
                .find(|c| c.kind() == "macro_invocation")
            {
                emit_macro_invocation(db, invoc, text, entries);
            }
        }
        "macro_invocation" => {
            // Item-position `m!();` appears as a direct macro_invocation
            // node under `declaration_list` (not wrapped in
            // expression_statement).
            emit_macro_invocation(db, child, text, entries);
        }
        _ => {}
    }
}

/// Build a `MemmapEntry::MacroUse` from a tree-sitter `macro_invocation`
/// node. Shared between the item-position and expression-statement
/// wrappers.
fn emit_macro_invocation<'db>(
    db: &'db dyn Db,
    invoc: tree_sitter::Node<'_>,
    text: &str,
    entries: &mut Vec<MemmapEntry<'db>>,
) {
    let Some(macro_node) = invoc.child_by_field_name("macro") else {
        return;
    };
    let segments = crate::ts_helpers::collect_macro_path_segments(db, macro_node, text);
    if segments.is_empty() {
        return;
    }
    let span = SpanIndices {
        start: invoc.start_byte() as u32,
        end: invoc.end_byte() as u32,
    };
    let path = Path::new(db, segments, span);
    let input_tokens = crate::ts_helpers::extract_macro_invocation_tokens(invoc, text);
    entries.push(MemmapEntry::MacroUse(MacroUse {
        path,
        input_tokens,
        state: MacroUseState::Unresolved,
    }));
}

/// Best-effort emission of `Redirect` / `Glob` entries from a `use` item
/// produced inside a macro expansion.
fn emit_use_declaration<'db>(
    db: &'db dyn Db,
    node: tree_sitter::Node<'_>,
    text: &str,
    entries: &mut Vec<MemmapEntry<'db>>,
) {
    let Some(arg) = node.child_by_field_name("argument") else {
        return;
    };
    emit_use_argument(db, arg, text, entries);
}

fn emit_use_argument<'db>(
    db: &'db dyn Db,
    node: tree_sitter::Node<'_>,
    text: &str,
    entries: &mut Vec<MemmapEntry<'db>>,
) {
    match node.kind() {
        "use_as_clause" => {
            let Some(path_node) = node.child_by_field_name("path") else {
                return;
            };
            let Some(alias_node) = node.child_by_field_name("alias") else {
                return;
            };
            let path = build_path(db, path_node, text);
            let alias = Name::new(db, text[alias_node.byte_range()].to_owned());
            entries.push(MemmapEntry::Redirect {
                name: alias,
                target: path,
            });
        }
        "use_wildcard" => {
            // `path::*`. The path is the first named child before `*`.
            let mut cursor = node.walk();
            let path_node = node
                .named_children(&mut cursor)
                .find(|c| c.kind() != "*")
                .and_then(|c| {
                    if matches!(
                        c.kind(),
                        "scoped_identifier" | "identifier" | "self" | "crate" | "super"
                    ) {
                        Some(c)
                    } else {
                        None
                    }
                });
            if let Some(path_node) = path_node {
                let path = build_path(db, path_node, text);
                entries.push(MemmapEntry::Glob { path });
            }
        }
        "scoped_identifier" | "identifier" => {
            // `use foo::bar;` — bar is the last segment, path is the full thing.
            let path = build_path(db, node, text);
            let segments = path.segments(db);
            if let Some(last) = segments.last().copied() {
                entries.push(MemmapEntry::Redirect {
                    name: last,
                    target: path,
                });
            }
        }
        "use_list" | "scoped_use_list" => {
            // Best-effort: skip — rare in expansion output; handled
            // by full file_item_tree in Phase 4.
        }
        _ => {}
    }
}

fn build_path<'db>(db: &'db dyn Db, node: tree_sitter::Node<'_>, text: &str) -> Path<'db> {
    let segments = crate::ts_helpers::collect_macro_path_segments(db, node, text);
    let span = SpanIndices {
        start: node.start_byte() as u32,
        end: node.end_byte() as u32,
    };
    Path::new(db, segments, span)
}

/// Parse a text buffer into a `Vec<Item>` — used to populate the items
/// field of an inline `mod` produced by macro expansion.
fn parse_text_into_item_vec<'db>(db: &'db dyn Db, text: &str) -> Vec<Item<'db>> {
    // Convert each parsed MemmapEntry back to its backing Item (where
    // applicable). Entries that aren't Items (Redirect, Glob, MacroDef,
    // MacroUse) are turned into their nearest Item equivalent so the
    // ModItem's items field stays schema-compatible with file_item_tree.
    //
    // This is coarse; Phase 4 will route mod-body parsing through
    // file_item_tree for full fidelity.
    let entries = parse_text_into_entries(db, text);
    let mut items = Vec::new();
    for entry in entries {
        match entry {
            MemmapEntry::Item(i) => items.push(i),
            MemmapEntry::MacroDef(def) => items.push(Item::MacroDef(def)),
            MemmapEntry::MacroUse(mu) => {
                // Build a MacroInvocationItem carrying the same path+tokens
                // so the outer memmap seeder can pick it up again.
                let inv =
                    crate::item::MacroInvocationItem::new(db, mu.path, mu.input_tokens, DUMMY_SPAN);
                items.push(Item::MacroInvocation(inv));
            }
            // Redirects/Globs inside an inline mod need to become
            // UseGroup items — which is plumbing we punt to Phase 4.
            // They're uncommon in practice for macro-expanded inline
            // modules.
            MemmapEntry::Redirect { .. } | MemmapEntry::Glob { .. } => {}
        }
    }
    items
}
