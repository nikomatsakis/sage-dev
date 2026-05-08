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
    Ambiguous,
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
use crate::symbol::{Symbol, SymbolSource};
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
                MacroResolution::Found(macro_def) => {
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
                MacroResolution::Found(macro_def) => {
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

/// Result of resolving a macro path.
enum MacroResolution<'db> {
    Found(MacroDef<'db>),
    Ambiguous,
    NotFound,
}

impl<'db> From<Option<MacroDef<'db>>> for MacroResolution<'db> {
    fn from(opt: Option<MacroDef<'db>>) -> Self {
        match opt {
            Some(def) => MacroResolution::Found(def),
            None => MacroResolution::NotFound,
        }
    }
}

/// Resolve a macro path within the current MEM-map context.
fn resolve_memmap_path<'db>(
    db: &'db dyn Db,
    module: Module<'db>,
    source_root: SourceRoot,
    crate_root: Module<'db>,
    entries: &[MemmapEntry<'db>],
    path: Path<'db>,
) -> MacroResolution<'db> {
    let segments = path.segments(db);
    if segments.is_empty() {
        return MacroResolution::NotFound;
    }

    if segments.len() == 1 {
        // Single segment: look in current module's entries
        let name = segments[0];
        // Check named members in MacroNS (non-glob, highest priority)
        for entry in entries {
            if let MemmapEntry::Named(member) = entry {
                if member.name == name {
                    if let NamedMemberKind::MacroDef(def) = &member.kind {
                        return MacroResolution::Found(*def);
                    }
                }
            }
        }
        // Check globs — collect all candidates to detect ambiguity
        let mut glob_result: Option<MacroDef<'db>> = None;
        for entry in entries {
            if let MemmapEntry::Glob(glob) = entry {
                if let Some(def) =
                    find_macro_in_module(db, glob.source_module, name, source_root, crate_root)
                {
                    if let Some(existing) = glob_result {
                        if existing != def {
                            return MacroResolution::Ambiguous;
                        }
                    } else {
                        glob_result = Some(def);
                    }
                }
            }
        }
        return match glob_result {
            Some(def) => MacroResolution::Found(def),
            None => MacroResolution::NotFound,
        };
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
                                return MacroResolution::Found(*def);
                            }
                        }
                    }
                }
                return MacroResolution::NotFound;
            }
            // self::a::b::m — resolve from current module
            walk_path_to_macro(db, module, source_root, crate_root, &segments[1..]).into()
        }
        "crate" => {
            walk_path_to_macro(db, crate_root, source_root, crate_root, &segments[1..]).into()
        }
        "super" => {
            if let ModuleSource::Local {
                parent: Some(p), ..
            } = module.source(db)
            {
                walk_path_to_macro(db, p, source_root, crate_root, &segments[1..]).into()
            } else {
                MacroResolution::NotFound
            }
        }
        _ => {
            // Bare identifier: look in current entries (including globs) for a module
            let first = segments[0];
            // Check named members for a module with this name
            for entry in entries {
                if let MemmapEntry::Named(member) = entry {
                    if member.name == first && member.ns == Namespace::Type {
                        if let NamedMemberKind::Item(item) = &member.kind {
                            if let Some(m) = symbol_to_module(
                                db,
                                Symbol::new(db, SymbolSource::Local(*item)),
                                source_root,
                                module,
                            ) {
                                return walk_path_to_macro(
                                    db,
                                    m,
                                    source_root,
                                    crate_root,
                                    &segments[1..],
                                )
                                .into();
                            }
                        }
                    }
                }
            }
            // Check expanded subtrees for a module
            for entry in entries {
                if let MemmapEntry::MacroUse(mu) = entry {
                    if let MacroUseState::Expanded(sub) = &mu.state {
                        for sub_entry in sub {
                            if let MemmapEntry::Named(member) = sub_entry {
                                if member.name == first && member.ns == Namespace::Type {
                                    if let NamedMemberKind::Item(item) = &member.kind {
                                        if let Some(m) = symbol_to_module(
                                            db,
                                            Symbol::new(db, SymbolSource::Local(*item)),
                                            source_root,
                                            module,
                                        ) {
                                            return walk_path_to_macro(
                                                db,
                                                m,
                                                source_root,
                                                crate_root,
                                                &segments[1..],
                                            )
                                            .into();
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
            // Check globs for a module with this name
            for entry in entries {
                if let MemmapEntry::Glob(glob) = entry {
                    if matches!(glob.source_module.source(db), ModuleSource::External(..)) {
                        continue;
                    }
                    let source_memmap =
                        module_memmap(db, glob.source_module, source_root, crate_root);
                    for src_entry in source_memmap.entries(db) {
                        if let MemmapEntry::Named(member) = src_entry {
                            if member.name == first && member.ns == Namespace::Type {
                                if let NamedMemberKind::Item(item) = &member.kind {
                                    if let Some(m) = symbol_to_module(
                                        db,
                                        Symbol::new(db, SymbolSource::Local(*item)),
                                        source_root,
                                        glob.source_module,
                                    ) {
                                        return walk_path_to_macro(
                                            db,
                                            m,
                                            source_root,
                                            crate_root,
                                            &segments[1..],
                                        )
                                        .into();
                                    }
                                }
                            }
                        }
                    }
                }
            }
            // Fall through to extern prelude
            match resolve_first_segment(db, module, source_root, crate_root, segments) {
                Ok((m, rest)) => walk_path_to_macro(db, m, source_root, crate_root, rest).into(),
                Err(_) => MacroResolution::NotFound,
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
    name: Name<'db>,
    source_root: SourceRoot,
    crate_root: Module<'db>,
) -> Option<MacroDef<'db>> {
    if matches!(module.source(db), ModuleSource::External(..)) {
        return None;
    }
    let memmap = module_memmap(db, module, source_root, crate_root);
    for entry in memmap.entries(db) {
        if let MemmapEntry::Named(member) = entry {
            if member.name == name {
                if let NamedMemberKind::MacroDef(def) = &member.kind {
                    return Some(*def);
                }
            }
        }
    }
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

// ---------------------------------------------------------------------------
// Validation: memmap_errors
// ---------------------------------------------------------------------------

/// Errors detected by inspecting the converged MEM-map.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MemmapError<'db> {
    /// Two non-glob items with the same name in the same namespace.
    DuplicateName { name: Name<'db>, ns: Namespace },
    /// A macro invocation whose path never resolved after convergence.
    UnresolvedMacro { path: Path<'db> },
    /// A macro invocation that resolved to multiple candidates.
    AmbiguousMacro { path: Path<'db> },
    /// A macro expansion introduced a non-glob name that conflicts with a glob-imported name.
    TimeTravelViolation { name: Name<'db>, ns: Namespace },
}

/// Inspect the converged MEM-map and return all validation errors.
pub fn memmap_errors<'db>(
    db: &'db dyn Db,
    module: Module<'db>,
    source_root: SourceRoot,
    crate_root: Module<'db>,
) -> Vec<MemmapError<'db>> {
    let memmap = module_memmap(db, module, source_root, crate_root);
    let entries = memmap.entries(db);
    let mut errors = Vec::new();

    // Collect all named members (including from expanded subtrees) for duplicate detection
    let mut names: Vec<(Name<'db>, Namespace)> = Vec::new();
    collect_all_names(entries, &mut names);

    // Check for duplicate non-glob names in the same namespace
    for i in 0..names.len() {
        for j in (i + 1)..names.len() {
            if names[i] == names[j] {
                let err = MemmapError::DuplicateName {
                    name: names[i].0,
                    ns: names[i].1,
                };
                if !errors.contains(&err) {
                    errors.push(err);
                }
            }
        }
    }

    // Check for unresolved macros
    collect_unresolved_macros(entries, &mut errors);

    // Check for time-travel violations: macro-expanded non-glob names that
    // conflict with glob-imported names
    let mut expanded_names: Vec<(Name<'db>, Namespace)> = Vec::new();
    collect_expanded_names(entries, &mut expanded_names);
    for (name, ns) in &expanded_names {
        if name_available_via_glob(db, entries, *name, *ns, source_root, crate_root) {
            let err = MemmapError::TimeTravelViolation {
                name: *name,
                ns: *ns,
            };
            if !errors.contains(&err) {
                errors.push(err);
            }
        }
    }

    errors
}

/// Recursively collect all (name, namespace) pairs from non-glob named members.
/// Both items and redirects (named imports) are non-glob and participate in
/// duplicate detection.
fn collect_all_names<'db>(entries: &[MemmapEntry<'db>], out: &mut Vec<(Name<'db>, Namespace)>) {
    for entry in entries {
        match entry {
            MemmapEntry::Named(member) => {
                out.push((member.name, member.ns));
            }
            MemmapEntry::MacroUse(mu) => {
                if let MacroUseState::Expanded(sub) = &mu.state {
                    collect_all_names(sub, out);
                }
            }
            _ => {}
        }
    }
}

/// Recursively check for unresolved and ambiguous macros.
fn collect_unresolved_macros<'db>(
    entries: &[MemmapEntry<'db>],
    errors: &mut Vec<MemmapError<'db>>,
) {
    for entry in entries {
        if let MemmapEntry::MacroUse(mu) = entry {
            match &mu.state {
                MacroUseState::Unresolved => {
                    errors.push(MemmapError::UnresolvedMacro { path: mu.path });
                }
                MacroUseState::Ambiguous => {
                    errors.push(MemmapError::AmbiguousMacro { path: mu.path });
                }
                MacroUseState::Expanded(sub) => {
                    collect_unresolved_macros(sub, errors);
                }
                _ => {}
            }
        }
    }
}

/// Collect names that come from macro expansion subtrees only (not root-level named members).
fn collect_expanded_names<'db>(
    entries: &[MemmapEntry<'db>],
    out: &mut Vec<(Name<'db>, Namespace)>,
) {
    for entry in entries {
        if let MemmapEntry::MacroUse(mu) = entry {
            if let MacroUseState::Expanded(sub) = &mu.state {
                collect_all_names(sub, out);
            }
        }
    }
}

/// Check if a name is available via any glob stem in the entries.
fn name_available_via_glob<'db>(
    db: &'db dyn Db,
    entries: &[MemmapEntry<'db>],
    name: Name<'db>,
    ns: Namespace,
    source_root: SourceRoot,
    crate_root: Module<'db>,
) -> bool {
    for entry in entries {
        if let MemmapEntry::Glob(glob) = entry {
            if matches!(glob.source_module.source(db), ModuleSource::External(..)) {
                continue;
            }
            let source_memmap = module_memmap(db, glob.source_module, source_root, crate_root);
            for src_entry in source_memmap.entries(db) {
                if let MemmapEntry::Named(member) = src_entry {
                    if member.name == name && member.ns == ns {
                        return true;
                    }
                }
            }
        }
    }
    false
}
