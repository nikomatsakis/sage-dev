//! Construction-time macro path resolution.
//!
//! Resolves a macro invocation path to a set of candidate callees
//! against a snapshot of the enclosing module's MEM-map entries.

use sage_stash::{Slice, Stash};

use crate::Db;
use crate::cst::macro_invocations::MacroInvocationCstData;
use crate::cst::paths::{Path, PathSegment};
use crate::cst::uses::UseKind;
use crate::local_syms::LocalModItemSym;
use crate::local_syms::macro_defs::LocalMacroDefSym;
use crate::name::Name;
use crate::resolve::{ResolutionError, SourceRoot, definition, symbol_to_module};
use crate::symbol::{DefIndex, MacroDefSymbol, ModSymbol, SymExtKind, Symbol, SymbolData};

use super::data::*;

/// Resolve a macro path within the current MEM-map context.
pub(super) fn resolve_macro_path<'db>(
    db: &'db dyn Db,
    module: ModSymbol<'db>,
    entries_stash: &Stash,
    entries: Slice<MemmapEntry<'db>>,
    macro_use: &MacroInvocation<'db>,
) -> Vec<MacroCallee<'db>> {
    let (
        path_stash,
        MacroInvocationCstData {
            path,
            input_tokens: _,
            span: _,
        },
    ) = macro_use.invocation.cst(db).open_deref();

    let defs = resolve_macro_path_to_defs(db, module, path_stash, entries, path);
    defs.into_iter().map(MacroCallee::Rules).collect()
}

fn resolve_module_path_during_macro_expansion<'db>(
    db: &'db dyn Db,
    module: ModSymbol<'db>,
    path_stash: &Stash,
    path: Path,
    entries_stash: &Stash,
    entries: Slice<MemmapEntry<'db>>,
) -> Vec<Symbol<'db>> {
    match path {
        Path::Relative(first, rest) => {
            let mut macro_defs = Vec::new();

            // If this is something like `foo!()`, then `foo` must be a macro
            if path_stash[rest].is_empty() {
                collect_named_macros(
                    db,
                    module,
                    entries_stash,
                    entries,
                    first.name,
                    &mut macro_defs,
                );
            } else {
                // Otherwise, we have `foo::bar!()`, so `foo` must be resolved as a module.
                let mut modules = Vec::new();
                collect_modules(db, module, entries_stash, entries, first.name, &mut modules);
                for m in modules {
                    resolve_relative_module_path_during_macro_expansion(
                        db,
                        m,
                        path_stash,
                        rest,
                        &mut macro_defs,
                    );
                }
            }
        }
        Path::Anchored(_anchor, _members) => {
            // An anchored path like `super::bar::baz` etc.
            todo!()
        }
    }

    vec![]
}

fn resolve_relative_module_path_during_macro_expansion<'db>(
    db: &'db dyn Db,
    module: ModSymbol<'db>,
    path_stash: &Stash,
    rest: &[PathSegment<'db>],
    out: &mut Vec<Symbol<'db>>,
) {
    match &rest {
        [] => panic!("should not have empty list of segments"),
        [last] => {}
        [next, rest @ ..] => {}
    }
}

/// Search entries for items whose name matches, including named `use` imports
/// and macro expansion results.
///
/// Because this search is taking place during macro expansion, we look through
/// glob imports eagerly, even if there are named items. This is required to avoid
/// time-traveling artifacts.
fn each_symbol_with_name<'db>(
    db: &'db dyn Db,
    module: ModSymbol<'db>,
    name: Name<'db>,
    entries_stash: &Stash,
    entries: Slice<MemmapEntry<'db>>,
    op: &mut impl FnMut(Symbol<'db>),
) {
    for &entry in &entries_stash[entries] {
        match entry {
            MemmapEntry::MacroInvocation(invocation) => {
                for expansion in &entries_stash[invocation.expansions] {
                    each_memmap_entry_with_name(db, name, entries_stash, expansion.entries, op);
                }
            }

            MemmapEntry::Imports(use_sym) => {
                let (use_stash, use_imports) = use_sym.imports(db).open();
                for &import in &use_stash[use_imports] {
                    match import.kind {
                        UseKind::Named(n) => {
                            // `use foo::Bar` -- resolve the path if we are looking for `Bar`
                            if n == name {
                                for used_item in resolve_module_path_during_macro_expansion(
                                    db,
                                    module,
                                    use_stash,
                                    use_stash[import.path],
                                    entries_stash,
                                    entries,
                                ) {
                                    if let Some(n) = used_item.name(db) {
                                        if n == name {
                                            op(used_item);
                                        }
                                    }
                                }
                            }
                        }
                        UseKind::Glob => {
                            // `use foo::bar::*` -- resolve the path, scan for the item we are looking for
                            for used_item in resolve_module_path_during_macro_expansion(
                                db,
                                module,
                                use_stash,
                                use_stash[import.path],
                                entries_stash,
                                entries,
                            ) {
                                if let SymbolData::ModSymbol(used_mod_item) = used_item.data(db) {
                                    for &glob_item in used_mod_item.expanded_module_items(db) {
                                        if let Some(n) = glob_item.name(db) {
                                            if n == name {
                                                op(glob_item);
                                            }
                                        }
                                    }
                                }
                            }
                        }
                        UseKind::Unnamed => {
                            // we can ignore `use foo::Bar as _` since it does not create visible names
                        }
                    }
                }
            }

            MemmapEntry::Function(n, sym) => {
                if n == name {
                    op(sym.into());
                }
            }
            MemmapEntry::Struct(n, sym) => {
                if n == name {
                    op(sym.into());
                }
            }
            MemmapEntry::Enum(n, sym) => {
                if n == name {
                    op(sym.into());
                }
            }
            MemmapEntry::Trait(n, sym) => {
                if n == name {
                    op(sym.into());
                }
            }
            MemmapEntry::Impl(n, sym) => {
                if n == name {
                    op(sym.into());
                }
            }
            MemmapEntry::TypeAlias(n, sym) => {
                if n == name {
                    op(sym.into());
                }
            }
            MemmapEntry::Const(n, sym) => {
                if n == name {
                    op(sym.into());
                }
            }
            MemmapEntry::Static(n, sym) => {
                if n == name {
                    op(sym.into());
                }
            }
            MemmapEntry::Mod(n, sym) => {
                if n == name {
                    op(sym.into());
                }
            }
            MemmapEntry::MacroDef(n, sym) => {
                if n == name {
                    op(sym.into());
                }
            }
        }
    }
}

/// Search entries for items whose name matches, including named `use` imports.
/// Also descends into macro expansion results.
fn collect_modules<'db>(
    db: &'db dyn Db,
    module: ModSymbol<'db>,
    stash: &Stash,
    entries: Slice<MemmapEntry<'db>>,
    name: Name<'db>,
    out: &mut Vec<ModSymbol<'db>>,
) {
    each_memmap_entry_with_name(db, stash, entries, &mut |entry| {
        if let MemmapEntry::Mod(n, sym) = entry {
            if n == name {
                out.push(MacroCallee::Module(sym));
            }
        }
    });
}

/// Search entries for items whose name matches, including named `use` imports.
/// Also descends into macro expansion results.
fn collect_named_macros<'db>(
    db: &'db dyn Db,
    module: ModSymbol<'db>,
    stash_entries: &Stash,
    entries: Slice<MemmapEntry<'db>>,
    name: Name<'db>,
    out: &mut Vec<MacroCallee<'db>>,
) {
    each_symbol_with_name(
        db,
        module,
        name,
        stash_entries,
        entries,
        &mut |sym| match sym.data(db) {
            SymbolData::MacroDefSymbol(macro_def_symbol) => match macro_def_symbol {
                MacroDefSymbol::Local(sym) => out.push(MacroCallee::Rules(sym)),
                MacroDefSymbol::Ext(sym_ext) => match sym_ext.kind(db) {
                    SymExtKind::MacroDef => out.push(MacroCallee::ExtRules(sym_ext)),
                    SymExtKind::Fn
                    | SymExtKind::Struct
                    | SymExtKind::TupleStructCtor
                    | SymExtKind::Enum
                    | SymExtKind::Trait
                    | SymExtKind::Impl
                    | SymExtKind::Mod
                    | SymExtKind::TypeAlias
                    | SymExtKind::Const
                    | SymExtKind::Static
                    | SymExtKind::Use
                    | SymExtKind::Other => {
                        unreachable!("unexpected sym ext kind: {:?}", sym_ext.kind(db))
                    }
                },
            },
        },
    );
}

/// Check a single `use` declaration for a named import matching `name`,
/// and resolve the import path to a symbol.
fn collect_from_use_named<'db>(
    db: &'db dyn Db,
    module: ModSymbol<'db>,
    source_root: SourceRoot,
    use_sym: crate::local_syms::uses::LocalUseSym<'db>,
    name: Name<'db>,
    out: &mut Vec<Symbol<'db>>,
) {
    let (use_stash, imports) = use_sym.imports(db).open();
    for import in &use_stash[imports] {
        let UseKind::Named(alias) = import.kind else {
            continue;
        };
        if alias != name {
            continue;
        }
        let path = use_stash[import.path];
        let path_names = path_to_names(db, &use_stash, path);
        if let Some(sym) = resolve_use_path_to_symbol(db, module, source_root, &path_names) {
            if !out.contains(&sym) {
                out.push(sym);
            }
        }
    }
}

/// Resolve a use-path (as a list of names) to a symbol.
fn resolve_use_path_to_symbol<'db>(
    db: &'db dyn Db,
    module: ModSymbol<'db>,
    source_root: SourceRoot,
    segments: &[Name<'db>],
) -> Option<Symbol<'db>> {
    if segments.is_empty() {
        return None;
    }
    let (leading, last) = segments.split_at(segments.len() - 1);
    if leading.is_empty() {
        // Single-segment use: `use foo;` — look up in the current module.
        definition(db, module, last[0], source_root)
    } else {
        let target_module = resolve_path_to_module(db, module, source_root, leading)?;
        definition(db, target_module, last[0], source_root)
    }
}

/// Search glob imports for a name. For each `use foo::*` in entries,
/// resolve `foo` to a module and look for `name` inside it.
fn collect_from_globs<'db>(
    db: &'db dyn Db,
    module: ModSymbol<'db>,
    source_root: SourceRoot,
    stash: &Stash,
    entries: Slice<MemmapEntry<'db>>,
    name: Name<'db>,
    out: &mut Vec<Symbol<'db>>,
) {
    for entry in &stash[entries] {
        let MemmapEntry::Imports(use_sym) = entry else {
            continue;
        };
        let (use_stash, imports) = use_sym.imports(db).open();
        for import in &use_stash[imports] {
            if !matches!(import.kind, crate::cst::uses::UseKind::Glob) {
                continue;
            }
            // Resolve the glob source path to a module, then look for `name` in it.
            let glob_path = use_stash[import.path];
            let path_names = path_to_names(db, &use_stash, glob_path);
            let Some(target_module) = resolve_path_to_module(db, module, source_root, &path_names)
            else {
                continue;
            };
            if let Some(sym) = definition(db, target_module, name, source_root) {
                if !out.contains(&sym) {
                    out.push(sym);
                }
            }
        }
    }
}

/// Resolve a sequence of names to a module, starting from `module`.
fn resolve_path_to_module<'db>(
    db: &'db dyn Db,
    module: ModSymbol<'db>,
    source_root: SourceRoot,
    segments: &[Name<'db>],
) -> Option<ModSymbol<'db>> {
    let mut current = module;
    for seg in segments {
        let sym = definition(db, current, *seg, source_root)?;
        current = symbol_to_module(db, sym, source_root, current)?;
    }
    Some(current)
}

/// Flatten a `Path` to a list of names (for use-path resolution within this module).
fn path_to_names<'db>(db: &'db dyn Db, stash: &Stash, path: Path<'db>) -> Vec<Name<'db>> {
    let mut names = Vec::new();
    match path {
        Path::Anchored(anchor, seg_slice) => {
            collect_anchor_names(db, stash, anchor, &mut names);
            let segs = &stash[seg_slice];
            names.extend(segs.iter().map(|s| s.name));
        }
        Path::Relative(first, rest_slice) => {
            names.push(first.name);
            let rest = &stash[rest_slice];
            names.extend(rest.iter().map(|s| s.name));
        }
    }
    names
}

fn collect_anchor_names<'db>(
    db: &'db dyn Db,
    stash: &Stash,
    anchor: crate::cst::paths::PathAnchor<'db>,
    out: &mut Vec<Name<'db>>,
) {
    use crate::cst::paths::PathAnchorKind;
    match anchor.kind {
        PathAnchorKind::ExternCrate(name) => {
            out.push(Name::new(db, String::new()));
            out.push(name);
        }
        PathAnchorKind::CurrentCrate => {
            out.push(Name::new(db, "crate".to_owned()));
        }
        PathAnchorKind::Self_ => {
            out.push(Name::new(db, "self".to_owned()));
        }
        PathAnchorKind::DollarCrate => {
            out.push(Name::new(db, "$crate".to_owned()));
        }
        PathAnchorKind::Super(inner_ptr) => {
            let inner = stash[inner_ptr];
            collect_anchor_names(db, stash, inner, out);
            out.push(Name::new(db, "super".to_owned()));
        }
    }
}

/// Derive the `SourceRoot` from a local module by reading its parent scope.
/// Returns `None` for the crate root (whose parent is `None`) or external modules.
pub(super) fn source_root_of<'db>(db: &'db dyn Db, module: ModSymbol<'db>) -> Option<SourceRoot> {
    let ModSymbol::Ast(ast) = module else {
        return None;
    };
    Some(ast.parent(db)?.source_root(db))
}

/// Walk a path from a module to find a MacroDefAst at the end.
fn walk_path_to_macro<'db>(
    db: &'db dyn Db,
    mut current: ModSymbol<'db>,
    source_root: SourceRoot,
    segments: &[Name<'db>],
) -> Option<LocalMacroDefSym<'db>> {
    if segments.is_empty() {
        return None;
    }

    for (i, seg) in segments.iter().enumerate() {
        if i < segments.len() - 1 {
            let sym = definition(db, current, *seg, source_root)?;
            current = symbol_to_module(db, sym, source_root, current)?;
        } else {
            let current_ast = match current {
                ModSymbol::Ast(a) => a,
                ModSymbol::Ext(_) => return None,
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
) -> Option<LocalMacroDefSym<'db>> {
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
) -> Option<LocalMacroDefSym<'db>> {
    let ast = match module {
        ModSymbol::Ast(a) => a,
        ModSymbol::Ext(_) => return None,
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
    let LocalModItemSym::Mod(mod_item) = item else {
        return None;
    };
    if mod_item.name(db) != first {
        return None;
    }
    let scope = crate::scope::ScopeSymbol::Module(parent, source_root);
    let sym = Symbol::local(*item, scope);
    symbol_to_module(db, sym, source_root, parent)
}

fn dedup<'db>(mut defs: Vec<LocalMacroDefSym<'db>>) -> Vec<LocalMacroDefSym<'db>> {
    let mut out: Vec<LocalMacroDefSym<'db>> = Vec::new();
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
