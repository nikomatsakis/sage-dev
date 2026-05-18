//! Validation: detect errors in the converged MEM-map.

use crate::Db;
use crate::item::ItemAst;
use crate::module::{ModSymbol, ModSymbolData};
use crate::name::Name;
use crate::resolve::{
    MacroKind, Namespace, SourceRoot, item_in_namespace, item_name,
    resolve_use_path_to_module_from_path,
};
use crate::types::Path;

use super::data::*;
use super::expanded_module;

/// Errors detected by inspecting the converged MEM-map.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MemmapError<'db> {
    DuplicateName { name: Name<'db>, ns: Namespace },
    UnresolvedMacro { path: Path<'db> },
    AmbiguousMacro { path: Path<'db> },
    TimeTravelViolation { name: Name<'db>, ns: Namespace },
    UnresolvedRedirect { name: Name<'db> },
    UnresolvedGlob { path: Path<'db> },
}

pub fn memmap_errors<'db>(
    db: &'db dyn Db,
    module: ModSymbol<'db>,
    source_root: SourceRoot,
) -> Vec<MemmapError<'db>> {
    let ast = match module.data() {
        ModSymbolData::Ast(a) => a,
        ModSymbolData::Ext(_) => return Vec::new(),
    };
    let memmap = expanded_module(db, ast, source_root);
    let entries = memmap.entries(db);
    let mut errors = Vec::new();

    let mut names: Vec<(Name<'db>, Namespace)> = Vec::new();
    collect_all_names(db, entries, &mut names);

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

    collect_macro_errors(entries, &mut errors);
    collect_unresolved_redirects_globs(db, module, entries, source_root, &mut errors);

    let mut expanded_names: Vec<(Name<'db>, Namespace)> = Vec::new();
    collect_expanded_names(db, entries, &mut expanded_names);
    for (name, ns) in &expanded_names {
        if name_available_via_glob(db, module, entries, *name, *ns, source_root) {
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

fn collect_all_names<'db>(
    db: &'db dyn Db,
    entries: &[MemmapEntry<'db>],
    out: &mut Vec<(Name<'db>, Namespace)>,
) {
    for entry in entries {
        match entry {
            MemmapEntry::Item(item) => {
                if let Some(name) = item_name(db, *item) {
                    if item_in_namespace(db, *item, Namespace::Type) {
                        out.push((name, Namespace::Type));
                    }
                    if item_in_namespace(db, *item, Namespace::Value) {
                        out.push((name, Namespace::Value));
                    }
                }
            }
            MemmapEntry::TupleStructCtor(s) => {
                out.push((s.name(db), Namespace::Value));
            }
            MemmapEntry::MacroDef(def) => {
                out.push((def.name(db), Namespace::Macro(MacroKind::Bang)));
            }
            MemmapEntry::Redirect { name, target: _ } => {
                out.push((*name, Namespace::Type));
            }
            MemmapEntry::Glob { .. } => {}
            MemmapEntry::MacroUse(mu) => {
                for exp in &mu.expansions {
                    collect_all_names(db, &exp.entries, out);
                }
            }
        }
    }
}

fn collect_macro_errors<'db>(entries: &[MemmapEntry<'db>], errors: &mut Vec<MemmapError<'db>>) {
    for entry in entries {
        if let MemmapEntry::MacroUse(mu) = entry {
            if mu.expansions.is_empty() {
                errors.push(MemmapError::UnresolvedMacro { path: mu.path });
            } else {
                if mu.expansions.len() > 1 {
                    errors.push(MemmapError::AmbiguousMacro { path: mu.path });
                }
                for exp in &mu.expansions {
                    collect_macro_errors(&exp.entries, errors);
                }
            }
        }
    }
}

fn collect_unresolved_redirects_globs<'db>(
    db: &'db dyn Db,
    module: ModSymbol<'db>,
    entries: &[MemmapEntry<'db>],
    source_root: SourceRoot,
    out: &mut Vec<MemmapError<'db>>,
) {
    for entry in entries {
        match entry {
            MemmapEntry::Redirect { name, target } => {
                if resolve_use_path_to_module_from_path(db, module, source_root, *target).is_none()
                    && target_resolves_to_nothing(db, module, source_root, *target)
                {
                    let err = MemmapError::UnresolvedRedirect { name: *name };
                    if !out.contains(&err) {
                        out.push(err);
                    }
                }
            }
            MemmapEntry::Glob { path } => {
                if resolve_use_path_to_module_from_path(db, module, source_root, *path).is_none() {
                    let err = MemmapError::UnresolvedGlob { path: *path };
                    if !out.contains(&err) {
                        out.push(err);
                    }
                }
            }
            MemmapEntry::MacroUse(mu) => {
                for exp in &mu.expansions {
                    collect_unresolved_redirects_globs(db, module, &exp.entries, source_root, out);
                }
            }
            _ => {}
        }
    }
}

fn target_resolves_to_nothing<'db>(
    db: &'db dyn Db,
    current_module: ModSymbol<'db>,
    source_root: SourceRoot,
    path: Path<'db>,
) -> bool {
    // A redirect's target has no inherent namespace — try Type first,
    // then Value and Macro(Bang). If none resolves, the redirect is
    // truly unresolvable.
    current_module
        .resolve_path(db, source_root, path, Namespace::Type)
        .or_else(|_| current_module.resolve_path(db, source_root, path, Namespace::Value))
        .or_else(|_| {
            current_module.resolve_path(db, source_root, path, Namespace::Macro(MacroKind::Bang))
        })
        .is_err()
}

fn collect_expanded_names<'db>(
    db: &'db dyn Db,
    entries: &[MemmapEntry<'db>],
    out: &mut Vec<(Name<'db>, Namespace)>,
) {
    for entry in entries {
        if let MemmapEntry::MacroUse(mu) = entry {
            for exp in &mu.expansions {
                collect_all_names(db, &exp.entries, out);
            }
        }
    }
}

fn name_available_via_glob<'db>(
    db: &'db dyn Db,
    module: ModSymbol<'db>,
    entries: &[MemmapEntry<'db>],
    name: Name<'db>,
    ns: Namespace,
    source_root: SourceRoot,
) -> bool {
    for entry in entries {
        if let MemmapEntry::Glob { path } = entry {
            let Some(target) = resolve_use_path_to_module_from_path(db, module, source_root, *path)
            else {
                continue;
            };
            let target_ast = match target.data() {
                ModSymbolData::Ast(a) => a,
                ModSymbolData::Ext(_) => continue,
            };
            let source_memmap = expanded_module(db, target_ast, source_root);
            for src_entry in source_memmap.entries(db) {
                match src_entry {
                    MemmapEntry::Item(item) => {
                        if item_name(db, *item) == Some(name) && item_in_namespace(db, *item, ns) {
                            return true;
                        }
                    }
                    MemmapEntry::MacroDef(def) => {
                        if def.name(db) == name && matches!(ns, Namespace::Macro(_)) {
                            return true;
                        }
                    }
                    MemmapEntry::Redirect { name: n, .. } => {
                        if *n == name {
                            return true;
                        }
                    }
                    _ => {}
                }
            }
        }
    }
    false
}

#[allow(dead_code)]
fn _use_item(_: ItemAst<'_>) {}
