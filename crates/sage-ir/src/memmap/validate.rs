//! Validation: detect errors in the converged MEM-map.

use sage_stash::{Slice, Stash};

use crate::Db;
use crate::item::ItemAst;
use crate::module::{ModSymbol, ModSymbolData};
use crate::name::Name;
use crate::resolve::{MacroKind, Namespace, Resolver, SourceRoot, item_in_namespace, item_name};
use crate::scope::ScopeSymbol;

use super::data::*;
use super::expanded_module;

/// Errors detected by inspecting the converged MEM-map.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MemmapError<'db> {
    DuplicateName { name: Name<'db>, ns: Namespace },
    UnresolvedMacro { path: Vec<Name<'db>> },
    AmbiguousMacro { path: Vec<Name<'db>> },
    TimeTravelViolation { name: Name<'db>, ns: Namespace },
    UnresolvedRedirect { name: Name<'db> },
    UnresolvedGlob { path: Vec<Name<'db>> },
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
    let stash = memmap.stash(db);
    let entries = memmap.entries(db);
    let mut errors = Vec::new();

    let mut names: Vec<(Name<'db>, Namespace)> = Vec::new();
    collect_all_names(db, stash, entries, &mut names);

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

    collect_macro_errors(stash, entries, &mut errors);
    collect_unresolved_redirects_globs(db, module, stash, entries, source_root, &mut errors);

    let mut expanded_names: Vec<(Name<'db>, Namespace)> = Vec::new();
    collect_expanded_names(db, stash, entries, &mut expanded_names);
    for (name, ns) in &expanded_names {
        if name_available_via_glob(db, module, stash, entries, *name, *ns, source_root) {
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
    stash: &Stash,
    entries: Slice<MemmapEntry<'db>>,
    out: &mut Vec<(Name<'db>, Namespace)>,
) {
    for entry in &stash[entries] {
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
            MemmapEntry::Redirect { name, .. } => {
                out.push((*name, Namespace::Type));
            }
            MemmapEntry::Glob { .. } => {}
            MemmapEntry::MacroUse(mu) => {
                for exp in &stash[mu.expansions] {
                    collect_all_names(db, stash, exp.entries, out);
                }
            }
        }
    }
}

fn collect_macro_errors<'db>(
    stash: &Stash,
    entries: Slice<MemmapEntry<'db>>,
    errors: &mut Vec<MemmapError<'db>>,
) {
    for entry in &stash[entries] {
        if let MemmapEntry::MacroUse(mu) = entry {
            let expansions = &stash[mu.expansions];
            if expansions.is_empty() {
                errors.push(MemmapError::UnresolvedMacro {
                    path: stash[mu.path].to_vec(),
                });
            } else {
                if expansions.len() > 1 {
                    errors.push(MemmapError::AmbiguousMacro {
                        path: stash[mu.path].to_vec(),
                    });
                }
                for exp in expansions {
                    collect_macro_errors(stash, exp.entries, errors);
                }
            }
        }
    }
}

fn collect_unresolved_redirects_globs<'db>(
    db: &'db dyn Db,
    module: ModSymbol<'db>,
    stash: &Stash,
    entries: Slice<MemmapEntry<'db>>,
    source_root: SourceRoot,
    out: &mut Vec<MemmapError<'db>>,
) {
    for entry in &stash[entries] {
        match entry {
            MemmapEntry::Redirect { name, target } => {
                let target_vec: Vec<_> = stash[*target].to_vec();
                let mut resolver = Resolver::new(db, ScopeSymbol::Module(module, source_root));
                if resolver.resolve_segments_to_module(&target_vec).is_err()
                    && target_resolves_to_nothing(db, module, source_root, &target_vec)
                {
                    let err = MemmapError::UnresolvedRedirect { name: *name };
                    if !out.contains(&err) {
                        out.push(err);
                    }
                }
            }
            MemmapEntry::Glob { path } => {
                let path_vec: Vec<_> = stash[*path].to_vec();
                let mut resolver = Resolver::new(db, ScopeSymbol::Module(module, source_root));
                if resolver.resolve_segments_to_module(&path_vec).is_err() {
                    let err = MemmapError::UnresolvedGlob { path: path_vec };
                    if !out.contains(&err) {
                        out.push(err);
                    }
                }
            }
            MemmapEntry::MacroUse(mu) => {
                for exp in &stash[mu.expansions] {
                    collect_unresolved_redirects_globs(
                        db,
                        module,
                        stash,
                        exp.entries,
                        source_root,
                        out,
                    );
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
    segments: &[Name<'db>],
) -> bool {
    let mut resolver = Resolver::new(db, ScopeSymbol::Module(current_module, source_root));
    resolver
        .resolve_segments(segments, Namespace::Type)
        .or_else(|_| resolver.resolve_segments(segments, Namespace::Value))
        .or_else(|_| resolver.resolve_segments(segments, Namespace::Macro(MacroKind::Bang)))
        .is_err()
}

fn collect_expanded_names<'db>(
    db: &'db dyn Db,
    stash: &Stash,
    entries: Slice<MemmapEntry<'db>>,
    out: &mut Vec<(Name<'db>, Namespace)>,
) {
    for entry in &stash[entries] {
        if let MemmapEntry::MacroUse(mu) = entry {
            for exp in &stash[mu.expansions] {
                collect_all_names(db, stash, exp.entries, out);
            }
        }
    }
}

fn name_available_via_glob<'db>(
    db: &'db dyn Db,
    module: ModSymbol<'db>,
    stash: &Stash,
    entries: Slice<MemmapEntry<'db>>,
    name: Name<'db>,
    ns: Namespace,
    source_root: SourceRoot,
) -> bool {
    for entry in &stash[entries] {
        if let MemmapEntry::Glob { path } = entry {
            let path_vec: Vec<_> = stash[*path].to_vec();
            let mut resolver = Resolver::new(db, ScopeSymbol::Module(module, source_root));
            let Ok(target) = resolver.resolve_segments_to_module(&path_vec) else {
                continue;
            };
            let target_ast = match target.data() {
                ModSymbolData::Ast(a) => a,
                ModSymbolData::Ext(_) => continue,
            };
            let source_memmap = expanded_module(db, target_ast, source_root);
            let src_stash = source_memmap.stash(db);
            let src_entries = source_memmap.entries(db);
            for src_entry in &src_stash[src_entries] {
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
