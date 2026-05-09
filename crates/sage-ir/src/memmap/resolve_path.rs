//! Macro path resolution within the MEM-map.
//!
//! Handles all path forms:
//! - `m!()` — single segment, checks local entries then globs
//! - `self::m!()` — resolves in current module
//! - `crate::inner::m!()` — absolute from crate root
//! - `super::m!()` — parent module
//! - `bar::m!()` — bare identifier, checks local modules then extern prelude

use crate::Db;
use crate::item::MacroDefItem;
use crate::module::{Module, ModuleSource};
use crate::name::Name;
use crate::resolve::{Namespace, SourceRoot, definition, resolve_first_segment, symbol_to_module};
use crate::symbol::{Symbol, SymbolSource};
use crate::types::Path;

use super::data::*;
use super::module_memmap;

/// Result of resolving a macro path.
pub(super) enum MacroResolution<'db> {
    Found(MacroDefItem<'db>),
    Ambiguous,
    NotFound,
}

impl<'db> From<Option<MacroDefItem<'db>>> for MacroResolution<'db> {
    fn from(opt: Option<MacroDefItem<'db>>) -> Self {
        match opt {
            Some(def) => MacroResolution::Found(def),
            None => MacroResolution::NotFound,
        }
    }
}

/// Resolve a macro path within the current MEM-map context.
pub(super) fn resolve_memmap_path<'db>(
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
        let name = segments[0];
        for entry in entries {
            if let MemmapEntry::Named(member) = entry {
                if member.name == name {
                    if let NamedMemberKind::MacroDef(def) = &member.kind {
                        return MacroResolution::Found(*def);
                    }
                }
            }
        }
        let mut glob_result: Option<MacroDefItem<'db>> = None;
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
            if segments.len() == 2 {
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
            let first = segments[0];
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
            match resolve_first_segment(db, module, source_root, crate_root, segments) {
                Ok((m, rest)) => walk_path_to_macro(db, m, source_root, crate_root, rest).into(),
                Err(_) => MacroResolution::NotFound,
            }
        }
    }
}

/// Walk a path from a module to find a MacroDefItem at the end.
fn walk_path_to_macro<'db>(
    db: &'db dyn Db,
    mut current: Module<'db>,
    source_root: SourceRoot,
    crate_root: Module<'db>,
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
            let target_memmap = module_memmap(db, current, source_root, crate_root);
            for entry in target_memmap.entries(db) {
                if let MemmapEntry::Named(member) = entry {
                    if member.name == *seg {
                        if let NamedMemberKind::MacroDef(def) = &member.kind {
                            return Some(*def);
                        }
                        if let NamedMemberKind::Redirect { target } = &member.kind {
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

/// Resolve a use-redirect path to a MacroDefItem.
fn resolve_redirect_to_macro<'db>(
    db: &'db dyn Db,
    current_module: Module<'db>,
    source_root: SourceRoot,
    crate_root: Module<'db>,
    path: Path<'db>,
) -> Option<MacroDefItem<'db>> {
    let segments = path.segments(db);
    if segments.is_empty() {
        return None;
    }
    let (first_module, rest) =
        resolve_first_segment(db, current_module, source_root, crate_root, segments).ok()?;
    walk_path_to_macro(db, first_module, source_root, crate_root, rest)
}

/// Find a MacroDefItem by name in a module's memmap (for glob lookup).
pub(super) fn find_macro_in_module<'db>(
    db: &'db dyn Db,
    module: Module<'db>,
    name: Name<'db>,
    source_root: SourceRoot,
    crate_root: Module<'db>,
) -> Option<MacroDefItem<'db>> {
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
