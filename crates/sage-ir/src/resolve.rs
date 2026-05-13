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
    match item {
        // Structs live in both Type (as a type name) and Value (as a
        // constructor or unit value).
        Item::Struct(_) => matches!(ns, Namespace::Type | Namespace::Value),

        // Type-namespace-only items.
        Item::Enum(_) | Item::Trait(_) | Item::TypeAlias(_) | Item::Mod(_) => {
            matches!(ns, Namespace::Type)
        }

        // Value-namespace-only items.
        Item::Function(_) | Item::Const(_) | Item::Static(_) => matches!(ns, Namespace::Value),

        // `macro_rules!` definitions live in the bang-macro namespace.
        // Not observable in practice — seeding routes these to
        // `MemmapEntry::MacroDef`, not `MemmapEntry::Item` — but we
        // answer correctly here for exhaustiveness.
        Item::MacroDef(_) => matches!(ns, Namespace::Macro(MacroKind::Bang)),

        // Items with no name: they're never looked up by name, so
        // they don't occupy any namespace.
        Item::Impl(_) | Item::Use(_) | Item::MacroInvocation(_) | Item::Error(_) => false,
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
        let sym = current
            .resolve_member(db, source_root, name, Namespace::Type)
            .ok()
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

/// Resolve a name in a module's scope.
///
/// Layers `Module::resolve_member` (which handles the module's own
/// contents — items, redirects, globs, with named > glob priority)
/// with extern-prelude and std-prelude fallbacks.
///
/// Priority (highest to lowest):
/// 1. Members of `module` (items, named redirects, expansion-introduced names)
/// 2. Glob-imported names (recursively, via `Module::resolve_member`)
/// 3. Extern prelude (via `tcx.extern_crate`)
/// 4. Std prelude (implicit `use std::prelude::v1::*`)
///
/// Ambiguity in (1) or (2) propagates as `Err(Ambiguous)`.
pub fn resolve_name<'db>(
    db: &'db dyn Db,
    module: Module<'db>,
    source_root: SourceRoot,
    name: Name<'db>,
    ns: Namespace,
) -> Result<Symbol<'db>, ResolutionError> {
    // 1 + 2: the module's own members (named > glob handled inside).
    match module.resolve_member(db, source_root, name, ns) {
        Ok(sym) => return Ok(sym),
        Err(ResolutionError::Ambiguous) => return Err(ResolutionError::Ambiguous),
        Err(ResolutionError::Unresolved) => {}
    }

    // 3. Extern prelude.
    if let Some(crate_num) = db.tcx().extern_crate(name.text(db)) {
        return Ok(Symbol::new(
            db,
            SymbolSource::External(crate_num, crate::module::DefIndex(0)),
        ));
    }

    // 4. Std prelude.
    resolve_in_std_prelude(db, name, ns).ok_or(ResolutionError::Unresolved)
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

/// Inherent resolution methods on `Module`.
///
/// These are the user-facing entry point for asking "what does this
/// name refer to in this module?". They consult the MEM-map for local
/// modules and `TcxDb` for external modules, so macro-introduced names
/// are visible uniformly.
///
/// These methods do NOT apply extern/std prelude fallback — preludes
/// are a crate-root concern handled by the top-level path walker.
impl<'db> Module<'db> {
    /// Resolve a name in this module's direct contents.
    ///
    /// - Local / LocalInline: walks the MEM-map (flattened through
    ///   `MacroUse::Expanded` branches). Named entries beat globs.
    /// - External: consults `TcxDb::module_children`.
    ///
    /// Returns `Err(Ambiguous)` if the name has more than one match
    /// (whether among named entries or among glob candidates).
    /// Returns `Err(Unresolved)` on zero matches or when a cycle
    /// short-circuit prevents progress.
    pub fn resolve_member(
        self,
        db: &'db dyn Db,
        source_root: SourceRoot,
        name: Name<'db>,
        ns: Namespace,
    ) -> Result<Symbol<'db>, ResolutionError> {
        use crate::memmap::{MacroUseState, MemmapEntry, module_memmap};

        let frame_kind = match ns {
            Namespace::Type => 0,
            Namespace::Value => 1,
            Namespace::Macro(MacroKind::Bang) => 2,
            Namespace::Macro(MacroKind::Attr) => 3,
            Namespace::Macro(MacroKind::Derive) => 4,
        };
        let Some(_guard) = FrameGuard::enter(
            salsa::plumbing::AsId::as_id(&self),
            salsa::plumbing::AsId::as_id(&name),
            FRAME_DEFINITION + frame_kind,
        ) else {
            return Err(ResolutionError::Unresolved);
        };

        match self.source(db) {
            ModuleSource::External(..) => {
                return definition_in_ns(db, self, name, ns).ok_or(ResolutionError::Unresolved);
            }
            ModuleSource::Local { .. } | ModuleSource::LocalInline { .. } => {}
        }

        let memmap = module_memmap(db, self, source_root);
        let mut named: Vec<Symbol<'db>> = Vec::new();
        let mut glob_matches: Vec<Symbol<'db>> = Vec::new();

        fn walk<'db>(
            db: &'db dyn Db,
            module: Module<'db>,
            source_root: SourceRoot,
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
                            // Respect the caller's requested namespace.
                            if let Ok(sym) = module.resolve_path(db, source_root, *target, ns) {
                                if !named.contains(&sym) {
                                    named.push(sym);
                                }
                            }
                        }
                    }
                    MemmapEntry::Glob { path } => {
                        // Glob target is a module path.
                        let Ok(target) = module.resolve_path_to_module(db, source_root, *path)
                        else {
                            continue;
                        };
                        // Look up the requested name inside the glob target.
                        let sym = target.resolve_member(db, source_root, name, ns).ok();
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
            self,
            source_root,
            memmap.entries(db),
            name,
            ns,
            &mut named,
            &mut glob_matches,
        );

        match named.len() {
            1 => Ok(named[0]),
            0 => match glob_matches.len() {
                0 => Err(ResolutionError::Unresolved),
                1 => Ok(glob_matches[0]),
                _ => Err(ResolutionError::Ambiguous),
            },
            _ => Err(ResolutionError::Ambiguous),
        }
    }

    /// Walk a path starting from this module and return the symbol it
    /// resolves to.
    ///
    /// First-segment dispatch handles the usual path prefixes:
    /// `crate`, `self`, `super`, leading `::` (extern prelude), and
    /// bare identifiers (which try `resolve_member` first, then fall
    /// back to extern prelude).
    ///
    /// Every subsequent segment is resolved via `resolve_member`, with
    /// `Namespace::Type` for intermediate segments (they must name a
    /// module) and `final_ns` for the terminal segment.
    ///
    /// Cycle-safe via the shared `FrameGuard` stack. Meant for
    /// post-construction callers — construction-time code should use
    /// `resolve_use_path_to_module_from_path_ctime` instead, which
    /// walks via `definition` to avoid re-entering the current
    /// module's `module_memmap` query.
    pub fn resolve_path(
        self,
        db: &'db dyn Db,
        source_root: SourceRoot,
        path: Path<'db>,
        final_ns: Namespace,
    ) -> Result<Symbol<'db>, ResolutionError> {
        // Cycle-break: short-circuit if already resolving this (module, path).
        let Some(_guard) = FrameGuard::enter(
            salsa::plumbing::AsId::as_id(&self),
            salsa::plumbing::AsId::as_id(&path),
            FRAME_PATH_SYMBOL,
        ) else {
            return Err(ResolutionError::Unresolved);
        };

        let segments = path.segments(db);
        if segments.is_empty() {
            return Err(ResolutionError::Unresolved);
        }

        let (first_module, rest) = self.dispatch_first_segment(db, source_root, segments)?;

        if rest.is_empty() {
            // Path was purely a prefix (`crate`, `self`, `super`, or a
            // single extern-crate name). Only extern modules have a
            // Symbol we can return directly; locals fall through to Err.
            // Callers that want the module itself (e.g. glob targets)
            // should use `resolve_path_to_module` instead.
            return match first_module.source(db) {
                ModuleSource::External(cn, di) => {
                    Ok(Symbol::new(db, SymbolSource::External(cn, di)))
                }
                _ => Err(ResolutionError::Unresolved),
            };
        }

        let mut current = first_module;
        for (i, seg) in rest.iter().enumerate() {
            let is_last = i == rest.len() - 1;
            let seg_ns = if is_last { final_ns } else { Namespace::Type };
            let sym = current.resolve_member(db, source_root, *seg, seg_ns)?;
            if !is_last {
                current = symbol_to_module(db, sym, source_root, current)
                    .ok_or(ResolutionError::Unresolved)?;
            } else {
                return Ok(sym);
            }
        }
        unreachable!("rest is non-empty and loop always returns on is_last")
    }

    /// Walk a path starting from this module and return the module it
    /// resolves to.
    ///
    /// Like `resolve_path`, but always treats the terminal as a module
    /// rather than trying to produce a terminal `Symbol`. Accepts
    /// "prefix only" paths (`crate`, `self`, `super`, single extern
    /// crate name) by returning the prefix module directly. Used by
    /// glob-target resolution, where the path spelled in
    /// `use foo::*` resolves to a *module*, not a value/type.
    pub fn resolve_path_to_module(
        self,
        db: &'db dyn Db,
        source_root: SourceRoot,
        path: Path<'db>,
    ) -> Result<Module<'db>, ResolutionError> {
        let Some(_guard) = FrameGuard::enter(
            salsa::plumbing::AsId::as_id(&self),
            salsa::plumbing::AsId::as_id(&path),
            FRAME_PATH_SYMBOL,
        ) else {
            return Err(ResolutionError::Unresolved);
        };

        let segments = path.segments(db);
        if segments.is_empty() {
            return Err(ResolutionError::Unresolved);
        }

        let (first_module, rest) = self.dispatch_first_segment(db, source_root, segments)?;

        let mut current = first_module;
        for seg in rest {
            let sym = current.resolve_member(db, source_root, *seg, Namespace::Type)?;
            current = symbol_to_module(db, sym, source_root, current)
                .ok_or(ResolutionError::Unresolved)?;
        }
        Ok(current)
    }

    /// Dispatch the first segment of a path. Returns the module to
    /// start from plus the remaining segments to walk. Shared by
    /// `resolve_path` and `resolve_path_to_module`.
    fn dispatch_first_segment(
        self,
        db: &'db dyn Db,
        source_root: SourceRoot,
        segments: &'db [Name<'db>],
    ) -> Result<(Module<'db>, &'db [Name<'db>]), ResolutionError> {
        let first = segments[0];
        let first_text = first.text(db);
        let rest = &segments[1..];

        match first_text.as_str() {
            "" => {
                // Leading `::` — extern prelude only, consumes both
                // the empty segment and the crate name.
                if rest.is_empty() {
                    return Err(ResolutionError::Unresolved);
                }
                let crate_name = rest[0].text(db);
                let cn = db
                    .tcx()
                    .extern_crate(crate_name)
                    .ok_or(ResolutionError::Unresolved)?;
                let ext_mod =
                    Module::new(db, ModuleSource::External(cn, crate::module::DefIndex(0)));
                Ok((ext_mod, &rest[1..]))
            }
            "crate" => Ok((self.crate_root(db), rest)),
            "self" => Ok((self, rest)),
            "super" => self
                .parent(db)
                .map(|p| (p, rest))
                .ok_or(ResolutionError::Unresolved),
            _ => {
                // Bare identifier: try the current module first, then
                // fall back to extern prelude.
                if let Ok(sym) = self.resolve_member(db, source_root, first, Namespace::Type) {
                    if let Some(child_mod) = symbol_to_module(db, sym, source_root, self) {
                        return Ok((child_mod, rest));
                    }
                    // A non-module symbol is only meaningful if this
                    // was the whole path; the caller handles that.
                }
                if let Some(cn) = db.tcx().extern_crate(first_text) {
                    let ext_mod =
                        Module::new(db, ModuleSource::External(cn, crate::module::DefIndex(0)));
                    return Ok((ext_mod, rest));
                }
                Err(ResolutionError::Unresolved)
            }
        }
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
pub fn resolve_use_path<'db>(
    db: &'db dyn Db,
    current_module: Module<'db>,
    source_root: SourceRoot,
    import: UseImport<'db>,
) -> Result<Symbol<'db>, ResolutionError> {
    let segments = import.path(db).segments(db);
    if segments.is_empty() {
        return Err(ResolutionError::Unresolved);
    }

    let (first_module, rest) = resolve_first_segment(db, current_module, source_root, segments)?;

    let mut current = first_module;
    for (i, seg) in rest.iter().enumerate() {
        let sym = definition(db, current, *seg).ok_or(ResolutionError::Unresolved)?;
        if i < rest.len() - 1 {
            current = symbol_to_module(db, sym, source_root, current)
                .ok_or(ResolutionError::Unresolved)?;
        } else {
            return Ok(sym);
        }
    }

    match first_module.source(db) {
        ModuleSource::Local { .. } | ModuleSource::LocalInline { .. } => {
            Err(ResolutionError::Unresolved)
        }
        ModuleSource::External(cn, di) => Ok(Symbol::new(db, SymbolSource::External(cn, di))),
    }
}

/// Resolve a path (e.g. from a stored glob `MemmapEntry`) to a Module,
/// or return None if it doesn't resolve. Post-construction variant —
/// walks path segments via the memmap, so macro-introduced inline
/// modules are visible as path roots.
///
/// Thin wrapper around `Module::resolve_path_to_module`. Kept as a
/// free function because the validator finds the `Option<Module>`
/// return type more ergonomic than `Result<Module, _>`.
pub fn resolve_use_path_to_module_from_path<'db>(
    db: &'db dyn Db,
    current_module: Module<'db>,
    source_root: SourceRoot,
    path: Path<'db>,
) -> Option<Module<'db>> {
    current_module
        .resolve_path_to_module(db, source_root, path)
        .ok()
}

/// Construction-time variant: walks path segments via `definition`
/// (file_item_tree-backed) rather than the MEM-map, so it's safe to
/// call from inside `module_memmap` without re-entering the current
/// module's query.
pub fn resolve_use_path_to_module_from_path_ctime<'db>(
    db: &'db dyn Db,
    current_module: Module<'db>,
    source_root: SourceRoot,
    path: Path<'db>,
) -> Option<Module<'db>> {
    let segments = path.segments(db);
    if segments.is_empty() {
        return None;
    }

    let (first_module, rest) =
        resolve_first_segment(db, current_module, source_root, segments).ok()?;

    let mut current = first_module;
    for seg in rest {
        let sym = definition(db, current, *seg)?;
        current = symbol_to_module(db, sym, source_root, current)?;
    }
    Some(current)
}

/// Resolve the first segment of a use path.
/// Returns (module to search in, remaining segments).
pub(crate) fn resolve_first_segment<'db>(
    db: &'db dyn Db,
    current_module: Module<'db>,
    source_root: SourceRoot,
    segments: &'db [Name<'db>],
) -> Result<(Module<'db>, &'db [Name<'db>]), ResolutionError> {
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
                let ext_mod = Module::new(
                    db,
                    ModuleSource::External(crate_num, crate::module::DefIndex(0)),
                );
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
            if let Some(sym) = definition(db, current_module, first) {
                if let Some(child_mod) = symbol_to_module(db, sym, source_root, current_module) {
                    return Ok((child_mod, rest));
                }
                if rest.is_empty() {
                    // non-module terminal — caller handles
                }
            }
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
