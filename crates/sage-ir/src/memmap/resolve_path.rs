//! Construction-time macro path resolution.
//!
//! Resolves a macro invocation path to a set of candidate callees
//! against a snapshot of the enclosing module's MEM-map entries.

use sage_stash::{Slice, Stash};

use crate::Db;
use crate::item::{ItemAst, MacroDefAst};
use crate::module::{ModSymbol, ModSymbolData};
use crate::name::Name;
use crate::resolve::{ResolutionError, SourceRoot, definition, symbol_to_module};
use crate::symbol::Symbol;

use super::data::*;
use super::expanded_module;

fn memmap_first_segment<'db, 's>(
    db: &'db dyn Db,
    current_module: ModSymbol<'db>,
    source_root: SourceRoot,
    segments: &'s [Name<'db>],
) -> Result<(ModSymbol<'db>, &'s [Name<'db>]), ResolutionError> {
    let first = segments[0];
    let first_text = first.text(db);
    let rest = &segments[1..];

    match first_text.as_str() {
        "" => {
            if rest.is_empty() {
                return Err(ResolutionError::Unresolved);
            }
            let crate_name = rest[0].text(db);
            if let Some(crate_num) = db.tcx().extern_crate(crate_name) {
                let ext_mod = ModSymbol::external(crate_num, crate::module::DefIndex(0));
                return Ok((ext_mod, &rest[1..]));
            }
            Err(ResolutionError::Unresolved)
        }
        "crate" => Ok((current_module.crate_root(db), rest)),
        "self" => Ok((current_module, rest)),
        "super" => current_module
            .parent(db)
            .map(|p| (p, rest))
            .ok_or(ResolutionError::Unresolved),
        _ => {
            if let Some(sym) = definition(db, current_module, first, source_root) {
                if let Some(child_mod) = symbol_to_module(db, sym, source_root, current_module) {
                    return Ok((child_mod, rest));
                }
            }
            if let Some(crate_num) = db.tcx().extern_crate(first_text) {
                let ext_mod = ModSymbol::external(crate_num, crate::module::DefIndex(0));
                return Ok((ext_mod, rest));
            }
            Err(ResolutionError::Unresolved)
        }
    }
}

fn memmap_resolve_path_to_module<'db>(
    db: &'db dyn Db,
    current_module: ModSymbol<'db>,
    source_root: SourceRoot,
    segments: &[Name<'db>],
) -> Option<ModSymbol<'db>> {
    if segments.is_empty() {
        return None;
    }

    let (first_module, rest) =
        memmap_first_segment(db, current_module, source_root, segments).ok()?;

    let mut current = first_module;
    for seg in rest {
        let sym = definition(db, current, *seg, source_root)?;
        current = symbol_to_module(db, sym, source_root, current)?;
    }
    Some(current)
}

/// Resolve a macro path within the current MEM-map context.
pub(super) fn resolve_macro_path<'db>(
    db: &'db dyn Db,
    module: ModSymbol<'db>,
    source_root: SourceRoot,
    stash: &Stash,
    entries: Slice<MemmapEntry<'db>>,
    path: &[Name<'db>],
) -> Vec<MacroCallee<'db>> {
    let defs = resolve_macro_path_to_defs(db, module, source_root, stash, entries, path);
    defs.into_iter().map(MacroCallee::Rules).collect()
}

fn resolve_macro_path_to_defs<'db>(
    db: &'db dyn Db,
    module: ModSymbol<'db>,
    source_root: SourceRoot,
    stash: &Stash,
    entries: Slice<MemmapEntry<'db>>,
    segments: &[Name<'db>],
) -> Vec<MacroDefAst<'db>> {
    if segments.is_empty() {
        return Vec::new();
    }

    if segments.len() == 1 {
        let name = segments[0];
        let mut named: Vec<MacroDefAst<'db>> = Vec::new();
        collect_named_macro_defs(db, stash, entries, name, &mut named);
        if !named.is_empty() {
            return dedup(named);
        }

        // Glob fallback.
        let mut globbed: Vec<MacroDefAst<'db>> = Vec::new();
        for entry in &stash[entries] {
            if let MemmapEntry::Glob { path } = entry {
                let path_vec: Vec<_> = stash[*path].to_vec();
                if let Some(target) =
                    memmap_resolve_path_to_module(db, module, source_root, &path_vec)
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

        // Textual-scope fallback.
        let mut current = module;
        loop {
            let is_inline = matches!(
                current.data(),
                ModSymbolData::Ast(a) if a.inline_unexpanded_items(db).is_some()
            );
            if !is_inline {
                break;
            }
            let Some(parent) = current.parent(db) else {
                break;
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
                let mut named: Vec<MacroDefAst<'db>> = Vec::new();
                collect_named_macro_defs(db, stash, entries, name, &mut named);
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
            for entry in &stash[entries] {
                if let MemmapEntry::Item(ItemAst::Mod(_)) = entry {
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
            for entry in &stash[entries] {
                if let MemmapEntry::MacroUse(mu) = entry {
                    for exp in &stash[mu.expansions] {
                        for sub_entry in &stash[exp.entries] {
                            if let MemmapEntry::Item(ItemAst::Mod(_)) = sub_entry {
                                if let Some(m) =
                                    item_as_child_module(db, sub_entry, first, source_root, module)
                                {
                                    let mut defs = Vec::new();
                                    if let Some(def) = walk_path_to_macro(db, m, source_root, rest)
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

            // 3. Globs.
            for entry in &stash[entries] {
                if let MemmapEntry::Glob { path: glob_path } = entry {
                    let glob_path_vec: Vec<_> = stash[*glob_path].to_vec();
                    let Some(glob_target) =
                        memmap_resolve_path_to_module(db, module, source_root, &glob_path_vec)
                    else {
                        continue;
                    };
                    let glob_ast = match glob_target.data() {
                        ModSymbolData::Ast(a) => a,
                        ModSymbolData::Ext(_) => continue,
                    };
                    let source_memmap = expanded_module(db, glob_ast, source_root);
                    let src_stash = source_memmap.stash(db);
                    let src_entries = source_memmap.entries(db);
                    for src_entry in &src_stash[src_entries] {
                        if let MemmapEntry::Item(ItemAst::Mod(_)) = src_entry {
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

            // 4. Fall back to the generic first-segment resolver.
            match memmap_first_segment(db, module, source_root, segments) {
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
    stash: &Stash,
    entries: Slice<MemmapEntry<'db>>,
    name: Name<'db>,
    out: &mut Vec<MacroDefAst<'db>>,
) {
    for entry in &stash[entries] {
        match entry {
            MemmapEntry::MacroDef(def) => {
                if def.name(db) == name {
                    out.push(*def);
                }
            }
            MemmapEntry::MacroUse(mu) => {
                for exp in &stash[mu.expansions] {
                    collect_named_macro_defs(db, stash, exp.entries, name, out);
                }
            }
            _ => {}
        }
    }
}

/// Walk a path from a module to find a MacroDefAst at the end.
fn walk_path_to_macro<'db>(
    db: &'db dyn Db,
    mut current: ModSymbol<'db>,
    source_root: SourceRoot,
    segments: &[Name<'db>],
) -> Option<MacroDefAst<'db>> {
    if segments.is_empty() {
        return None;
    }

    for (i, seg) in segments.iter().enumerate() {
        if i < segments.len() - 1 {
            let sym = definition(db, current, *seg, source_root)?;
            current = symbol_to_module(db, sym, source_root, current)?;
        } else {
            let current_ast = match current.data() {
                ModSymbolData::Ast(a) => a,
                ModSymbolData::Ext(_) => return None,
            };
            let target_memmap = expanded_module(db, current_ast, source_root);
            let target_stash = target_memmap.stash(db);
            let target_entries = target_memmap.entries(db);
            for entry in &target_stash[target_entries] {
                match entry {
                    MemmapEntry::MacroDef(def) => {
                        if def.name(db) == *seg {
                            return Some(*def);
                        }
                    }
                    MemmapEntry::Redirect { name, target } => {
                        if *name == *seg {
                            let target_vec: Vec<_> = target_stash[*target].to_vec();
                            return resolve_redirect_to_macro(
                                db,
                                current,
                                source_root,
                                &target_vec,
                            );
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

/// Resolve a use-redirect path to a MacroDefAst.
fn resolve_redirect_to_macro<'db>(
    db: &'db dyn Db,
    current_module: ModSymbol<'db>,
    source_root: SourceRoot,
    segments: &[Name<'db>],
) -> Option<MacroDefAst<'db>> {
    if segments.is_empty() {
        return None;
    }
    let (first_module, rest) =
        memmap_first_segment(db, current_module, source_root, segments).ok()?;
    walk_path_to_macro(db, first_module, source_root, rest)
}

/// Find a MacroDefAst by name in a module's memmap (for glob lookup).
pub(super) fn find_macro_in_module<'db>(
    db: &'db dyn Db,
    module: ModSymbol<'db>,
    name: Name<'db>,
    source_root: SourceRoot,
) -> Option<MacroDefAst<'db>> {
    let ast = match module.data() {
        ModSymbolData::Ast(a) => a,
        ModSymbolData::Ext(_) => return None,
    };
    let memmap = expanded_module(db, ast, source_root);
    let stash = memmap.stash(db);
    let entries = memmap.entries(db);
    for entry in &stash[entries] {
        if let MemmapEntry::MacroDef(def) = entry {
            if def.name(db) == name {
                return Some(*def);
            }
        }
    }
    None
}

fn item_as_child_module<'db>(
    db: &'db dyn Db,
    entry: &MemmapEntry<'db>,
    first: Name<'db>,
    source_root: SourceRoot,
    parent: ModSymbol<'db>,
) -> Option<ModSymbol<'db>> {
    let MemmapEntry::Item(item) = entry else {
        return None;
    };
    let ItemAst::Mod(mod_item) = item else {
        return None;
    };
    if mod_item.name(db) != first {
        return None;
    }
    let scope = crate::scope::ScopeSymbol::Module(parent, source_root);
    let sym = Symbol::local(*item, scope);
    symbol_to_module(db, sym, source_root, parent)
}

fn dedup<'db>(mut defs: Vec<MacroDefAst<'db>>) -> Vec<MacroDefAst<'db>> {
    let mut out: Vec<MacroDefAst<'db>> = Vec::new();
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
