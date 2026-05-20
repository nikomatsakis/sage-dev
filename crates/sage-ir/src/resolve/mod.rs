use crate::Db;
use crate::item::*;
use crate::module::{ModExt, ModSymbol, ModSymbolData};
use crate::name::Name;
use crate::source::SourceFile;
use crate::symbol::{Intrinsic, SymExt, Symbol, SymbolData};
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
pub(crate) fn item_in_namespace(_db: &dyn Db, item: ItemAst<'_>, ns: Namespace) -> bool {
    match item {
        // Structs live in Type only. The value-namespace entry for
        // tuple/unit structs is handled by `MemmapEntry::TupleStructCtor`.
        ItemAst::Struct(_) => matches!(ns, Namespace::Type),

        // Type-namespace-only items.
        ItemAst::Enum(_) | ItemAst::Trait(_) | ItemAst::TypeAlias(_) | ItemAst::Mod(_) => {
            matches!(ns, Namespace::Type)
        }

        // Value-namespace-only items.
        ItemAst::Function(_) | ItemAst::Const(_) | ItemAst::Static(_) => {
            matches!(ns, Namespace::Value)
        }

        // `macro_rules!` definitions live in the bang-macro namespace.
        ItemAst::MacroDef(_) => matches!(ns, Namespace::Macro(MacroKind::Bang)),

        // Items with no name: they're never looked up by name, so
        // they don't occupy any namespace.
        ItemAst::Impl(_) | ItemAst::Use(_) | ItemAst::MacroInvocation(_) | ItemAst::Error(..) => {
            false
        }
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

/// Format a module for logging.
fn module_label(db: &dyn Db, module: ModSymbol<'_>) -> String {
    match module.data() {
        ModSymbolData::Ast(ast) => {
            match (ast.file(db), ast.inline_unexpanded_items(db).is_some()) {
                (Some(f), _) => format!("\"{}\"", f.path(db)),
                (None, true) => format!("inline \"{}\"", ast.name(db).text(db)),
                (None, false) => format!("decl \"{}\"", ast.name(db).text(db)),
            }
        }
        ModSymbolData::Ext(ext) => format!("extern({}, {})", ext.crate_num.0, ext.def_index.0),
    }
}

/// Use imports declared in a local module.
fn local_module_use_imports<'db>(db: &'db dyn Db, ast: ModAst<'db>) -> Vec<UseImport<'db>> {
    let items = ast.unexpanded_items(db);
    collect_use_imports(db, &items)
}

fn collect_use_imports<'db>(db: &'db dyn Db, items: &[ItemAst<'db>]) -> Vec<UseImport<'db>> {
    let mut imports = Vec::new();
    for item in items {
        if let ItemAst::Use(group) = item {
            imports.extend_from_slice(group.imports(db));
        }
    }
    imports
}

/// Find a direct child definition by name.
pub fn definition<'db>(
    db: &'db dyn Db,
    module: ModSymbol<'db>,
    name: Name<'db>,
) -> Option<Symbol<'db>> {
    db.log_query(format!(
        "definition({}, \"{}\")",
        module_label(db, module),
        name.text(db)
    ));
    match module.data() {
        ModSymbolData::Ast(ast) => {
            for item in ast.unexpanded_items(db) {
                if item_name(db, item) == Some(name) {
                    return Some(Symbol::ast(item));
                }
            }
            None
        }
        ModSymbolData::Ext(ext) => {
            let raw = db.tcx().module_children(ext.crate_num, ext.def_index);
            let name_text = name.text(db);
            raw.into_iter()
                .find(|c| c.name == *name_text)
                .map(|c| Symbol::external(c.crate_num, c.def_index))
        }
    }
}

/// Like `definition`, but filters by namespace for external modules.
/// For local modules, behaves like `definition` (no namespace filter on items).
pub fn definition_in_ns<'db>(
    db: &'db dyn Db,
    module: ModSymbol<'db>,
    name: Name<'db>,
    ns: Namespace,
) -> Option<Symbol<'db>> {
    match module.data() {
        ModSymbolData::Ast(_) => definition(db, module, name),
        ModSymbolData::Ext(ext) => {
            let raw = db.tcx().module_children(ext.crate_num, ext.def_index);
            let name_text = name.text(db);
            raw.into_iter()
                .find(|c| c.name == *name_text && c.namespace == ns)
                .map(|c| Symbol::external(c.crate_num, c.def_index))
        }
    }
}

/// Resolve a `mod foo` declaration ModAst to its resolved ModSymbol.
///
/// For inline modules (`mod foo { ... }`), mints a resolved ModAst
/// that wraps the declaration with parent context (and no `file`).
/// For file-based modules (`mod foo;`), looks up `foo.rs` or
/// `foo/mod.rs` in the source root and mints a ModAst with the
/// resolved file.
pub fn resolve_mod<'db>(
    db: &'db dyn Db,
    parent: ModSymbol<'db>,
    decl: ModAst<'db>,
    source_root: SourceRoot,
) -> Option<ModSymbol<'db>> {
    resolve_mod_tracked(db, parent, decl, source_root).map(ModSymbol::ast)
}

#[salsa::tracked]
fn resolve_mod_tracked<'db>(
    db: &'db dyn Db,
    parent: ModSymbol<'db>,
    decl: ModAst<'db>,
    source_root: SourceRoot,
) -> Option<ModAst<'db>> {
    if let Some(items) = decl.inline_unexpanded_items(db) {
        let inline = ModAst::new(
            db,
            decl.name(db),
            Some(parent),
            None,
            decl.attrs(db).clone(),
            Some(items.clone()),
            decl.span(db),
        );
        return Some(inline);
    }

    let parent_file = parent.containing_file(db)?;
    let parent_path = parent_file.path(db);
    let mod_name = decl.name(db).text(db);
    let parent_dir = parent_dir_for(parent_path);

    let candidates = [
        format!("{parent_dir}{mod_name}.rs"),
        format!("{parent_dir}{mod_name}/mod.rs"),
    ];

    for candidate in &candidates {
        if let Some(child_file) = lookup_source_file(db, source_root, candidate) {
            let resolved = ModAst::new(
                db,
                decl.name(db),
                Some(parent),
                Some(child_file),
                decl.attrs(db).clone(),
                None,
                decl.span(db),
            );
            return Some(resolved);
        }
    }
    None
}

/// Resolve a module path like ["cmd", "get"] to a ModSymbol.
pub fn resolve_module_path<'db>(
    db: &'db dyn Db,
    root: ModSymbol<'db>,
    source_root: SourceRoot,
    segments: &[&str],
) -> Option<ModSymbol<'db>> {
    let mut resolver = Resolver::new(db, source_root);
    resolver.resolve_module_path(root, segments)
}

/// Extract the name from an item, if it has one.
pub fn item_name<'db>(db: &'db dyn Db, item: ItemAst<'db>) -> Option<Name<'db>> {
    match item {
        ItemAst::Function(f) => Some(f.name(db)),
        ItemAst::Struct(s) => Some(s.name(db)),
        ItemAst::Enum(e) => Some(e.name(db)),
        ItemAst::Trait(t) => Some(t.name(db)),
        ItemAst::TypeAlias(t) => Some(t.name(db)),
        ItemAst::Const(c) => Some(c.name(db)),
        ItemAst::Static(s) => Some(s.name(db)),
        ItemAst::Mod(m) => Some(m.name(db)),
        ItemAst::Impl(_)
        | ItemAst::Use(_)
        | ItemAst::MacroDef(_)
        | ItemAst::MacroInvocation(_)
        | ItemAst::Error(..) => None,
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
// Use-imports query (cosmetic — kept as a free helper for tests/log).
// ---------------------------------------------------------------------------

/// Items declared in a module (from `parse_source_file` or the inline
/// items list for local modules; empty for external).
pub fn module_items<'db>(db: &'db dyn Db, module: ModSymbol<'db>) -> Vec<ItemAst<'db>> {
    db.log_query(format!("module_items({})", module_label(db, module)));
    match module.data() {
        ModSymbolData::Ast(ast) => ast.unexpanded_items(db),
        ModSymbolData::Ext(_) => Vec::new(),
    }
}

/// Use imports declared in a module. Empty for external modules.
pub fn module_use_imports<'db>(db: &'db dyn Db, module: ModSymbol<'db>) -> Vec<UseImport<'db>> {
    db.log_query(format!("module_use_imports({})", module_label(db, module)));
    match module.data() {
        ModSymbolData::Ast(ast) => local_module_use_imports(db, ast),
        ModSymbolData::Ext(_) => Vec::new(),
    }
}

// ---------------------------------------------------------------------------
// Name resolution
// ---------------------------------------------------------------------------

/// Resolve a single name in a module's scope.
///
/// Convenience function that creates a temporary `Resolver`. For repeated
/// resolution calls, prefer creating a `Resolver` and calling its methods.
pub fn resolve_name<'db>(
    db: &'db dyn Db,
    module: ModSymbol<'db>,
    source_root: SourceRoot,
    name: Name<'db>,
    ns: Namespace,
) -> Result<Symbol<'db>, ResolutionError> {
    let mut resolver = Resolver::new(db, source_root);
    resolver.resolve_name(module, name, ns)
}

// ---------------------------------------------------------------------------
// Resolver — stateful path resolver with cycle detection
// ---------------------------------------------------------------------------

/// An in-flight member lookup, used to detect cycles through mutual globs.
#[derive(Clone, PartialEq, Eq)]
struct InFlightQuery<'db> {
    module: ModSymbol<'db>,
    name: Name<'db>,
    ns: Namespace,
}

/// Stateful path resolver. Carries cycle-detection state so that mutual
/// glob imports terminate instead of recursing infinitely.
///
/// Create one per top-level resolution request (e.g., per signature lowered
/// or per body resolved). All resolution calls within that scope share the
/// same cycle-detection context.
pub struct Resolver<'db> {
    db: &'db dyn Db,
    source_root: SourceRoot,
    in_flight: Vec<InFlightQuery<'db>>,
}

impl<'db> Resolver<'db> {
    pub fn new(db: &'db dyn Db, source_root: SourceRoot) -> Self {
        Self {
            db,
            source_root,
            in_flight: Vec::new(),
        }
    }

    /// Resolve a single name in a module's scope.
    ///
    /// Full fallback chain: module members → extern prelude → std prelude → intrinsics.
    pub fn resolve_name(
        &mut self,
        module: ModSymbol<'db>,
        name: Name<'db>,
        ns: Namespace,
    ) -> Result<Symbol<'db>, ResolutionError> {
        resolve_plain_name(self, module, name, ns)
    }

    /// Resolve a slice of name segments to a symbol.
    pub fn resolve_segments(
        &mut self,
        module: ModSymbol<'db>,
        segments: &[Name<'db>],
        final_ns: Namespace,
    ) -> Result<Symbol<'db>, ResolutionError> {
        if segments.is_empty() {
            return Err(ResolutionError::Unresolved);
        }

        if segments.len() == 1 {
            return resolve_plain_name(self, module, segments[0], final_ns);
        }

        let (first_module, rest) = dispatch_first_segment(self, module, segments)?;

        if rest.is_empty() {
            return match first_module.data() {
                ModSymbolData::Ext(ext) => Ok(Symbol::ext(SymExt::from(ext))),
                ModSymbolData::Ast(_) => Err(ResolutionError::Unresolved),
            };
        }

        resolve_remainder(self, first_module, rest, final_ns)
    }

    /// Resolve a salsa-interned Path to a symbol.
    pub fn resolve_path(
        &mut self,
        module: ModSymbol<'db>,
        path: Path<'db>,
        final_ns: Namespace,
    ) -> Result<Symbol<'db>, ResolutionError> {
        let segments = path.segments(self.db);
        self.resolve_segments(module, segments, final_ns)
    }

    /// Resolve a slice of name segments to a module.
    pub fn resolve_segments_to_module(
        &mut self,
        module: ModSymbol<'db>,
        segments: &[Name<'db>],
    ) -> Result<ModSymbol<'db>, ResolutionError> {
        if segments.is_empty() {
            return Err(ResolutionError::Unresolved);
        }

        let (first_module, rest) = dispatch_first_segment(self, module, segments)?;
        resolve_remainder_to_module(self, first_module, rest)
    }

    /// Resolve a salsa-interned Path to a module.
    pub fn resolve_path_to_module(
        &mut self,
        module: ModSymbol<'db>,
        path: Path<'db>,
    ) -> Result<ModSymbol<'db>, ResolutionError> {
        let segments = path.segments(self.db);
        self.resolve_segments_to_module(module, segments)
    }

    /// Resolve a name in this module's direct contents (memmap-aware).
    pub fn resolve_member(
        &mut self,
        module: ModSymbol<'db>,
        name: Name<'db>,
        ns: Namespace,
    ) -> Result<Symbol<'db>, ResolutionError> {
        resolve_member_impl(self, module, name, ns)
    }

    /// Resolve a module path from string segments (convenience for tests/debug).
    pub fn resolve_module_path(
        &mut self,
        root: ModSymbol<'db>,
        segments: &[&str],
    ) -> Option<ModSymbol<'db>> {
        let mut current = root;
        for seg_text in segments {
            let name = Name::new(self.db, (*seg_text).to_owned());
            let sym = self
                .resolve_member(current, name, Namespace::Type)
                .ok()
                .or_else(|| definition(self.db, current, name))?;
            current = symbol_to_module(self.db, sym, self.source_root, current)?;
        }
        Some(current)
    }

    fn push_query(&mut self, query: InFlightQuery<'db>) -> bool {
        if self.in_flight.contains(&query) {
            return false;
        }
        self.in_flight.push(query);
        true
    }

    fn pop_query(&mut self) {
        self.in_flight.pop();
    }
}

// ---------------------------------------------------------------------------
// Internal helpers used by Resolver
// ---------------------------------------------------------------------------

fn resolve_member_impl<'db>(
    resolver: &mut Resolver<'db>,
    module: ModSymbol<'db>,
    name: Name<'db>,
    ns: Namespace,
) -> Result<Symbol<'db>, ResolutionError> {
    use crate::memmap::expanded_module;

    let query = InFlightQuery { module, name, ns };
    if !resolver.push_query(query) {
        return Err(ResolutionError::Unresolved);
    }

    let db = resolver.db;
    let source_root = resolver.source_root;

    let result = match module.data() {
        ModSymbolData::Ext(_) => {
            definition_in_ns(db, module, name, ns).ok_or(ResolutionError::Unresolved)
        }
        ModSymbolData::Ast(ast) => {
            let memmap = expanded_module(db, ast, source_root);
            let mut named: Vec<Symbol<'db>> = Vec::new();
            let mut glob_matches: Vec<Symbol<'db>> = Vec::new();

            walk_entries(
                resolver,
                module,
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
    };

    resolver.pop_query();
    result
}

fn walk_entries<'db>(
    resolver: &mut Resolver<'db>,
    module: ModSymbol<'db>,
    entries: &[crate::memmap::MemmapEntry<'db>],
    name: Name<'db>,
    ns: Namespace,
    named: &mut Vec<Symbol<'db>>,
    glob_matches: &mut Vec<Symbol<'db>>,
) {
    use crate::memmap::MemmapEntry;

    let db = resolver.db;

    for entry in entries {
        match entry {
            MemmapEntry::Item(item) => {
                if item_name(db, *item) == Some(name) && item_in_namespace(db, *item, ns) {
                    let sym = Symbol::ast(*item);
                    if !named.contains(&sym) {
                        named.push(sym);
                    }
                }
            }
            MemmapEntry::TupleStructCtor(s) => {
                if s.name(db) == name && matches!(ns, Namespace::Value) {
                    let sym = Symbol::tuple_struct_ctor(*s);
                    if !named.contains(&sym) {
                        named.push(sym);
                    }
                }
            }
            MemmapEntry::MacroDef(def) => {
                if def.name(db) == name && matches!(ns, Namespace::Macro(_)) {
                    let _ = def;
                }
            }
            MemmapEntry::Redirect { name: n, target } => {
                if *n == name {
                    if let Ok(sym) = resolver.resolve_segments(module, target, ns) {
                        if !named.contains(&sym) {
                            named.push(sym);
                        }
                    }
                }
            }
            MemmapEntry::Glob { path } => {
                let Ok(target) = resolver.resolve_segments_to_module(module, path) else {
                    continue;
                };
                let sym = resolver.resolve_member(target, name, ns).ok();
                if let Some(sym) = sym {
                    if !glob_matches.contains(&sym) {
                        glob_matches.push(sym);
                    }
                }
            }
            MemmapEntry::MacroUse(mu) => {
                for exp in &mu.expansions {
                    walk_entries(
                        resolver,
                        module,
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

fn resolve_plain_name<'db>(
    resolver: &mut Resolver<'db>,
    module: ModSymbol<'db>,
    name: Name<'db>,
    ns: Namespace,
) -> Result<Symbol<'db>, ResolutionError> {
    let db = resolver.db;

    match resolver.resolve_member(module, name, ns) {
        Ok(sym) => return Ok(sym),
        Err(ResolutionError::Ambiguous) => return Err(ResolutionError::Ambiguous),
        Err(ResolutionError::Unresolved) => {}
    }

    if let Some(crate_num) = db.tcx().extern_crate(name.text(db)) {
        return Ok(Symbol::external(crate_num, crate::module::DefIndex(0)));
    }

    if let Some(sym) = resolve_in_std_prelude(db, name, ns) {
        return Ok(sym);
    }

    if matches!(ns, Namespace::Type)
        && let Some(intrinsic) = Intrinsic::from_name(name.text(db))
    {
        return Ok(Symbol::intrinsic(intrinsic));
    }

    Err(ResolutionError::Unresolved)
}

fn dispatch_first_segment<'db, 's>(
    resolver: &mut Resolver<'db>,
    module: ModSymbol<'db>,
    segments: &'s [Name<'db>],
) -> Result<(ModSymbol<'db>, &'s [Name<'db>]), ResolutionError> {
    let db = resolver.db;
    let source_root = resolver.source_root;
    let first = segments[0];
    let first_text = first.text(db);
    let rest = &segments[1..];

    match first_text.as_str() {
        "" => {
            if rest.is_empty() {
                return Err(ResolutionError::Unresolved);
            }
            let crate_name = rest[0].text(db);
            let cn = db
                .tcx()
                .extern_crate(crate_name)
                .ok_or(ResolutionError::Unresolved)?;
            let ext_mod = ModSymbol::ext(ModExt::new(cn, crate::module::DefIndex(0)));
            Ok((ext_mod, &rest[1..]))
        }
        "crate" => Ok((module.crate_root(db), rest)),
        "self" => Ok((module, rest)),
        "super" => module
            .parent(db)
            .map(|p| (p, rest))
            .ok_or(ResolutionError::Unresolved),
        _ => {
            if let Ok(sym) = resolver.resolve_member(module, first, Namespace::Type) {
                if let Some(child_mod) = symbol_to_module(db, sym, source_root, module) {
                    return Ok((child_mod, rest));
                }
            }
            if let Some(cn) = db.tcx().extern_crate(first_text) {
                let ext_mod = ModSymbol::ext(ModExt::new(cn, crate::module::DefIndex(0)));
                return Ok((ext_mod, rest));
            }
            Err(ResolutionError::Unresolved)
        }
    }
}

fn resolve_remainder<'db>(
    resolver: &mut Resolver<'db>,
    start: ModSymbol<'db>,
    segments: &[Name<'db>],
    final_ns: Namespace,
) -> Result<Symbol<'db>, ResolutionError> {
    if segments.is_empty() {
        return match start.data() {
            ModSymbolData::Ext(ext) => Ok(Symbol::ext(SymExt::from(ext))),
            ModSymbolData::Ast(_) => Err(ResolutionError::Unresolved),
        };
    }

    let mut current = start;
    for (i, seg) in segments.iter().enumerate() {
        let is_last = i == segments.len() - 1;
        let seg_ns = if is_last { final_ns } else { Namespace::Type };
        let sym = resolver.resolve_member(current, *seg, seg_ns)?;
        if !is_last {
            current = symbol_to_module(resolver.db, sym, resolver.source_root, current)
                .ok_or(ResolutionError::Unresolved)?;
        } else {
            return Ok(sym);
        }
    }
    unreachable!("segments is non-empty and loop always returns on is_last")
}

fn resolve_remainder_to_module<'db>(
    resolver: &mut Resolver<'db>,
    start: ModSymbol<'db>,
    segments: &[Name<'db>],
) -> Result<ModSymbol<'db>, ResolutionError> {
    let mut current = start;
    for seg in segments {
        let sym = resolver.resolve_member(current, *seg, Namespace::Type)?;
        current = symbol_to_module(resolver.db, sym, resolver.source_root, current)
            .ok_or(ResolutionError::Unresolved)?;
    }
    Ok(current)
}

/// Resolve a name in the std prelude (`std::prelude::v1`).
fn resolve_in_std_prelude<'db>(
    db: &'db dyn Db,
    name: Name<'db>,
    ns: Namespace,
) -> Option<Symbol<'db>> {
    let std_crate = db.tcx().extern_crate("std")?;
    let std_root = ModSymbol::external(std_crate, crate::module::DefIndex(0));

    let prelude_name = Name::new(db, "prelude".to_owned());
    let prelude_sym = definition(db, std_root, prelude_name)?;
    let dummy_root = SourceRoot::new(db, Vec::new());
    let prelude_mod = symbol_to_module(db, prelude_sym, dummy_root, std_root)?;

    let v1_name = Name::new(db, "v1".to_owned());
    let v1_sym = definition(db, prelude_mod, v1_name)?;
    let v1_mod = symbol_to_module(db, v1_sym, dummy_root, prelude_mod)?;

    let ext = match v1_mod.data() {
        ModSymbolData::Ext(e) => e,
        ModSymbolData::Ast(_) => return None,
    };
    let raw = db.tcx().module_children(ext.crate_num, ext.def_index);
    let name_text = name.text(db);
    raw.into_iter()
        .find(|c| c.name == *name_text && c.namespace == ns)
        .map(|c| Symbol::external(c.crate_num, c.def_index))
}

/// Post-construction wrapper around `ModSymbol::resolve_path_to_module`.
pub fn resolve_use_path_to_module_from_path<'db>(
    db: &'db dyn Db,
    current_module: ModSymbol<'db>,
    source_root: SourceRoot,
    path: Path<'db>,
) -> Option<ModSymbol<'db>> {
    let mut resolver = Resolver::new(db, source_root);
    resolver.resolve_path_to_module(current_module, path).ok()
}

/// Try to convert a Symbol into a ModSymbol (for walking into child segments).
pub(crate) fn symbol_to_module<'db>(
    db: &'db dyn Db,
    sym: Symbol<'db>,
    source_root: SourceRoot,
    parent: ModSymbol<'db>,
) -> Option<ModSymbol<'db>> {
    match sym.data() {
        SymbolData::Ast(ItemAst::Mod(decl)) => {
            // resolve_mod handles both inline and file-based cases.
            resolve_mod(db, parent, decl, source_root)
        }
        SymbolData::Ext(ext) => {
            if !db.tcx().is_module(ext.crate_num, ext.def_index) {
                return None;
            }
            Some(ModSymbol::external(ext.crate_num, ext.def_index))
        }
        _ => None,
    }
}
