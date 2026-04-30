use crate::Db;
use crate::item::*;
use crate::lower::file_item_tree;
use crate::module::{Module, ModuleSource};
use crate::name::Name;
use crate::source::SourceFile;
use crate::symbol::{Symbol, SymbolSource};
use crate::types::{UseImport, UseKind};

// ---------------------------------------------------------------------------
// Namespace
// ---------------------------------------------------------------------------

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub enum Namespace {
    Type,
    Value,
    Macro,
}

/// Whether an item lives in the given namespace.
fn item_in_namespace(_db: &dyn Db, item: Item<'_>, ns: Namespace) -> bool {
    match (item, ns) {
        (
            Item::Struct(_) | Item::Enum(_) | Item::Trait(_) | Item::TypeAlias(_) | Item::Mod(_),
            Namespace::Type,
        ) => true,
        (Item::Function(_) | Item::Const(_) | Item::Static(_), Namespace::Value) => true,
        // Structs also live in the value namespace (constructor).
        (Item::Struct(_), Namespace::Value) => true,
        // Enum variants live in both type and value, but we don't have them as top-level items.
        // For derive resolution, we need macro namespace — builtins are resolved via extern prelude.
        _ => false,
    }
}

// ---------------------------------------------------------------------------
// Resolution errors
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResolutionError {
    Unresolved,
    Ambiguous,
}

/// A collection of source files in the workspace, keyed by relative path.
#[salsa::input]
pub struct SourceRoot {
    #[returns(ref)]
    pub files: Vec<SourceFile>,
}

/// Items declared in a module (from file_item_tree for local,
/// from TcxDb for external).
#[salsa::tracked(returns(ref))]
pub fn module_items<'db>(db: &'db dyn Db, module: Module<'db>) -> Vec<Item<'db>> {
    match module.source(db) {
        ModuleSource::Local { file, .. } => file_item_tree(db, file).clone(),
        ModuleSource::External(..) => Vec::new(),
    }
}

/// Use imports in a module (from file_item_tree for local,
/// empty for external).
#[salsa::tracked(returns(ref))]
pub fn module_use_imports<'db>(db: &'db dyn Db, module: Module<'db>) -> Vec<UseImport<'db>> {
    match module.source(db) {
        ModuleSource::Local { file, .. } => {
            let items = file_item_tree(db, file);
            let mut imports = Vec::new();
            for item in items {
                if let Item::Use(group) = item {
                    imports.extend_from_slice(group.imports(db));
                }
            }
            imports
        }
        ModuleSource::External(..) => Vec::new(),
    }
}

/// Find a direct child definition by name.
#[salsa::tracked]
pub fn definition<'db>(
    db: &'db dyn Db,
    module: Module<'db>,
    name: Name<'db>,
) -> Option<Symbol<'db>> {
    match module.source(db) {
        ModuleSource::Local { .. } => {
            for item in module_items(db, module) {
                if item_name(db, *item) == Some(name) {
                    return Some(Symbol::new(db, SymbolSource::Local(*item)));
                }
            }
            None
        }
        ModuleSource::External(crate_num, def_index) => {
            let children = db.tcx().module_children(db, crate_num, def_index);
            children
                .into_iter()
                .find(|(n, _, _)| *n == name)
                .map(|(_, sym, _)| sym)
        }
    }
}

/// Resolve a ModItem to its Module.
///
/// For file-based modules (`mod foo;`), looks up `foo.rs` or `foo/mod.rs`
/// in the source root. For inline modules, returns None.
pub fn resolve_mod<'db>(
    db: &'db dyn Db,
    parent: Module<'db>,
    mod_item: ModItem<'db>,
    source_root: SourceRoot,
) -> Option<Module<'db>> {
    if mod_item.items(db).is_some() {
        return None; // inline module
    }

    let ModuleSource::Local { file, .. } = module_source(db, parent) else {
        return None;
    };
    let parent_path = file.path(db);
    let mod_name = mod_item.name(db).text(db);
    let parent_dir = parent_dir_for(parent_path);

    let candidates = [
        format!("{parent_dir}{mod_name}.rs"),
        format!("{parent_dir}{mod_name}/mod.rs"),
    ];

    for candidate in &candidates {
        if let Some(child_file) = lookup_source_file(db, source_root, candidate) {
            return Some(Module::new(
                db,
                ModuleSource::Local {
                    file: child_file,
                    parent: Some(parent),
                },
            ));
        }
    }
    None
}

/// Resolve a module path like ["cmd", "get"] to a Module.
pub fn resolve_module_path<'db>(
    db: &'db dyn Db,
    root: Module<'db>,
    source_root: SourceRoot,
    segments: &[&str],
) -> Option<Module<'db>> {
    let mut current = root;
    for seg_text in segments {
        let name = Name::new(db, (*seg_text).to_owned());
        let sym = definition(db, current, name)?;
        match sym.source(db) {
            SymbolSource::Local(item) => {
                let Item::Mod(mod_item) = item else {
                    return None;
                };
                current = resolve_mod(db, current, mod_item, source_root)?;
            }
            SymbolSource::External(crate_num, def_index) => {
                current = Module::new(db, ModuleSource::External(crate_num, def_index));
            }
        }
    }
    Some(current)
}

/// Extract the name from an item, if it has one.
pub fn item_name<'db>(db: &'db dyn Db, item: Item<'db>) -> Option<Name<'db>> {
    match item {
        Item::Function(f) => Some(f.name(db)),
        Item::Struct(s) => Some(s.name(db)),
        Item::Enum(e) => Some(e.name(db)),
        Item::Trait(t) => Some(t.name(db)),
        Item::TypeAlias(t) => Some(t.name(db)),
        Item::Const(c) => Some(c.name(db)),
        Item::Static(s) => Some(s.name(db)),
        Item::Mod(m) => Some(m.name(db)),
        Item::Impl(_) | Item::Use(_) | Item::Error(_) => None,
    }
}

/// Helper to extract ModuleSource fields without pattern-matching on a reference.
fn module_source<'db>(db: &'db dyn Db, module: Module<'db>) -> ModuleSource<'db> {
    module.source(db).clone()
}

/// Look up a source file by path in the source root.
fn lookup_source_file(db: &dyn Db, root: SourceRoot, path: &str) -> Option<SourceFile> {
    root.files(db).iter().find(|f| f.path(db) == path).copied()
}

/// Compute the directory prefix for resolving child modules.
fn parent_dir_for(path: &str) -> String {
    if path == "lib.rs" || path == "main.rs" {
        return String::new();
    }
    if let Some(prefix) = path.strip_suffix("/mod.rs") {
        return format!("{prefix}/");
    }
    if let Some(stem) = path.strip_suffix(".rs") {
        return format!("{stem}/");
    }
    String::new()
}

// ---------------------------------------------------------------------------
// Name resolution
// ---------------------------------------------------------------------------

/// Resolve a name in a module's scope.
///
/// 1. Check declared items matching namespace
/// 2. Check named use imports — resolve matching import's path on demand
/// 3. Check glob imports — resolve each glob's target module, search children
/// 4. Check extern prelude (via tcx.extern_crate)
pub fn resolve_name<'db>(
    db: &'db dyn Db,
    module: Module<'db>,
    source_root: SourceRoot,
    crate_root: Module<'db>,
    name: Name<'db>,
    ns: Namespace,
) -> Result<Symbol<'db>, ResolutionError> {
    // 1. Declared items
    for item in module_items(db, module) {
        if item_name(db, *item) == Some(name) && item_in_namespace(db, *item, ns) {
            return Ok(Symbol::new(db, SymbolSource::Local(*item)));
        }
    }

    // 2. Named use imports
    let imports = module_use_imports(db, module);
    let mut named_match = None;
    for import in imports {
        if let UseKind::Named(alias) = import.kind(db) {
            if alias == name {
                let sym = resolve_use_path(db, module, source_root, crate_root, *import)?;
                if named_match.is_some() {
                    return Err(ResolutionError::Ambiguous);
                }
                named_match = Some(sym);
            }
        }
    }
    if let Some(sym) = named_match {
        return Ok(sym);
    }

    // 3. Glob imports
    let mut glob_match = None;
    for import in imports {
        if matches!(import.kind(db), UseKind::Glob) {
            if let Ok(target_module) =
                resolve_use_path_to_module(db, module, source_root, crate_root, *import)
            {
                if let Some(sym) = definition(db, target_module, name) {
                    if glob_match.is_some() {
                        return Err(ResolutionError::Ambiguous);
                    }
                    glob_match = Some(sym);
                }
            }
        }
    }
    if let Some(sym) = glob_match {
        return Ok(sym);
    }

    // 4. Extern prelude
    let name_text = name.text(db);
    if let Some(crate_num) = db.tcx().extern_crate(name_text) {
        return Ok(Symbol::new(
            db,
            SymbolSource::External(crate_num, crate::module::DefIndex(0)),
        ));
    }

    Err(ResolutionError::Unresolved)
}

/// Resolve a use import's path to a Symbol.
///
/// First segment: crate → root, self → current, super → parent,
///   bare → current module items then extern prelude.
/// Remaining segments: definition(module, segment) for each.
pub fn resolve_use_path<'db>(
    db: &'db dyn Db,
    current_module: Module<'db>,
    source_root: SourceRoot,
    crate_root: Module<'db>,
    import: UseImport<'db>,
) -> Result<Symbol<'db>, ResolutionError> {
    let segments = import.path(db).segments(db);
    if segments.is_empty() {
        return Err(ResolutionError::Unresolved);
    }

    let (first_module, rest) =
        resolve_first_segment(db, current_module, source_root, crate_root, segments)?;

    // Walk remaining segments
    let mut current = first_module;
    for (i, seg) in rest.iter().enumerate() {
        let sym = definition(db, current, *seg).ok_or(ResolutionError::Unresolved)?;
        if i < rest.len() - 1 {
            // Intermediate segment — must resolve to a module
            current = symbol_to_module(db, sym, source_root, current)
                .ok_or(ResolutionError::Unresolved)?;
        } else {
            // Last segment — return the symbol
            return Ok(sym);
        }
    }

    // If rest is empty, the import is just the first segment (e.g., `use crate`)
    // Return the module itself as a symbol — this is unusual but valid for `use foo::{self}`
    match first_module.source(db) {
        ModuleSource::Local { .. } => {
            // Find the item that corresponds to this module in the parent
            Err(ResolutionError::Unresolved)
        }
        ModuleSource::External(cn, di) => Ok(Symbol::new(db, SymbolSource::External(cn, di))),
    }
}

/// Resolve a use import's path to a Module (for glob imports).
fn resolve_use_path_to_module<'db>(
    db: &'db dyn Db,
    current_module: Module<'db>,
    source_root: SourceRoot,
    crate_root: Module<'db>,
    import: UseImport<'db>,
) -> Result<Module<'db>, ResolutionError> {
    let segments = import.path(db).segments(db);
    if segments.is_empty() {
        return Err(ResolutionError::Unresolved);
    }

    let (first_module, rest) =
        resolve_first_segment(db, current_module, source_root, crate_root, segments)?;

    let mut current = first_module;
    for seg in rest {
        let sym = definition(db, current, *seg).ok_or(ResolutionError::Unresolved)?;
        current =
            symbol_to_module(db, sym, source_root, current).ok_or(ResolutionError::Unresolved)?;
    }
    Ok(current)
}

/// Resolve the first segment of a use path.
/// Returns (module to search in, remaining segments).
fn resolve_first_segment<'db>(
    db: &'db dyn Db,
    current_module: Module<'db>,
    source_root: SourceRoot,
    crate_root: Module<'db>,
    segments: &'db [Name<'db>],
) -> Result<(Module<'db>, &'db [Name<'db>]), ResolutionError> {
    let first = segments[0];
    let first_text = first.text(db);
    let rest = &segments[1..];

    match first_text.as_str() {
        "" => {
            // Leading `::` — force extern prelude lookup on the next segment
            if rest.is_empty() {
                return Err(ResolutionError::Unresolved);
            }
            let crate_name = rest[0].text(db);
            if let Some(crate_num) = db.tcx().extern_crate(crate_name) {
                let ext_mod = Module::new(
                    db,
                    ModuleSource::External(crate_num, crate::module::DefIndex(0)),
                );
                return Ok((ext_mod, &rest[1..]));
            }
            Err(ResolutionError::Unresolved)
        }
        "crate" => Ok((crate_root, rest)),
        "self" => Ok((current_module, rest)),
        "super" => {
            let ModuleSource::Local {
                parent: Some(parent),
                ..
            } = current_module.source(db)
            else {
                return Err(ResolutionError::Unresolved);
            };
            Ok((parent, rest))
        }
        _ => {
            // Bare identifier: check current module's items first
            if let Some(sym) = definition(db, current_module, first) {
                if let Some(child_mod) = symbol_to_module(db, sym, source_root, current_module) {
                    return Ok((child_mod, rest));
                }
                // It's a non-module item — if rest is empty, this is the target
                if rest.is_empty() {
                    // This case is handled by the caller
                }
            }
            // Then extern prelude
            if let Some(crate_num) = db.tcx().extern_crate(first_text) {
                let ext_mod = Module::new(
                    db,
                    ModuleSource::External(crate_num, crate::module::DefIndex(0)),
                );
                return Ok((ext_mod, rest));
            }
            Err(ResolutionError::Unresolved)
        }
    }
}

/// Try to convert a Symbol into a Module (for walking into child segments).
fn symbol_to_module<'db>(
    db: &'db dyn Db,
    sym: Symbol<'db>,
    source_root: SourceRoot,
    parent: Module<'db>,
) -> Option<Module<'db>> {
    match sym.source(db) {
        SymbolSource::Local(Item::Mod(mod_item)) => {
            if mod_item.items(db).is_some() {
                // Inline module — create a Module for it.
                // We don't have a separate SourceFile, so we can't use resolve_mod.
                // For now, return None for inline modules.
                None
            } else {
                resolve_mod(db, parent, mod_item, source_root)
            }
        }
        SymbolSource::External(crate_num, def_index) => Some(Module::new(
            db,
            ModuleSource::External(crate_num, def_index),
        )),
        _ => None,
    }
}
