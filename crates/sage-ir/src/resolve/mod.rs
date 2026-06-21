//! Construction-time macro path resolution.
//!
//! Resolves a macro invocation path to a set of candidate callees
//! against a snapshot of the enclosing module's MEM-map entries.

use sage_stash::Stash;

use crate::Db;
use crate::cst::paths::{Path, PathAnchorKind, PathSegment};
use crate::cst::uses::UseKind;
use crate::local_syms::intrinsic_types::IntrinsicTypeSym;
use crate::name::Name;
use crate::symbol::intrinsic::Intrinsic;
use crate::symbol::{DefIndex, ModSymbol, SymExt, SymExtKind, Symbol, SymbolData, UseSymbol};

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, salsa::Update)]
pub enum Namespace {
    /// Types, traits, modules, type aliases, enum variants (as types).
    Type,
    /// Functions, constants, statics, enum variant constructors, local bindings.
    Value,
    /// `macro_rules!` and proc-macro bang macros.
    Macro,
}

#[derive(Copy, Clone)]
pub(crate) enum ResolvePhase {
    MacroExpansion,
    Normal,
}

/// Resolve a single unqualified name in the given module.
pub(crate) fn resolve_name<'db>(
    db: &'db dyn Db,
    phase: ResolvePhase,
    module: ModSymbol<'db>,
    name: Name<'db>,
    namespace: Namespace,
) -> Vec<Symbol<'db>> {
    flexibly_resolve_name_from_module(db, phase, module, name, namespace)
}

pub(crate) fn resolve_path<'db>(
    db: &'db dyn Db,
    phase: ResolvePhase,
    module: ModSymbol<'db>,
    stash: &Stash,
    path: Path<'db>,
    namespace: Namespace,
) -> Vec<Symbol<'db>> {
    match path {
        Path::Relative(first, rest) => {
            if stash[rest].is_empty() {
                // If there is only one segment, then we resolve it (flexibly) against the requested namespace.
                return flexibly_resolve_name_from_module(db, phase, module, first.name, namespace);
            }

            // Otherwise, we (flexibly) resolve it as a module name...
            let symbols =
                flexibly_resolve_name_from_module(db, phase, module, first.name, Namespace::Type);

            // ...and then we resolve the remaining segments against the module's contents.
            resolve_remaining_segments(db, phase, stash, symbols, &stash[rest], namespace)
        }
        Path::Anchored(anchor, members) => {
            let anchor_modules = resolve_anchor(db, module, stash, anchor.kind);
            let rest = &stash[members];

            // If the path IS the anchor (e.g., `use self`), treat it as a module name.
            if rest.is_empty() {
                if namespace != Namespace::Type {
                    // Anchors are always modules, so other namespaces don't apply
                    return vec![];
                }
                return anchor_modules
                    .into_iter()
                    .map(|m| mod_to_symbol(m))
                    .collect();
            }

            // Otherwise, resolve the remaining segments against the anchor modules.
            resolve_remaining_segments_from_modules(
                db,
                phase,
                stash,
                anchor_modules,
                rest,
                namespace,
            )
        }
    }
}

/// The first segment is resolved "flexibly" against multiple
/// scopes in priority order:
///
///   1. Current module (items + named `use` imports)
///   2. Glob imports (`use foo::*`)
///   3. Extern prelude (dependency crate names)
///   4. Standard library prelude (`Option`, `Vec`, etc.)
///   5. Language prelude (primitive types like `i32`, `bool`)
///
/// Subsequent segments are resolved rigidly: each one must name
/// a child of the module found by the previous segment.
///
/// During macro expansion, globs and named items are searched
/// together (no priority split) to avoid time-traveling ambiguities.
fn flexibly_resolve_name_from_module<'db>(
    db: &'db dyn Db,
    phase: ResolvePhase,
    module: ModSymbol<'db>,
    name: Name<'db>,
    namespace: Namespace,
) -> Vec<Symbol<'db>> {
    // Levels 1+2: current module items (named uses) and glob imports,
    // with phase-dependent priority handled by `resolve_name_from_module`.
    let results = resolve_name_from_module(db, phase, module, name, namespace);
    if !results.is_empty() {
        return results;
    }

    // Level 3: extern prelude (dependency crate names).
    if let Some(sym) = lookup_extern_prelude(db, name, namespace) {
        return vec![sym];
    }

    // Level 4: standard library prelude (`use ::std::prelude::rust_2021::*`).
    let results = lookup_std_prelude(db, phase, name, namespace);
    if !results.is_empty() {
        return results;
    }

    // Level 5: language prelude (primitive types like `i32`, `bool`).
    if let Some(sym) = lookup_lang_prelude(db, name, namespace) {
        return vec![sym];
    }

    vec![]
}

fn resolve_remaining_segments<'db>(
    db: &'db dyn Db,
    phase: ResolvePhase,
    stash: &Stash,
    symbols: Vec<Symbol<'db>>,
    rest: &[PathSegment<'db>],
    namespace: Namespace,
) -> Vec<Symbol<'db>> {
    resolve_remaining_segments_from_modules(
        db,
        phase,
        stash,
        symbols.into_iter().flat_map(|s| s.module(db)),
        rest,
        namespace,
    )
}

fn resolve_remaining_segments_from_modules<'db>(
    db: &'db dyn Db,
    phase: ResolvePhase,
    stash: &Stash,
    module_symbols: impl IntoIterator<Item = ModSymbol<'db>>,
    rest: &[PathSegment<'db>],
    namespace: Namespace,
) -> Vec<Symbol<'db>> {
    assert!(!rest.is_empty(), "`rest` must be non-empty");

    module_symbols
        .into_iter()
        .flat_map(|m| match rest {
            [final_segment] => {
                resolve_name_from_module(db, phase, m, final_segment.name, namespace)
            }

            [next_segment, rest @ ..] => {
                let next_symbols =
                    resolve_name_from_module(db, phase, m, next_segment.name, Namespace::Type);
                resolve_remaining_segments(db, phase, stash, next_symbols, rest, namespace)
            }

            [] => {
                panic!("resolve_remaining_segments invoked with empty `rest`")
            }
        })
        .collect()
}

fn resolve_name_from_module<'db>(
    db: &'db dyn Db,
    phase: ResolvePhase,
    module: ModSymbol<'db>,
    name: Name<'db>,
    namespace: Namespace,
) -> Vec<Symbol<'db>> {
    match phase {
        ResolvePhase::MacroExpansion => lookup_in_module(
            db,
            phase,
            LookupFilter {
                named: true,
                globs: true,
            },
            module,
            name,
            namespace,
        ),
        ResolvePhase::Normal => {
            let results = lookup_in_module(
                db,
                phase,
                LookupFilter {
                    named: true,
                    globs: false,
                },
                module,
                name,
                namespace,
            );
            if !results.is_empty() {
                return results;
            }
            lookup_in_module(
                db,
                phase,
                LookupFilter {
                    named: false,
                    globs: true,
                },
                module,
                name,
                namespace,
            )
        }
    }
}

struct LookupFilter {
    named: bool,
    globs: bool,
}

/// Search entries for items whose name matches, including named `use` imports.
/// Also descends into macro expansion results.
fn lookup_in_module<'db>(
    db: &'db dyn Db,
    phase: ResolvePhase,
    filter: LookupFilter,
    module: ModSymbol<'db>,
    name: Name<'db>,
    namespace: Namespace,
) -> Vec<Symbol<'db>> {
    let module_expanded_items = module.expanded_module_items(db);

    let mut results = vec![];
    for &item in module_expanded_items {
        match item.data(db) {
            SymbolData::FnSymbol(..)
            | SymbolData::StructSymbol(..)
            | SymbolData::EnumSymbol(..)
            | SymbolData::TraitSymbol(..)
            | SymbolData::TypeAliasSymbol(..)
            | SymbolData::ConstSymbol(..)
            | SymbolData::StaticSymbol(..)
            | SymbolData::ImplSymbol(..)
            | SymbolData::ModSymbol(..)
            | SymbolData::MacroDefSymbol(..)
            | SymbolData::IntrinsicTypeSymbol(..) => {
                if filter.named
                    && let Some((n, nspace)) = item.name(db)
                    && n == name
                    && nspace == namespace
                {
                    results.push(item);
                }
            }

            SymbolData::UseSymbol(sym) => {
                match sym {
                    UseSymbol::Local(sym) => {
                        let (stash, imports) = sym.imports(db).open();
                        for import in &stash[imports] {
                            match import.kind {
                                UseKind::Named(n) => {
                                    if filter.named && n == name {
                                        results.extend(resolve_path(
                                            db,
                                            phase,
                                            module,
                                            stash,
                                            stash[import.path],
                                            namespace,
                                        ));
                                    }
                                }
                                UseKind::Glob => {
                                    if filter.globs {
                                        let glob_from = resolve_path(
                                            db,
                                            phase,
                                            module,
                                            stash,
                                            stash[import.path],
                                            Namespace::Type,
                                        );
                                        results.extend(resolve_glob(
                                            db, phase, &glob_from, name, namespace,
                                        ));
                                    }
                                }
                                UseKind::Unnamed => {
                                    // nothing to do here
                                }
                            }
                        }
                    }

                    UseSymbol::Ext(_) => {
                        panic!("Not yet implemented: external use items");
                    }
                }
            }
        }
    }

    results
}

fn lookup_extern_prelude<'db>(
    db: &'db dyn Db,
    name: Name<'db>,
    namespace: Namespace,
) -> Option<Symbol<'db>> {
    if namespace != Namespace::Type {
        return None;
    }
    let crate_num = db.tcx().extern_crate(name.text(db))?;
    Some(SymExt::new(db, crate_num, DefIndex(0), SymExtKind::Mod).into())
}

/// Look up `name` in each of the modules that `glob_from` resolved to.
fn resolve_glob<'db>(
    db: &'db dyn Db,
    phase: ResolvePhase,
    glob_from: &[Symbol<'db>],
    name: Name<'db>,
    namespace: Namespace,
) -> Vec<Symbol<'db>> {
    glob_from
        .iter()
        .filter_map(|s| s.module(db))
        .flat_map(|m| resolve_name_from_module(db, phase, m, name, namespace))
        .collect()
}

/// Resolve `name` as if `use ::std::prelude::rust_2021::*` were in scope.
fn lookup_std_prelude<'db>(
    db: &'db dyn Db,
    phase: ResolvePhase,
    name: Name<'db>,
    namespace: Namespace,
) -> Vec<Symbol<'db>> {
    let std_name = Name::new(db, "std".to_owned());
    let Some(std_sym) = lookup_extern_prelude(db, std_name, Namespace::Type) else {
        return vec![];
    };
    let Some(std_mod) = std_sym.module(db) else {
        return vec![];
    };

    let prelude_name = Name::new(db, "prelude".to_owned());
    let prelude_syms = resolve_name_from_module(db, phase, std_mod, prelude_name, Namespace::Type);

    // TODO: derive this from the crate's actual edition
    let edition_name = Name::new(db, "rust_2024".to_owned());
    let edition_mods: Vec<Symbol<'db>> = prelude_syms
        .iter()
        .filter_map(|s| s.module(db))
        .flat_map(|m| resolve_name_from_module(db, phase, m, edition_name, Namespace::Type))
        .collect();

    resolve_glob(db, phase, &edition_mods, name, namespace)
}

fn lookup_lang_prelude<'db>(
    db: &'db dyn Db,
    name: Name<'db>,
    namespace: Namespace,
) -> Option<Symbol<'db>> {
    if namespace != Namespace::Type {
        return None;
    }
    let intrinsic = Intrinsic::from_name(name.text(db))?;
    Some(IntrinsicTypeSym::new(db, intrinsic).into())
}

fn resolve_anchor<'db>(
    db: &'db dyn Db,
    module: ModSymbol<'db>,
    stash: &Stash,
    kind: PathAnchorKind<'db>,
) -> Vec<ModSymbol<'db>> {
    match kind {
        PathAnchorKind::Self_ => vec![module],

        PathAnchorKind::CurrentCrate => vec![crate_root_of(db, module)],

        PathAnchorKind::Super(inner_ptr) => {
            let inner = stash[inner_ptr];
            let inner_modules = resolve_anchor(db, module, stash, inner.kind);
            inner_modules
                .into_iter()
                .filter_map(|m| parent_module(db, m))
                .collect()
        }

        PathAnchorKind::ExternCrate(name) => lookup_extern_prelude(db, name, Namespace::Type)
            .into_iter()
            .filter_map(|s| s.module(db))
            .collect(),

        PathAnchorKind::DollarCrate => {
            // For local macros, equivalent to `crate`.
            vec![crate_root_of(db, module)]
        }
    }
}

fn mod_to_symbol<'db>(m: ModSymbol<'db>) -> Symbol<'db> {
    match m {
        ModSymbol::Local(local) => local.into(),
        ModSymbol::Ext(ext) => ext.into(),
    }
}

fn parent_module<'db>(db: &'db dyn Db, module: ModSymbol<'db>) -> Option<ModSymbol<'db>> {
    match module {
        ModSymbol::Local(local) => Some(local.parent(db)?.module(db).into()),
        ModSymbol::Ext(_) => None,
    }
}

fn crate_root_of<'db>(db: &'db dyn Db, module: ModSymbol<'db>) -> ModSymbol<'db> {
    match module {
        ModSymbol::Local(local) => {
            let mut current = local;
            while let Some(scope) = current.parent(db) {
                current = scope.module(db);
            }
            current.into()
        }
        ModSymbol::Ext(ext) => {
            // For external modules, the crate root is DefIndex(0) of the same crate.
            SymExt::new(db, ext.crate_num(db), DefIndex(0), SymExtKind::Mod).into()
        }
    }
}
