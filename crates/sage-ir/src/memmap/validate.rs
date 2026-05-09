//! Validation: detect errors in the converged MEM-map.
//!
//! Runs after the fixpoint converges to detect:
//! - Duplicate non-glob names in the same namespace
//! - Unresolved macro invocations (path never resolved)
//! - Ambiguous macro invocations (multiple glob candidates)
//! - Time-travel violations (macro expansion introduces a name that shadows a glob import)

use crate::Db;
use crate::module::{Module, ModuleSource};
use crate::name::Name;
use crate::resolve::{Namespace, SourceRoot};
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

    let mut names: Vec<(Name<'db>, Namespace)> = Vec::new();
    collect_all_names(entries, &mut names);

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

    collect_unresolved_macros(entries, &mut errors);

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
