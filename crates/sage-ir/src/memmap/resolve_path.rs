//! Construction-time macro path resolution.
//!
//! Resolves a macro invocation path to a set of candidate callees
//! against a snapshot of the enclosing module's MEM-map entries.
//!
//!   * Single-segment: walk entries for `MacroDef`/`MacroUse::Expanded`
//!     matches (named), then globs (dynamically resolved to modules and
//!     searched). If any named matches exist, globs are ignored.
//!   * Multi-segment: dispatch on leading segment (`self`/`crate`/
//!     `super`/bare), then walk the remaining segments via
//!     `module_memmap` / `definition`.

use crate::Db;
use crate::item::{Item, MacroDefItem};
use crate::module::{Module, ModuleSource};
use crate::name::Name;
use crate::resolve::{SourceRoot, definition, resolve_first_segment, symbol_to_module};
use crate::symbol::{Symbol, SymbolSource};
use crate::types::Path;

use super::data::*;
use super::module_memmap;

/// Walk `path` from `current_module` to a module it names, using
/// `definition` (items-based) for each segment rather than the
/// MEM-map.
///
/// This is the **construction-time** path walker — safe to call from
/// inside `module_memmap` because it never re-enters `module_memmap`
/// on the current module. Trades off: it can't see names introduced
/// by macro expansion at the current module (they aren't in
/// `file_item_tree`), only in already-constructed target modules.
///
/// Used by `resolve_macro_path` when resolving glob-target modules
/// during macro-path resolution. Callers in non-ctime contexts should
/// use `Module::resolve_path_to_module` instead.
fn memmap_resolve_path_to_module<'db>(
    db: &'db dyn Db,
    current_module: Module<'db>,
    source_root: SourceRoot,
    path: Path<'db>,
) -> Option<Module<'db>> {
    let segments = path.segments(db);
    if segments.is_empty() {
        return None;
    }

    let (first_module, rest) =
        resolve_first_segment(db, current_module, source_root, segments).ok()?;

    let mut current = first_module;
    for seg in rest {
        let sym = definition(db, current, *seg)?;
        current = symbol_to_module(db, sym, source_root, current)?;
    }
    Some(current)
}

/// Resolve a macro path within the current MEM-map context.
pub(super) fn resolve_macro_path<'db>(
    db: &'db dyn Db,
    module: Module<'db>,
    source_root: SourceRoot,
    entries: &[MemmapEntry<'db>],
    path: Path<'db>,
) -> Vec<MacroCallee<'db>> {
    let defs = resolve_macro_path_to_defs(db, module, source_root, entries, path);
    defs.into_iter().map(MacroCallee::Rules).collect()
}

fn resolve_macro_path_to_defs<'db>(
    db: &'db dyn Db,
    module: Module<'db>,
    source_root: SourceRoot,
    entries: &[MemmapEntry<'db>],
    path: Path<'db>,
) -> Vec<MacroDefItem<'db>> {
    let segments = path.segments(db);
    if segments.is_empty() {
        return Vec::new();
    }

    if segments.len() == 1 {
        let name = segments[0];
        let mut named: Vec<MacroDefItem<'db>> = Vec::new();
        collect_named_macro_defs(db, entries, name, &mut named);
        if !named.is_empty() {
            return dedup(named);
        }

        // Glob fallback.
        let mut globbed: Vec<MacroDefItem<'db>> = Vec::new();
        for entry in entries {
            if let MemmapEntry::Glob { path } = entry {
                if let Some(target) = memmap_resolve_path_to_module(db, module, source_root, *path)
                {
                    if let Some(def) = find_macro_in_module(db, target, name, source_root) {
                        globbed.push(def);
                    }
                }
            }
        }
        if !globbed.is_empty() {
            return dedup(globbed);
        }

        // Textual-scope fallback: for LocalInline child modules,
        // macros declared in an ancestor's memmap are still visible
        // (mirroring rustc's `macro_rules!` scoping rule).
        let mut current = module;
        loop {
            let parent = match current.source(db) {
                ModuleSource::LocalInline { parent, .. } => parent,
                _ => break,
            };
            if let Some(def) = find_macro_in_module(db, parent, name, source_root) {
                return vec![def];
            }
            current = parent;
        }
        return Vec::new();
    }

    // Multi-segment: dispatch on leading segment.
    let first_text = segments[0].text(db);
    let rest = &segments[1..];
    match first_text.as_str() {
        "self" => {
            if rest.len() == 1 {
                let name = rest[0];
                let mut named: Vec<MacroDefItem<'db>> = Vec::new();
                collect_named_macro_defs(db, entries, name, &mut named);
                return dedup(named);
            }
            walk_path_to_macro(db, module, source_root, rest)
                .into_iter()
                .collect()
        }
        "crate" => walk_path_to_macro(db, module.crate_root(db), source_root, rest)
            .into_iter()
            .collect(),
        "super" => match module.parent(db) {
            Some(parent) => walk_path_to_macro(db, parent, source_root, rest)
                .into_iter()
                .collect(),
            None => Vec::new(),
        },
        _ => {
            let first = segments[0];

            // 1. Local items in this module's snapshot.
            for entry in entries {
                if let MemmapEntry::Item(Item::Mod(_)) = entry {
                    if let Some(m) = item_as_child_module(db, entry, first, source_root, module) {
                        let mut defs = Vec::new();
                        if let Some(def) = walk_path_to_macro(db, m, source_root, rest) {
                            defs.push(def);
                        }
                        return defs;
                    }
                }
            }

            // 2. Look inside expansion subtrees.
            for entry in entries {
                if let MemmapEntry::MacroUse(mu) = entry {
                    if let MacroUseState::Expanded(exps) = &mu.state {
                        for exp in exps {
                            for sub_entry in &exp.entries {
                                if let MemmapEntry::Item(Item::Mod(_)) = sub_entry {
                                    if let Some(m) = item_as_child_module(
                                        db,
                                        sub_entry,
                                        first,
                                        source_root,
                                        module,
                                    ) {
                                        let mut defs = Vec::new();
                                        if let Some(def) =
                                            walk_path_to_macro(db, m, source_root, rest)
                                        {
                                            defs.push(def);
                                        }
                                        return defs;
                                    }
                                }
                            }
                        }
                    }
                }
            }

            // 3. Globs — first segment may come from one.
            for entry in entries {
                if let MemmapEntry::Glob { path: glob_path } = entry {
                    let Some(glob_target) =
                        memmap_resolve_path_to_module(db, module, source_root, *glob_path)
                    else {
                        continue;
                    };
                    if matches!(glob_target.source(db), ModuleSource::External(..)) {
                        continue;
                    }
                    let source_memmap = module_memmap(db, glob_target, source_root);
                    for src_entry in source_memmap.entries(db) {
                        if let MemmapEntry::Item(Item::Mod(_)) = src_entry {
                            if let Some(m) =
                                item_as_child_module(db, src_entry, first, source_root, glob_target)
                            {
                                let mut defs = Vec::new();
                                if let Some(def) = walk_path_to_macro(db, m, source_root, rest) {
                                    defs.push(def);
                                }
                                return defs;
                            }
                        }
                    }
                }
            }

            // 4. Fall back to the generic first-segment resolver (extern prelude, etc.).
            match resolve_first_segment(db, module, source_root, segments) {
                Ok((m, rest)) => walk_path_to_macro(db, m, source_root, rest)
                    .into_iter()
                    .collect(),
                Err(_) => Vec::new(),
            }
        }
    }
}

/// Collect `MacroDef` entries with a matching name from `entries`,
/// descending through every `MacroUse::Expanded` branch.
fn collect_named_macro_defs<'db>(
    db: &'db dyn Db,
    entries: &[MemmapEntry<'db>],
    name: Name<'db>,
    out: &mut Vec<MacroDefItem<'db>>,
) {
    for entry in entries {
        match entry {
            MemmapEntry::MacroDef(def) => {
                if def.name(db) == name {
                    out.push(*def);
                }
            }
            MemmapEntry::MacroUse(mu) => {
                if let MacroUseState::Expanded(exps) = &mu.state {
                    for exp in exps {
                        collect_named_macro_defs(db, &exp.entries, name, out);
                    }
                }
            }
            _ => {}
        }
    }
}

/// Walk a path from a module to find a MacroDefItem at the end.
fn walk_path_to_macro<'db>(
    db: &'db dyn Db,
    mut current: Module<'db>,
    source_root: SourceRoot,
    segments: &[Name<'db>],
) -> Option<MacroDefItem<'db>> {
    if segments.is_empty() {
        return None;
    }

    for (i, seg) in segments.iter().enumerate() {
        if i < segments.len() - 1 {
            let sym = definition(db, current, *seg)?;
            current = symbol_to_module(db, sym, source_root, current)?;
        } else {
            let target_memmap = module_memmap(db, current, source_root);
            for entry in target_memmap.entries(db) {
                match entry {
                    MemmapEntry::MacroDef(def) => {
                        if def.name(db) == *seg {
                            return Some(*def);
                        }
                    }
                    MemmapEntry::Redirect { name, target } => {
                        if *name == *seg {
                            return resolve_redirect_to_macro(db, current, source_root, *target);
                        }
                    }
                    _ => {}
                }
            }
            return None;
        }
    }
    None
}

/// Resolve a use-redirect path to a MacroDefItem.
fn resolve_redirect_to_macro<'db>(
    db: &'db dyn Db,
    current_module: Module<'db>,
    source_root: SourceRoot,
    path: Path<'db>,
) -> Option<MacroDefItem<'db>> {
    let segments = path.segments(db);
    if segments.is_empty() {
        return None;
    }
    let (first_module, rest) =
        resolve_first_segment(db, current_module, source_root, segments).ok()?;
    walk_path_to_macro(db, first_module, source_root, rest)
}

/// Find a MacroDefItem by name in a module's memmap (for glob lookup).
pub(super) fn find_macro_in_module<'db>(
    db: &'db dyn Db,
    module: Module<'db>,
    name: Name<'db>,
    source_root: SourceRoot,
) -> Option<MacroDefItem<'db>> {
    if matches!(module.source(db), ModuleSource::External(..)) {
        return None;
    }
    let memmap = module_memmap(db, module, source_root);
    for entry in memmap.entries(db) {
        if let MemmapEntry::MacroDef(def) = entry {
            if def.name(db) == name {
                return Some(*def);
            }
        }
    }
    None
}

/// Helper: interpret a `MemmapEntry::Item(Item::Mod(..))` as a child
/// module if its name matches the supplied first segment.
fn item_as_child_module<'db>(
    db: &'db dyn Db,
    entry: &MemmapEntry<'db>,
    first: Name<'db>,
    source_root: SourceRoot,
    parent: Module<'db>,
) -> Option<Module<'db>> {
    let MemmapEntry::Item(item) = entry else {
        return None;
    };
    let Item::Mod(mod_item) = item else {
        return None;
    };
    if mod_item.name(db) != first {
        return None;
    }
    let sym = Symbol::new(db, SymbolSource::Local(*item));
    symbol_to_module(db, sym, source_root, parent)
}

/// Tiny set-dedup helper — preserves insertion order.
fn dedup<'db>(mut defs: Vec<MacroDefItem<'db>>) -> Vec<MacroDefItem<'db>> {
    let mut out: Vec<MacroDefItem<'db>> = Vec::new();
    defs.retain(|def| {
        if out.contains(def) {
            false
        } else {
            out.push(*def);
            true
        }
    });
    out
}
