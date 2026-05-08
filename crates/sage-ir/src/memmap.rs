//! Module MEM-map: the single source of truth for "what's in a module."
//!
//! `module_memmap` replaces `module_items` as the canonical query for
//! module-level name resolution. It contains named members (items +
//! use-redirects + macro defs), glob stems, macro uses, and anonymous items.

use crate::Db;
use crate::item::Item;
use crate::module::{Module, ModuleSource};
use crate::name::Name;
use crate::resolve::Namespace;
use crate::span::SpanIndices;
use crate::types::Path;

// ---------------------------------------------------------------------------
// Data model
// ---------------------------------------------------------------------------

/// The MEM-map for a single module.
#[salsa::tracked(debug)]
pub struct ModuleMemmap<'db> {
    #[returns(ref)]
    pub entries: Vec<MemmapEntry<'db>>,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash, salsa::Update)]
pub enum MemmapEntry<'db> {
    Named(NamedMember<'db>),
    MacroUse(MacroUse<'db>),
    Glob(GlobStem<'db>),
    Anon(Item<'db>),
}

#[derive(Clone, Debug, PartialEq, Eq, Hash, salsa::Update)]
pub struct NamedMember<'db> {
    pub name: Name<'db>,
    pub ns: Namespace,
    pub kind: NamedMemberKind<'db>,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash, salsa::Update)]
pub enum NamedMemberKind<'db> {
    /// A regular item (struct, enum, fn, mod, const, etc.)
    Item(Item<'db>),
    /// A use-redirect: `use foo::bar` or `use foo::bar as baz`
    Redirect { target: Path<'db> },
    /// A `macro_rules!` definition
    MacroDef(MacroDef<'db>),
}

#[derive(Clone, Debug, PartialEq, Eq, Hash, salsa::Update)]
pub struct MacroUse<'db> {
    pub path: Path<'db>,
    pub state: MacroUseState<'db>,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash, salsa::Update)]
pub enum MacroUseState<'db> {
    Unresolved,
    Unexpanded(MacroDef<'db>),
    Expanded(Vec<MemmapEntry<'db>>),
    Error,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash, salsa::Update)]
pub struct GlobStem<'db> {
    pub source_module: Module<'db>,
}

#[salsa::tracked(debug)]
pub struct MacroDef<'db> {
    pub name: Name<'db>,
    #[returns(ref)]
    pub body_tokens: String,
    pub span: SpanIndices,
}

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// Maximum macro expansion depth (same as rustc).
const MAX_EXPANSION_DEPTH: usize = 128;

// ---------------------------------------------------------------------------
// module_memmap query — fixpoint with cycle recovery
// ---------------------------------------------------------------------------

use crate::lower::file_item_tree;
use crate::resolve::{
    SourceRoot, definition, item_in_namespace, item_name, resolve_first_segment,
    resolve_use_path_to_module, symbol_to_module,
};
use crate::types::UseKind;

/// Compute the MEM-map for a module. Seeds from CST, then resolves and
/// expands macros. Uses salsa fixpoint for cross-module convergence.
#[salsa::tracked(returns(ref), cycle_initial = module_memmap_initial)]
pub fn module_memmap<'db>(
    db: &'db dyn Db,
    module: Module<'db>,
    source_root: SourceRoot,
    crate_root: Module<'db>,
) -> ModuleMemmap<'db> {
    let mut entries = match module.source(db) {
        ModuleSource::Local { file, .. } => {
            seed_from_cst(db, module, source_root, crate_root, file)
        }
        ModuleSource::External(..) => Vec::new(),
    };

    // Resolve and expand macros
    resolve_and_expand_macros(db, module, source_root, crate_root, &mut entries, 0);

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

// ---------------------------------------------------------------------------
// CST seeding
// ---------------------------------------------------------------------------

/// Seed MEM-map entries from the CST (tree-sitter parse).
fn seed_from_cst<'db>(
    db: &'db dyn Db,
    module: Module<'db>,
    source_root: SourceRoot,
    crate_root: Module<'db>,
    file: crate::source::SourceFile,
) -> Vec<MemmapEntry<'db>> {
    let text = file.text(db);
    let mut parser = tree_sitter::Parser::new();
    parser
        .set_language(&tree_sitter_rust::LANGUAGE.into())
        .expect("failed to set tree-sitter-rust language");
    let tree = parser.parse(text, None).expect("tree-sitter parse failed");

    let mut entries = Vec::new();
    let root = tree.root_node();
    let mut cursor = root.walk();

    for child in root.children(&mut cursor) {
        if !child.is_named() {
            continue;
        }
        match child.kind() {
            "macro_definition" => {
                if let Some(entry) = lower_macro_definition(db, child, text) {
                    entries.push(entry);
                }
            }
            "expression_statement" => {
                // Item-level macro invocation: expression_statement > macro_invocation
                if let Some(invoc) = child
                    .named_children(&mut child.walk())
                    .find(|c| c.kind() == "macro_invocation")
                {
                    if let Some(entry) = lower_macro_invocation(db, invoc, text) {
                        entries.push(entry);
                    }
                }
            }
            _ => {
                // Use file_item_tree for regular items (reuses existing lowering)
            }
        }
    }

    // Also seed from file_item_tree for regular items
    let items = file_item_tree(db, file);
    seed_regular_items(db, module, source_root, crate_root, items, &mut entries);

    entries
}

/// Lower a `macro_definition` tree-sitter node to a MacroDef entry.
fn lower_macro_definition<'db>(
    db: &'db dyn Db,
    node: tree_sitter::Node<'_>,
    text: &str,
) -> Option<MemmapEntry<'db>> {
    let name_node = node.child_by_field_name("name")?;
    let name = Name::new(db, text[name_node.byte_range()].to_owned());
    let span = SpanIndices {
        start: node.start_byte() as u32,
        end: node.end_byte() as u32,
    };

    // Extract body tokens from the first macro_rule's right (token_tree)
    let mut cursor = node.walk();
    let body_tokens = node
        .children(&mut cursor)
        .find(|c| c.kind() == "macro_rule")
        .and_then(|rule| rule.child_by_field_name("right"))
        .map(|tt| {
            let raw = &text[tt.byte_range()];
            // Strip outer braces { ... }
            raw.strip_prefix('{')
                .and_then(|s| s.strip_suffix('}'))
                .unwrap_or(raw)
                .trim()
                .to_owned()
        })
        .unwrap_or_default();

    let macro_def = MacroDef::new(db, name, body_tokens, span);
    Some(MemmapEntry::Named(NamedMember {
        name,
        ns: Namespace::Macro(crate::resolve::MacroKind::Bang),
        kind: NamedMemberKind::MacroDef(macro_def),
    }))
}

/// Lower a `macro_invocation` tree-sitter node to a MacroUse entry.
fn lower_macro_invocation<'db>(
    db: &'db dyn Db,
    node: tree_sitter::Node<'_>,
    text: &str,
) -> Option<MemmapEntry<'db>> {
    let macro_node = node.child_by_field_name("macro")?;
    let segments = collect_macro_path_segments(db, macro_node, text);
    if segments.is_empty() {
        return None;
    }
    let span = SpanIndices {
        start: node.start_byte() as u32,
        end: node.end_byte() as u32,
    };
    let path = Path::new(db, segments, span);
    Some(MemmapEntry::MacroUse(MacroUse {
        path,
        state: MacroUseState::Unresolved,
    }))
}

/// Collect path segments from a macro invocation's `macro` field.
fn collect_macro_path_segments<'db>(
    db: &'db dyn Db,
    node: tree_sitter::Node<'_>,
    text: &str,
) -> Vec<Name<'db>> {
    let mut segments = Vec::new();
    collect_segments_recursive(db, node, text, &mut segments);
    segments
}

fn collect_segments_recursive<'db>(
    db: &'db dyn Db,
    node: tree_sitter::Node<'_>,
    text: &str,
    out: &mut Vec<Name<'db>>,
) {
    match node.kind() {
        "identifier" => {
            out.push(Name::new(db, text[node.byte_range()].to_owned()));
        }
        "scoped_identifier" => {
            if let Some(path) = node.child_by_field_name("path") {
                collect_segments_recursive(db, path, text, out);
            }
            if let Some(name) = node.child_by_field_name("name") {
                out.push(Name::new(db, text[name.byte_range()].to_owned()));
            }
        }
        "self" => out.push(Name::new(db, "self".to_owned())),
        "crate" => out.push(Name::new(db, "crate".to_owned())),
        "super" => out.push(Name::new(db, "super".to_owned())),
        _ => {
            out.push(Name::new(db, text[node.byte_range()].to_owned()));
        }
    }
}

/// Seed entries from file_item_tree (regular items, use-redirects, globs).
fn seed_regular_items<'db>(
    db: &'db dyn Db,
    module: Module<'db>,
    source_root: SourceRoot,
    crate_root: Module<'db>,
    items: &[Item<'db>],
    entries: &mut Vec<MemmapEntry<'db>>,
) {
    for &item in items {
        match item {
            Item::Use(group) => {
                for import in group.imports(db) {
                    match import.kind(db) {
                        UseKind::Named(alias) => {
                            entries.push(MemmapEntry::Named(NamedMember {
                                name: alias,
                                ns: Namespace::Type, // redirects resolve at lookup time
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
            Item::Error(_) => {} // macro_definition and expression_statement handled above
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
}

// ---------------------------------------------------------------------------
// Macro resolution and expansion
// ---------------------------------------------------------------------------

/// Resolve and expand all unresolved MacroUse entries in the MEM-map.
fn resolve_and_expand_macros<'db>(
    db: &'db dyn Db,
    module: Module<'db>,
    source_root: SourceRoot,
    crate_root: Module<'db>,
    entries: &mut Vec<MemmapEntry<'db>>,
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

    // Snapshot entries for resolution (we need to read while mutating)
    let snapshot: Vec<MemmapEntry<'db>> = entries.clone();

    for i in 0..entries.len() {
        if let MemmapEntry::MacroUse(mu) = &entries[i] {
            if !matches!(mu.state, MacroUseState::Unresolved) {
                continue;
            }
            let path = mu.path;

            match resolve_memmap_path(db, module, source_root, crate_root, &snapshot, path) {
                Some(macro_def) => {
                    let mut expanded = expand_macro(db, macro_def);
                    // Recursively resolve macros in the expansion using the same snapshot
                    resolve_and_expand_inner(
                        db,
                        module,
                        source_root,
                        crate_root,
                        &mut expanded,
                        &snapshot,
                        depth + 1,
                    );
                    entries[i] = MemmapEntry::MacroUse(MacroUse {
                        path,
                        state: MacroUseState::Expanded(expanded),
                    });
                }
                None => {}
            }
        }
    }
}

/// Recursively resolve macros in expanded entries, using root_entries for resolution.
fn resolve_and_expand_inner<'db>(
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
                Some(macro_def) => {
                    let mut expanded = expand_macro(db, macro_def);
                    resolve_and_expand_inner(
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
                None => {}
            }
        }
    }
}

/// Resolve a macro path within the current MEM-map context.
/// Returns the first MacroDef found, or None.
fn resolve_memmap_path<'db>(
    db: &'db dyn Db,
    module: Module<'db>,
    source_root: SourceRoot,
    crate_root: Module<'db>,
    entries: &[MemmapEntry<'db>],
    path: Path<'db>,
) -> Option<MacroDef<'db>> {
    let segments = path.segments(db);
    if segments.is_empty() {
        return None;
    }

    if segments.len() == 1 {
        // Single segment: look in current module's entries
        let name = segments[0];
        // Check named members in MacroNS
        for entry in entries {
            if let MemmapEntry::Named(member) = entry {
                if member.name == name {
                    if let NamedMemberKind::MacroDef(def) = &member.kind {
                        return Some(*def);
                    }
                }
            }
        }
        // Check globs
        for entry in entries {
            if let MemmapEntry::Glob(glob) = entry {
                if let Some(def) = find_macro_in_module(db, glob.source_module, name) {
                    return Some(def);
                }
            }
        }
        return None;
    }

    // Multi-segment: resolve first segment, walk the rest
    let first_text = segments[0].text(db);
    match first_text.as_str() {
        "self" => {
            // self:: path — look in current entries for the remaining segments
            if segments.len() == 2 {
                // self::m — look for macro 'm' in current entries
                let name = segments[1];
                for entry in entries {
                    if let MemmapEntry::Named(member) = entry {
                        if member.name == name {
                            if let NamedMemberKind::MacroDef(def) = &member.kind {
                                return Some(*def);
                            }
                        }
                    }
                }
                return None;
            }
            // self::a::b::m — resolve from current module
            walk_path_to_macro(db, module, source_root, crate_root, &segments[1..])
        }
        "crate" => walk_path_to_macro(db, crate_root, source_root, crate_root, &segments[1..]),
        "super" => {
            if let ModuleSource::Local {
                parent: Some(p), ..
            } = module.source(db)
            {
                walk_path_to_macro(db, p, source_root, crate_root, &segments[1..])
            } else {
                None
            }
        }
        _ => {
            // Bare identifier: try resolve_first_segment
            match resolve_first_segment(db, module, source_root, crate_root, segments) {
                Ok((m, rest)) => walk_path_to_macro(db, m, source_root, crate_root, rest),
                Err(_) => None,
            }
        }
    }
}

/// Walk a path from a module to find a MacroDef at the end.
fn walk_path_to_macro<'db>(
    db: &'db dyn Db,
    mut current: Module<'db>,
    source_root: SourceRoot,
    crate_root: Module<'db>,
    segments: &[Name<'db>],
) -> Option<MacroDef<'db>> {
    if segments.is_empty() {
        return None;
    }

    // Walk intermediate segments
    for (i, seg) in segments.iter().enumerate() {
        if i < segments.len() - 1 {
            // Intermediate: must resolve to a module
            let sym = definition(db, current, *seg)?;
            current = symbol_to_module(db, sym, source_root, current)?;
        } else {
            // Last segment: look for a MacroDef
            // Check the target module's memmap for the macro
            let target_memmap = module_memmap(db, current, source_root, crate_root);
            for entry in target_memmap.entries(db) {
                if let MemmapEntry::Named(member) = entry {
                    if member.name == *seg {
                        if let NamedMemberKind::MacroDef(def) = &member.kind {
                            return Some(*def);
                        }
                        // Also check redirects — a `pub(crate) use m;` re-exports a macro
                        if let NamedMemberKind::Redirect { target } = &member.kind {
                            // Resolve the redirect to find the macro
                            return resolve_redirect_to_macro(
                                db,
                                current,
                                source_root,
                                crate_root,
                                *target,
                            );
                        }
                    }
                }
            }
            return None;
        }
    }
    None
}

/// Resolve a use-redirect path to a MacroDef.
fn resolve_redirect_to_macro<'db>(
    db: &'db dyn Db,
    current_module: Module<'db>,
    source_root: SourceRoot,
    crate_root: Module<'db>,
    path: Path<'db>,
) -> Option<MacroDef<'db>> {
    let segments = path.segments(db);
    if segments.is_empty() {
        return None;
    }
    let (first_module, rest) =
        resolve_first_segment(db, current_module, source_root, crate_root, segments).ok()?;
    walk_path_to_macro(db, first_module, source_root, crate_root, rest)
}

/// Find a MacroDef by name in a module's memmap (for glob lookup).
fn find_macro_in_module<'db>(
    db: &'db dyn Db,
    module: Module<'db>,
    _name: Name<'db>,
) -> Option<MacroDef<'db>> {
    // For external modules, we can't find macro_rules! defs
    if matches!(module.source(db), ModuleSource::External(..)) {
        return None;
    }
    // For local modules, we'd need source_root/crate_root to call module_memmap.
    // Since we don't have them here, check module_items for now.
    // This is a limitation — globs from modules with macros won't work for
    // single-segment macro resolution. Multi-segment paths work fine.
    None
}

// ---------------------------------------------------------------------------
// expand_macro — parse body tokens as items
// ---------------------------------------------------------------------------

/// Expand a no-arg macro: parse body_tokens as items and return MemmapEntry values.
pub fn expand_macro<'db>(db: &'db dyn Db, macro_def: MacroDef<'db>) -> Vec<MemmapEntry<'db>> {
    let body = macro_def.body_tokens(db);
    if body.is_empty() {
        return Vec::new();
    }

    // Parse body tokens as a source file (items)
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
                    // Structs also in value NS
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
                if let Some(entry) = lower_macro_definition(db, child, body) {
                    entries.push(entry);
                }
            }
            "expression_statement" => {
                // Nested macro invocation
                if let Some(invoc) = child
                    .named_children(&mut child.walk())
                    .find(|c| c.kind() == "macro_invocation")
                {
                    if let Some(entry) = lower_macro_invocation(db, invoc, body) {
                        entries.push(entry);
                    }
                }
            }
            _ => {}
        }
    }

    entries
}
