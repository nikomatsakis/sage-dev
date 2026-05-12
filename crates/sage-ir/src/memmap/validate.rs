//! Validation: detect errors in the converged MEM-map.
//!
//! Runs after the fixpoint converges to detect:
//! - Duplicate non-glob names in the same namespace
//! - Unresolved macro invocations (path never resolved)
//! - Ambiguous macro invocations (multiple candidates or branches)
//! - Time-travel violations (macro expansion introduces a name that
//!   shadows a glob import)

use crate::Db;
use crate::item::Item;
use crate::module::{Module, ModuleSource};
use crate::name::Name;
use crate::resolve::{
    MacroKind, Namespace, SourceRoot, item_in_namespace, item_name,
    resolve_use_path_to_module_from_path,
};
use crate::types::Path;

use super::data::*;
use super::module_memmap;

/// Errors detected by inspecting the converged MEM-map.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MemmapError<'db> {
    /// Two non-glob items with the same name in the same namespace.
    DuplicateName { name: Name<'db>, ns: Namespace },
    /// A macro invocation whose path never resolved after convergence.
    UnresolvedMacro { path: Path<'db> },
    /// A macro invocation that resolved to multiple candidates.
    AmbiguousMacro { path: Path<'db> },
    /// A macro expansion introduced a non-glob name that conflicts
    /// with a glob-imported name.
    TimeTravelViolation { name: Name<'db>, ns: Namespace },
}

pub fn memmap_errors<'db>(
    db: &'db dyn Db,
    module: Module<'db>,
    source_root: SourceRoot,
    crate_root: Module<'db>,
) -> Vec<MemmapError<'db>> {
    let memmap = module_memmap(db, module, source_root, crate_root);
    let entries = memmap.entries(db);
    let mut errors = Vec::new();

    // Duplicate names: flatten all (name, ns) pairs across the tree, then
    // report any pair that appears twice.
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

    // Time-travel: names introduced by expansion that also come via glob.
    let mut expanded_names: Vec<(Name<'db>, Namespace)> = Vec::new();
    collect_expanded_names(db, entries, &mut expanded_names);
    for (name, ns) in &expanded_names {
        if name_available_via_glob(db, module, entries, *name, *ns, source_root, crate_root) {
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

/// Collect `(name, namespace)` pairs for every named entry in the tree
/// (top-level and inside all `MacroUse::Expanded` branches).
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
            MemmapEntry::MacroDef(def) => {
                out.push((def.name(db), Namespace::Macro(MacroKind::Bang)));
            }
            MemmapEntry::Redirect { name, target: _ } => {
                // Redirects' namespace is dynamic — conservatively count
                // each redirect in Type namespace for the duplicate check
                // (matches pre-refactor behaviour which stored Type).
                out.push((*name, Namespace::Type));
            }
            MemmapEntry::Glob { .. } => {}
            MemmapEntry::MacroUse(mu) => {
                if let MacroUseState::Expanded(exps) = &mu.state {
                    for exp in exps {
                        collect_all_names(db, &exp.entries, out);
                    }
                }
            }
        }
    }
}

/// Recursively collect resolution errors from `MacroUse` entries.
fn collect_macro_errors<'db>(entries: &[MemmapEntry<'db>], errors: &mut Vec<MemmapError<'db>>) {
    for entry in entries {
        if let MemmapEntry::MacroUse(mu) = entry {
            match &mu.state {
                MacroUseState::Unresolved => {
                    errors.push(MemmapError::UnresolvedMacro { path: mu.path });
                }
                MacroUseState::Resolved(callees) if callees.len() > 1 => {
                    errors.push(MemmapError::AmbiguousMacro { path: mu.path });
                }
                MacroUseState::Resolved(_) => {}
                MacroUseState::Expanded(exps) => {
                    if exps.len() > 1 {
                        errors.push(MemmapError::AmbiguousMacro { path: mu.path });
                    }
                    for exp in exps {
                        collect_macro_errors(&exp.entries, errors);
                    }
                }
            }
        }
    }
}

/// Collect names introduced only through expansions.
fn collect_expanded_names<'db>(
    db: &'db dyn Db,
    entries: &[MemmapEntry<'db>],
    out: &mut Vec<(Name<'db>, Namespace)>,
) {
    for entry in entries {
        if let MemmapEntry::MacroUse(mu) = entry {
            if let MacroUseState::Expanded(exps) = &mu.state {
                for exp in exps {
                    collect_all_names(db, &exp.entries, out);
                }
            }
        }
    }
}

/// Does `name` appear under the given `ns` through any top-level glob?
fn name_available_via_glob<'db>(
    db: &'db dyn Db,
    module: Module<'db>,
    entries: &[MemmapEntry<'db>],
    name: Name<'db>,
    ns: Namespace,
    source_root: SourceRoot,
    crate_root: Module<'db>,
) -> bool {
    for entry in entries {
        if let MemmapEntry::Glob { path } = entry {
            let Some(target) =
                resolve_use_path_to_module_from_path(db, module, source_root, crate_root, *path)
            else {
                continue;
            };
            if matches!(target.source(db), ModuleSource::External(..)) {
                continue;
            }
            let source_memmap = module_memmap(db, target, source_root, crate_root);
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

// Silence `Item` unused-import warning when compiled without certain cfg.
#[allow(dead_code)]
fn _use_item(_: Item<'_>) {}
