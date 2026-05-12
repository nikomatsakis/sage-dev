use crate::Db;
use crate::item::*;
use crate::lower::file_item_tree;
use crate::module::{Module, ModuleSource};
use crate::name::Name;
use crate::source::SourceFile;
use crate::symbol::{Symbol, SymbolSource};
use crate::types::{Path, UseImport};

// ---------------------------------------------------------------------------
// Namespace
// ---------------------------------------------------------------------------

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub enum MacroKind {
    Bang,
    Attr,
    Derive,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub enum Namespace {
    Type,
    Value,
    Macro(MacroKind),
}

/// Whether an item lives in the given namespace.
pub(crate) fn item_in_namespace(_db: &dyn Db, item: Item<'_>, ns: Namespace) -> bool {
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
#[salsa::input(debug)]
pub struct SourceRoot {
    #[returns(ref)]
    pub files: Vec<SourceFile>,
}

/// Items declared in a module (from file_item_tree for local,
/// from the mod item for inline, from TcxDb for external — though the
/// external branch always returns empty here; use `definition` instead).
/// Format a module for logging.
fn module_label(db: &dyn Db, module: Module<'_>) -> String {
    match module.source(db) {
        ModuleSource::Local { file, .. } => format!("\"{}\"", file.path(db)),
        ModuleSource::LocalInline { mod_item, .. } => {
            format!("inline \"{}\"", mod_item.name(db).text(db))
        }
        ModuleSource::External(cn, di) => format!("extern({}, {})", cn.0, di.0),
    }
}

#[salsa::tracked(returns(ref))]
pub fn module_items<'db>(db: &'db dyn Db, module: Module<'db>) -> Vec<Item<'db>> {
    db.log_query(format!("module_items({})", module_label(db, module)));
    match module.source(db) {
        ModuleSource::Local { file, .. } => file_item_tree(db, file).clone(),
        ModuleSource::LocalInline { mod_item, .. } => {
            mod_item.items(db).clone().unwrap_or_default()
        }
        ModuleSource::External(..) => Vec::new(),
    }
}

/// Use imports in a module (from file_item_tree for local,
/// from the mod item's items for inline, empty for external).
#[salsa::tracked(returns(ref))]
pub fn module_use_imports<'db>(db: &'db dyn Db, module: Module<'db>) -> Vec<UseImport<'db>> {
    db.log_query(format!("module_use_imports({})", module_label(db, module)));
    match module.source(db) {
        ModuleSource::Local { file, .. } => {
            let items = file_item_tree(db, file);
            collect_use_imports(db, items.as_slice())
        }
        ModuleSource::LocalInline { mod_item, .. } => {
            let items: Vec<Item<'db>> = mod_item.items(db).clone().unwrap_or_default();
            collect_use_imports(db, &items)
        }
        ModuleSource::External(..) => Vec::new(),
    }
}

fn collect_use_imports<'db>(db: &'db dyn Db, items: &[Item<'db>]) -> Vec<UseImport<'db>> {
    let mut imports = Vec::new();
    for item in items {
        if let Item::Use(group) = item {
            imports.extend_from_slice(group.imports(db));
        }
    }
    imports
}

/// Find a direct child definition by name.
#[salsa::tracked]
pub fn definition<'db>(
    db: &'db dyn Db,
    module: Module<'db>,
    name: Name<'db>,
) -> Option<Symbol<'db>> {
    db.log_query(format!(
        "definition({}, \"{}\")",
        module_label(db, module),
        name.text(db)
    ));
    match module.source(db) {
        ModuleSource::Local { .. } | ModuleSource::LocalInline { .. } => {
            for item in module_items(db, module) {
                if item_name(db, *item) == Some(name) {
                    return Some(Symbol::new(db, SymbolSource::Local(*item)));
                }
            }
            None
        }
        ModuleSource::External(crate_num, def_index) => {
            let raw = db.tcx().module_children(crate_num, def_index);
            let name_text = name.text(db);
            raw.into_iter()
                .find(|c| c.name == *name_text)
                .map(|c| Symbol::new(db, SymbolSource::External(c.crate_num, c.def_index)))
        }
    }
}

/// Like `definition`, but filters by namespace for external modules.
/// For local modules, behaves like `definition` (no namespace filter on items).
pub fn definition_in_ns<'db>(
    db: &'db dyn Db,
    module: Module<'db>,
    name: Name<'db>,
    ns: Namespace,
) -> Option<Symbol<'db>> {
    match module.source(db) {
        ModuleSource::Local { .. } | ModuleSource::LocalInline { .. } => {
            definition(db, module, name)
        }
        ModuleSource::External(crate_num, def_index) => {
            let raw = db.tcx().module_children(crate_num, def_index);
            let name_text = name.text(db);
            raw.into_iter()
                .find(|c| c.name == *name_text && c.namespace == ns)
                .map(|c| Symbol::new(db, SymbolSource::External(c.crate_num, c.def_index)))
        }
    }
}

/// Resolve a ModItem to its Module.
///
/// For file-based modules (`mod foo;`), looks up `foo.rs` or `foo/mod.rs`
/// in the source root and creates a `ModuleSource::Local` module.
/// For inline modules (`mod foo { ... }`), creates a
/// `ModuleSource::LocalInline` module keyed on the parent + mod item.
pub fn resolve_mod<'db>(
    db: &'db dyn Db,
    parent: Module<'db>,
    mod_item: ModItem<'db>,
    source_root: SourceRoot,
) -> Option<Module<'db>> {
    if mod_item.items(db).is_some() {
        return Some(Module::new(
            db,
            ModuleSource::LocalInline { parent, mod_item },
        ));
    }

    let Some(parent_file) = parent.containing_file(db) else {
        return None;
    };
    let parent_path = parent_file.path(db);
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
                    declaration: Some(mod_item),
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
        // Use memmap-aware lookup so macro-created modules are visible.
        let sym = definition_via_memmap(db, current, source_root, root, name, Namespace::Type)
            .or_else(|| definition(db, current, name))?;
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
        Item::Impl(_)
        | Item::Use(_)
        | Item::MacroDef(_)
        | Item::MacroInvocation(_)
        | Item::Error(_) => None,
    }
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

/// Resolve a name in a module's scope using the MEM-map.
///
/// Priority (highest to lowest):
/// 1. Non-glob entries (declared items + named use-redirects)
/// 2. Glob imports (search each glob stem's source module)
/// 3. Extern prelude (via tcx.extern_crate)
/// 4. Std prelude (implicit `use std::prelude::v1::*`)
///
/// Multiple non-glob matches → Ambiguous.
/// Zero non-globs + multiple glob matches → Ambiguous.
pub fn resolve_name<'db>(
    db: &'db dyn Db,
    module: Module<'db>,
    source_root: SourceRoot,
    crate_root: Module<'db>,
    name: Name<'db>,
    ns: Namespace,
) -> Result<Symbol<'db>, ResolutionError> {
    use crate::memmap::module_memmap;

    let memmap = module_memmap(db, module, source_root, crate_root);

    // 1. Non-glob lookup: collect all NamedMember entries with matching name+ns
    //    This includes entries inside expanded macro subtrees.
    let mut non_glob_matches: Vec<Symbol<'db>> = Vec::new();
    collect_named_matches(
        db,
        module,
        source_root,
        crate_root,
        memmap.entries(db),
        name,
        ns,
        &mut non_glob_matches,
    );

    match non_glob_matches.len() {
        1 => return Ok(non_glob_matches[0]),
        n if n > 1 => return Err(ResolutionError::Ambiguous),
        _ => {}
    }

    // 2. Glob lookup: for each GlobStem, search the source module's memmap
    let mut glob_matches: Vec<Symbol<'db>> = Vec::new();
    collect_glob_matches(
        db,
        module,
        source_root,
        crate_root,
        memmap.entries(db),
        name,
        ns,
        &mut glob_matches,
    );

    match glob_matches.len() {
        1 => return Ok(glob_matches[0]),
        n if n > 1 => return Err(ResolutionError::Ambiguous),
        _ => {}
    }

    // 3. Extern prelude
    let name_text = name.text(db);
    if let Some(crate_num) = db.tcx().extern_crate(name_text) {
        return Ok(Symbol::new(
            db,
            SymbolSource::External(crate_num, crate::module::DefIndex(0)),
        ));
    }

    // 4. Std prelude
    if let Some(sym) = resolve_in_std_prelude(db, name, ns) {
        return Ok(sym);
    }

    Err(ResolutionError::Unresolved)
}

/// Recursively collect named matches from entries (including expanded subtrees).
fn collect_named_matches<'db>(
    db: &'db dyn Db,
    module: Module<'db>,
    source_root: SourceRoot,
    crate_root: Module<'db>,
    entries: &[crate::memmap::MemmapEntry<'db>],
    name: Name<'db>,
    ns: Namespace,
    out: &mut Vec<Symbol<'db>>,
) {
    use crate::memmap::{MacroUseState, MemmapEntry};

    for entry in entries {
        match entry {
            MemmapEntry::Item(item) => {
                if item_name(db, *item) != Some(name) {
                    continue;
                }
                if !item_in_namespace(db, *item, ns) {
                    continue;
                }
                let sym = Symbol::new(db, SymbolSource::Local(*item));
                if !out.contains(&sym) {
                    out.push(sym);
                }
            }
            MemmapEntry::MacroDef(def) => {
                if def.name(db) != name {
                    continue;
                }
                if !matches!(ns, Namespace::Macro(_)) {
                    continue;
                }
                // MacroDef is not representable as a Symbol today; skip
                // for now (Phase 3 will produce a proper MacroCallee).
                let _ = def;
            }
            MemmapEntry::Redirect { name: n, target } => {
                if *n != name {
                    continue;
                }
                // Namespace is dynamic — resolve the target and let
                // downstream decide (we return the resolved symbol
                // regardless of `ns` here; the post-construction
                // semantics in Phase 2 will filter properly).
                match resolve_path_to_symbol(db, module, source_root, crate_root, *target) {
                    Ok(sym) => {
                        if !out.contains(&sym) {
                            out.push(sym);
                        }
                    }
                    Err(_) => continue,
                }
            }
            MemmapEntry::Glob { .. } => {}
            MemmapEntry::MacroUse(mu) => {
                if let MacroUseState::Expanded(exps) = &mu.state {
                    for exp in exps {
                        collect_named_matches(
                            db,
                            module,
                            source_root,
                            crate_root,
                            &exp.entries,
                            name,
                            ns,
                            out,
                        );
                    }
                }
            }
        }
    }
}

/// Collect glob matches from entries (including expanded subtrees).
fn collect_glob_matches<'db>(
    db: &'db dyn Db,
    current_module: Module<'db>,
    source_root: SourceRoot,
    crate_root: Module<'db>,
    entries: &[crate::memmap::MemmapEntry<'db>],
    name: Name<'db>,
    ns: Namespace,
    out: &mut Vec<Symbol<'db>>,
) {
    use crate::memmap::{MacroUseState, MemmapEntry};

    for entry in entries {
        match entry {
            MemmapEntry::Glob { path } => {
                let Some(target) = resolve_use_path_to_module_from_path(
                    db,
                    current_module,
                    source_root,
                    crate_root,
                    *path,
                ) else {
                    continue;
                };
                // Use memmap-aware lookup for local targets so that
                // macro-expanded items in the target are visible.
                let sym = match target.source(db) {
                    ModuleSource::External(..) => definition_in_ns(db, target, name, ns),
                    _ => definition_via_memmap(db, target, source_root, crate_root, name, ns),
                };
                if let Some(sym) = sym {
                    if !out.contains(&sym) {
                        out.push(sym);
                    }
                }
            }
            MemmapEntry::MacroUse(mu) => {
                if let MacroUseState::Expanded(exps) = &mu.state {
                    for exp in exps {
                        collect_glob_matches(
                            db,
                            current_module,
                            source_root,
                            crate_root,
                            &exp.entries,
                            name,
                            ns,
                            out,
                        );
                    }
                }
            }
            _ => {}
        }
    }
}

/// Resolve a path (from a use-redirect) to a Symbol, using the MEM-map
/// of each intermediate module so that macro-introduced names in those
/// modules are visible.
pub fn resolve_path_to_symbol<'db>(
    db: &'db dyn Db,
    current_module: Module<'db>,
    source_root: SourceRoot,
    crate_root: Module<'db>,
    path: Path<'db>,
) -> Result<Symbol<'db>, ResolutionError> {
    // Cycle-break: short-circuit if already resolving this (module, path).
    let Some(_guard) = FrameGuard::enter(
        salsa::plumbing::AsId::as_id(&current_module),
        salsa::plumbing::AsId::as_id(&path),
        FRAME_PATH_SYMBOL,
    ) else {
        return Err(ResolutionError::Unresolved);
    };

    let segments = path.segments(db);
    if segments.is_empty() {
        return Err(ResolutionError::Unresolved);
    }

    let (first_module, rest) =
        first_segment_module_via_memmap(db, current_module, source_root, crate_root, segments)
            .ok_or(ResolutionError::Unresolved)?;

    let mut current = first_module;
    for (i, seg) in rest.iter().enumerate() {
        let is_last = i == rest.len() - 1;
        let seg_ns = if is_last {
            Namespace::Type
        } else {
            Namespace::Type
        };
        let sym = definition_via_memmap(db, current, source_root, crate_root, *seg, seg_ns)
            .or_else(|| {
                if is_last {
                    // Try Value namespace for `use foo::FUNC` / `use foo::CONST`.
                    definition_via_memmap(
                        db,
                        current,
                        source_root,
                        crate_root,
                        *seg,
                        Namespace::Value,
                    )
                    .or_else(|| {
                        // Try Macro namespace for `use foo::my_macro`.
                        definition_via_memmap(
                            db,
                            current,
                            source_root,
                            crate_root,
                            *seg,
                            Namespace::Macro(MacroKind::Bang),
                        )
                    })
                } else {
                    None
                }
            })
            .ok_or(ResolutionError::Unresolved)?;
        if !is_last {
            current = symbol_to_module(db, sym, source_root, current)
                .ok_or(ResolutionError::Unresolved)?;
        } else {
            return Ok(sym);
        }
    }

    // If rest is empty, the path is just the first segment module.
    match first_module.source(db) {
        ModuleSource::External(cn, di) => Ok(Symbol::new(db, SymbolSource::External(cn, di))),
        _ => Err(ResolutionError::Unresolved),
    }
}

// ---------------------------------------------------------------------------
// Cycle detection for MEM-map-aware path walking.
// ---------------------------------------------------------------------------

thread_local! {
    /// In-flight resolution frames. Each frame is `(module_id,
    /// path_or_name_id, kind)` — duplicates short-circuit to None,
    /// preventing infinite recursion through cyclic globs/redirects.
    static IN_FLIGHT: std::cell::RefCell<Vec<(salsa::Id, salsa::Id, u8)>> =
        const { std::cell::RefCell::new(Vec::new()) };
}

const FRAME_DEFINITION: u8 = 0;
const FRAME_PATH_SYMBOL: u8 = 1;
const FRAME_PATH_MODULE: u8 = 2;

/// RAII guard that pushes a resolution frame on entry and pops on drop.
/// The constructor returns `None` if the frame is already in flight —
/// caller should treat that as the cycle base case and return their
/// empty/None value.
struct FrameGuard {
    active: bool,
}

impl FrameGuard {
    fn enter(mod_id: salsa::Id, path_id: salsa::Id, kind: u8) -> Option<Self> {
        let cycle = IN_FLIGHT.with(|cell| {
            let mut borrowed = cell.borrow_mut();
            if borrowed.contains(&(mod_id, path_id, kind)) {
                true
            } else {
                borrowed.push((mod_id, path_id, kind));
                false
            }
        });
        if cycle {
            None
        } else {
            Some(FrameGuard { active: true })
        }
    }
}

impl Drop for FrameGuard {
    fn drop(&mut self) {
        if self.active {
            IN_FLIGHT.with(|cell| {
                cell.borrow_mut().pop();
            });
        }
    }
}

/// Like `definition`, but consults the module's MEM-map (flattened
/// through expansions) rather than `module_items`. This makes names
/// introduced by macro expansion visible during path walking.
///
/// Returns the single matching symbol, or `None` if zero or multiple
/// matches exist. Used for intermediate segments of a path walk — we
/// can't disambiguate here, so we conservatively return None on
/// ambiguity and let the caller flag it.
fn definition_via_memmap<'db>(
    db: &'db dyn Db,
    module: Module<'db>,
    source_root: SourceRoot,
    crate_root: Module<'db>,
    name: Name<'db>,
    ns: Namespace,
) -> Option<Symbol<'db>> {
    use crate::memmap::{MacroUseState, MemmapEntry, module_memmap};

    // Cycle-break: if we're already resolving (module, name) further
    // up the stack, bottom out with None. The name/ns pair is encoded
    // into `name`'s salsa id + namespace discriminant via the frame
    // kind, so different namespaces are distinct frames.
    let frame_kind = match ns {
        Namespace::Type => 0,
        Namespace::Value => 1,
        Namespace::Macro(MacroKind::Bang) => 2,
        Namespace::Macro(MacroKind::Attr) => 3,
        Namespace::Macro(MacroKind::Derive) => 4,
    };
    let _guard = FrameGuard::enter(
        salsa::plumbing::AsId::as_id(&module),
        salsa::plumbing::AsId::as_id(&name),
        FRAME_DEFINITION + frame_kind,
    )?;

    if matches!(module.source(db), ModuleSource::External(..)) {
        return definition_in_ns(db, module, name, ns);
    }

    let memmap = module_memmap(db, module, source_root, crate_root);
    let mut named: Vec<Symbol<'db>> = Vec::new();
    let mut glob_matches: Vec<Symbol<'db>> = Vec::new();

    fn walk<'db>(
        db: &'db dyn Db,
        module: Module<'db>,
        source_root: SourceRoot,
        crate_root: Module<'db>,
        entries: &[MemmapEntry<'db>],
        name: Name<'db>,
        ns: Namespace,
        named: &mut Vec<Symbol<'db>>,
        glob_matches: &mut Vec<Symbol<'db>>,
    ) {
        for entry in entries {
            match entry {
                MemmapEntry::Item(item) => {
                    if item_name(db, *item) == Some(name) && item_in_namespace(db, *item, ns) {
                        let sym = Symbol::new(db, SymbolSource::Local(*item));
                        if !named.contains(&sym) {
                            named.push(sym);
                        }
                    }
                }
                MemmapEntry::MacroDef(def) => {
                    if def.name(db) == name && matches!(ns, Namespace::Macro(_)) {
                        // No Symbol for local MacroDef yet; skip.
                        let _ = def;
                    }
                }
                MemmapEntry::Redirect { name: n, target } => {
                    if *n == name {
                        if let Ok(sym) =
                            resolve_path_to_symbol(db, module, source_root, crate_root, *target)
                        {
                            if !named.contains(&sym) {
                                named.push(sym);
                            }
                        }
                    }
                }
                MemmapEntry::Glob { path } => {
                    let Some(target) = resolve_use_path_to_module_from_path(
                        db,
                        module,
                        source_root,
                        crate_root,
                        *path,
                    ) else {
                        continue;
                    };
                    // Use `definition_via_memmap` for locals so that
                    // glob targets see macro-expanded names in the
                    // target's MEM-map. For externals, fall back to
                    // `definition_in_ns` (tcx-backed).
                    let sym = match target.source(db) {
                        ModuleSource::External(..) => definition_in_ns(db, target, name, ns),
                        _ => definition_via_memmap(db, target, source_root, crate_root, name, ns),
                    };
                    if let Some(sym) = sym {
                        if !glob_matches.contains(&sym) {
                            glob_matches.push(sym);
                        }
                    }
                }
                MemmapEntry::MacroUse(mu) => {
                    if let MacroUseState::Expanded(exps) = &mu.state {
                        for exp in exps {
                            walk(
                                db,
                                module,
                                source_root,
                                crate_root,
                                &exp.entries,
                                name,
                                ns,
                                named,
                                glob_matches,
                            );
                        }
                    }
                }
            }
        }
    }

    walk(
        db,
        module,
        source_root,
        crate_root,
        memmap.entries(db),
        name,
        ns,
        &mut named,
        &mut glob_matches,
    );

    match named.len() {
        1 => Some(named[0]),
        0 => {
            if glob_matches.len() == 1 {
                Some(glob_matches[0])
            } else {
                None
            }
        }
        _ => None,
    }
}

/// Resolve a name in the std prelude (`std::prelude::v1`).
///
/// This is the implicit `use std::prelude::v1::*` that Rust injects at the
/// crate root. We walk `std` → `prelude` → `v1` via TcxDb and search for
/// the name among its children, filtering by namespace.
fn resolve_in_std_prelude<'db>(
    db: &'db dyn Db,
    name: Name<'db>,
    ns: Namespace,
) -> Option<Symbol<'db>> {
    let std_crate = db.tcx().extern_crate("std")?;
    let std_root = Module::new(
        db,
        ModuleSource::External(std_crate, crate::module::DefIndex(0)),
    );

    // Walk std → prelude → v1
    let prelude_name = Name::new(db, "prelude".to_owned());
    let prelude_sym = definition(db, std_root, prelude_name)?;
    let dummy_root = SourceRoot::new(db, Vec::new());
    let prelude_mod = symbol_to_module(db, prelude_sym, dummy_root, std_root)?;

    let v1_name = Name::new(db, "v1".to_owned());
    let v1_sym = definition(db, prelude_mod, v1_name)?;
    let v1_mod = symbol_to_module(db, v1_sym, dummy_root, prelude_mod)?;

    // Search v1's children with namespace filtering
    let ModuleSource::External(cn, di) = v1_mod.source(db) else {
        return None;
    };
    let raw = db.tcx().module_children(cn, di);
    let name_text = name.text(db);
    raw.into_iter()
        .find(|c| c.name == *name_text && c.namespace == ns)
        .map(|c| Symbol::new(db, SymbolSource::External(c.crate_num, c.def_index)))
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
        ModuleSource::Local { .. } | ModuleSource::LocalInline { .. } => {
            // Find the item that corresponds to this module in the parent
            Err(ResolutionError::Unresolved)
        }
        ModuleSource::External(cn, di) => Ok(Symbol::new(db, SymbolSource::External(cn, di))),
    }
}

/// Resolve a use import's path to a Module (for glob imports).
pub fn resolve_use_path_to_module<'db>(
    db: &'db dyn Db,
    current_module: Module<'db>,
    source_root: SourceRoot,
    crate_root: Module<'db>,
    import: UseImport<'db>,
) -> Result<Module<'db>, ResolutionError> {
    resolve_use_path_to_module_from_path(
        db,
        current_module,
        source_root,
        crate_root,
        import.path(db),
    )
    .ok_or(ResolutionError::Unresolved)
}

/// Resolve a path (e.g. from a stored glob `MemmapEntry`) to a Module,
/// or return None if it doesn't resolve. Used at MEM-map lookup time to
/// resolve glob targets lazily, so globs whose target is created by
/// macro expansion resolve correctly.
///
/// This is the **post-construction** variant — it walks path segments
/// via the memmap, so macro-introduced inline modules are visible as
/// path roots. For **construction-time** callers (inside
/// `module_memmap`), use `resolve_use_path_to_module_from_path_ctime`
/// instead to avoid re-entering `module_memmap` on the current module.
pub fn resolve_use_path_to_module_from_path<'db>(
    db: &'db dyn Db,
    current_module: Module<'db>,
    source_root: SourceRoot,
    crate_root: Module<'db>,
    path: Path<'db>,
) -> Option<Module<'db>> {
    // Cycle-break.
    let _guard = FrameGuard::enter(
        salsa::plumbing::AsId::as_id(&current_module),
        salsa::plumbing::AsId::as_id(&path),
        FRAME_PATH_MODULE,
    )?;

    let segments = path.segments(db);
    if segments.is_empty() {
        return None;
    }

    let (first_module, rest) =
        first_segment_module_via_memmap(db, current_module, source_root, crate_root, segments)?;

    let mut current = first_module;
    for seg in rest {
        let sym =
            definition_via_memmap(db, current, source_root, crate_root, *seg, Namespace::Type)?;
        current = symbol_to_module(db, sym, source_root, current)?;
    }
    Some(current)
}

/// Construction-time variant of `resolve_use_path_to_module_from_path`:
/// walks path segments via `definition` (file_item_tree-backed) rather
/// than the MEM-map, so it's safe to call from inside `module_memmap`
/// without re-entering the current module's query.
///
/// Trades off: it can't resolve paths through macro-introduced inline
/// modules. Used by ctime glob/redirect fallback resolution where this
/// is acceptable (and usually file-based target modules are what's
/// involved anyway).
pub fn resolve_use_path_to_module_from_path_ctime<'db>(
    db: &'db dyn Db,
    current_module: Module<'db>,
    source_root: SourceRoot,
    crate_root: Module<'db>,
    path: Path<'db>,
) -> Option<Module<'db>> {
    let segments = path.segments(db);
    if segments.is_empty() {
        return None;
    }

    let (first_module, rest) =
        resolve_first_segment(db, current_module, source_root, crate_root, segments).ok()?;

    let mut current = first_module;
    for seg in rest {
        let sym = definition(db, current, *seg)?;
        current = symbol_to_module(db, sym, source_root, current)?;
    }
    Some(current)
}

/// A variant of `resolve_first_segment` that consults the MEM-map for
/// bare-identifier first segments, so that names introduced by macro
/// expansion (including `LocalInline` modules) can be found.
///
/// This is separate from `resolve_first_segment` because the latter is
/// used by construction-time callers where re-entering `module_memmap`
/// would be unsafe; the memmap-aware variant is used only by
/// post-construction path walkers.
fn first_segment_module_via_memmap<'db>(
    db: &'db dyn Db,
    current_module: Module<'db>,
    source_root: SourceRoot,
    crate_root: Module<'db>,
    segments: &'db [Name<'db>],
) -> Option<(Module<'db>, &'db [Name<'db>])> {
    let first = segments[0];
    let first_text = first.text(db);
    let rest = &segments[1..];

    match first_text.as_str() {
        "" => {
            if rest.is_empty() {
                return None;
            }
            let crate_name = rest[0].text(db);
            let cn = db.tcx().extern_crate(crate_name)?;
            let ext_mod = Module::new(db, ModuleSource::External(cn, crate::module::DefIndex(0)));
            Some((ext_mod, &rest[1..]))
        }
        "crate" => Some((crate_root, rest)),
        "self" => Some((current_module, rest)),
        "super" => {
            let parent = match current_module.source(db) {
                ModuleSource::Local {
                    parent: Some(p), ..
                } => p,
                ModuleSource::LocalInline { parent, .. } => parent,
                _ => return None,
            };
            Some((parent, rest))
        }
        _ => {
            // Try the current module's MEM-map (covers macro-introduced
            // inline modules via `LocalInline`).
            if let Some(sym) = definition_via_memmap(
                db,
                current_module,
                source_root,
                crate_root,
                first,
                Namespace::Type,
            ) {
                if let Some(child_mod) = symbol_to_module(db, sym, source_root, current_module) {
                    return Some((child_mod, rest));
                }
            }
            // Extern prelude.
            if let Some(cn) = db.tcx().extern_crate(first_text) {
                let ext_mod =
                    Module::new(db, ModuleSource::External(cn, crate::module::DefIndex(0)));
                return Some((ext_mod, rest));
            }
            None
        }
    }
}

/// Resolve the first segment of a use path.
/// Returns (module to search in, remaining segments).
pub(crate) fn resolve_first_segment<'db>(
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
            let parent = match current_module.source(db) {
                ModuleSource::Local {
                    parent: Some(p), ..
                } => p,
                ModuleSource::LocalInline { parent, .. } => parent,
                _ => return Err(ResolutionError::Unresolved),
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
pub(crate) fn symbol_to_module<'db>(
    db: &'db dyn Db,
    sym: Symbol<'db>,
    source_root: SourceRoot,
    parent: Module<'db>,
) -> Option<Module<'db>> {
    match sym.source(db) {
        SymbolSource::Local(Item::Mod(mod_item)) => {
            // resolve_mod handles both inline and file-based cases:
            // inline → LocalInline, file-based → Local.
            resolve_mod(db, parent, mod_item, source_root)
        }
        SymbolSource::External(crate_num, def_index) => Some(Module::new(
            db,
            ModuleSource::External(crate_num, def_index),
        )),
        _ => None,
    }
}
