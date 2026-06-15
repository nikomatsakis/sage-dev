use crate::Db;
use crate::local_syms::LocalModItemSym;
use crate::name::Name;
use crate::ribs::Ribs;
use crate::scope::ScopeSymbol;
use crate::source::SourceFile;
use crate::symbol::{Intrinsic, Symbol, SymbolData};
use crate::symbol::{ModSymbol, SymExt};

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
pub(crate) fn item_in_namespace(_db: &dyn Db, item: LocalModItemSym<'_>, ns: Namespace) -> bool {
    match item {
        // Structs live in Type only. The value-namespace entry for
        // tuple/unit structs is handled by `MemmapEntry::TupleStructCtor`.
        LocalModItemSym::Struct(_) => matches!(ns, Namespace::Type),

        // Type-namespace-only items.
        LocalModItemSym::Enum(_)
        | LocalModItemSym::Trait(_)
        | LocalModItemSym::TypeAlias(_)
        | LocalModItemSym::Mod(_) => {
            matches!(ns, Namespace::Type)
        }

        // Value-namespace-only items.
        LocalModItemSym::Function(_) | LocalModItemSym::Const(_) | LocalModItemSym::Static(_) => {
            matches!(ns, Namespace::Value)
        }

        // `macro_rules!` definitions live in the bang-macro namespace.
        LocalModItemSym::MacroDef(_) => matches!(ns, Namespace::Macro(MacroKind::Bang)),

        // Items with no name: they're never looked up by name, so
        // they don't occupy any namespace.
        LocalModItemSym::Impl(_)
        | LocalModItemSym::Use(_)
        | LocalModItemSym::MacroInvocation(_)
        | LocalModItemSym::Error(..) => false,
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
    match module {
        ModSymbol::Ast(ast) => match (ast.file(db), ast.inline_unexpanded_items(db).is_some()) {
            (Some(f), _) => format!("\"{}\"", f.path(db)),
            (None, true) => format!("inline \"{}\"", ast.name(db).text(db)),
            (None, false) => format!("decl \"{}\"", ast.name(db).text(db)),
        },
        ModSymbol::Ext(ext) => format!("extern({}, {})", ext.crate_num.0, ext.def_index.0),
    }
}

/// Find a direct child definition by name.
pub fn definition<'db>(
    db: &'db dyn Db,
    module: ModSymbol<'db>,
    name: Name<'db>,
    source_root: SourceRoot,
) -> Option<Symbol<'db>> {
    db.log_query(format!(
        "definition({}, \"{}\")",
        module_label(db, module),
        name.text(db)
    ));
    match module {
        ModSymbol::Ast(ast) => {
            let scope = ScopeSymbol::Module(module, source_root);
            for item in ast.unexpanded_items(db) {
                if item_name(db, item) == Some(name) {
                    return Some(Symbol::local(item, scope));
                }
            }
            None
        }
        ModSymbol::Ext(ext) => {
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
    source_root: SourceRoot,
) -> Option<Symbol<'db>> {
    match module {
        ModSymbol::Ast(_) => definition(db, module, name, source_root),
        ModSymbol::Ext(ext) => {
            let raw = db.tcx().module_children(ext.crate_num, ext.def_index);
            let name_text = name.text(db);
            raw.into_iter()
                .find(|c| c.name == *name_text && c.namespace == ns)
                .map(|c| Symbol::external(c.crate_num, c.def_index))
        }
    }
}

/// Resolve a `mod foo` declaration LocalModSym to its resolved ModSymbol.
///
/// For inline modules (`mod foo { ... }`), mints a resolved LocalModSym
/// that wraps the declaration with parent context (and no `file`).
/// For file-based modules (`mod foo;`), looks up `foo.rs` or
/// `foo/mod.rs` in the source root and mints a LocalModSym with the
/// resolved file.
pub fn resolve_mod<'db>(
    db: &'db dyn Db,
    parent: ModSymbol<'db>,
    decl: LocalModSym<'db>,
    source_root: SourceRoot,
) -> Option<ModSymbol<'db>> {
    resolve_mod_tracked(db, parent, decl, source_root).map(ModSymbol::ast)
}

#[salsa::tracked]
fn resolve_mod_tracked<'db>(
    db: &'db dyn Db,
    parent: ModSymbol<'db>,
    decl: LocalModSym<'db>,
    source_root: SourceRoot,
) -> Option<LocalModSym<'db>> {
    if let Some(items) = decl.inline_unexpanded_items(db) {
        let inline = LocalModSym::new(
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
            let resolved = LocalModSym::new(
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
    let mut resolver = Resolver::new(db, ScopeSymbol::Module(root, source_root));
    resolver.resolve_module_path(root, segments)
}

/// Extract the name from an item, if it has one.
pub fn item_name<'db>(db: &'db dyn Db, item: LocalModItemSym<'db>) -> Option<Name<'db>> {
    match item {
        LocalModItemSym::Function(f) => Some(f.name(db)),
        LocalModItemSym::Struct(s) => Some(s.name(db)),
        LocalModItemSym::Enum(e) => Some(e.name(db)),
        LocalModItemSym::Trait(t) => Some(t.name(db)),
        LocalModItemSym::TypeAlias(t) => Some(t.name(db)),
        LocalModItemSym::Const(c) => Some(c.name(db)),
        LocalModItemSym::Static(s) => Some(s.name(db)),
        LocalModItemSym::Mod(m) => Some(m.name(db)),
        LocalModItemSym::Impl(_)
        | LocalModItemSym::Use(_)
        | LocalModItemSym::MacroDef(_)
        | LocalModItemSym::MacroInvocation(_)
        | LocalModItemSym::Error(..) => None,
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
pub fn module_items<'db>(db: &'db dyn Db, module: ModSymbol<'db>) -> Vec<LocalModItemSym<'db>> {
    db.log_query(format!("module_items({})", module_label(db, module)));
    match module {
        ModSymbol::Ast(ast) => ast.unexpanded_items(db),
        ModSymbol::Ext(_) => Vec::new(),
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
    let mut resolver = Resolver::new(db, ScopeSymbol::Module(module, source_root));
    resolver.resolve_name(name, ns)
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
    scope: ScopeSymbol<'db>,
    in_flight: Vec<InFlightQuery<'db>>,
    pub ribs: Ribs<'db>,
}

impl<'db> Resolver<'db> {
    pub fn new(db: &'db dyn Db, scope: ScopeSymbol<'db>) -> Self {
        let source_root = scope.source_root(db);
        let mut ribs = Ribs::new();
        ribs.push_scope();
        Self {
            db,
            source_root,
            scope,
            in_flight: Vec::new(),
            ribs,
        }
    }

    /// Resolve a single name using the resolver's scope as the starting module.
    pub fn resolve_name(
        &mut self,
        name: Name<'db>,
        ns: Namespace,
    ) -> Result<Symbol<'db>, ResolutionError> {
        let module = self.scope.module(self.db);
        resolve_plain_name(self, module, name, ns)
    }

    /// Resolve a slice of name segments using the resolver's scope as the
    /// starting module.
    pub fn resolve_segments(
        &mut self,
        segments: &[Name<'db>],
        final_ns: Namespace,
    ) -> Result<Symbol<'db>, ResolutionError> {
        let module = self.scope.module(self.db);
        self.resolve_segments_in(module, segments, final_ns)
    }

    /// Resolve a slice of name segments starting from an explicit module.
    ///
    /// Used by internal helpers that traverse into child modules during
    /// multi-segment path resolution.
    pub fn resolve_segments_in(
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
            return match first_module {
                ModSymbol::Ext(ext) => Ok(Symbol::ext(ext)),
                ModSymbol::Ast(_) => Err(ResolutionError::Unresolved),
            };
        }

        resolve_remainder(self, first_module, rest, final_ns)
    }

    /// Resolve a slice of name segments to a module, starting from the
    /// resolver's scope.
    pub fn resolve_segments_to_module(
        &mut self,
        segments: &[Name<'db>],
    ) -> Result<ModSymbol<'db>, ResolutionError> {
        let module = self.scope.module(self.db);
        self.resolve_segments_to_module_in(module, segments)
    }

    /// Resolve a slice of name segments to a module, starting from an
    /// explicit module.
    pub fn resolve_segments_to_module_in(
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
                .or_else(|| definition(self.db, current, name, self.source_root))?;
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

    let result = match module {
        ModSymbol::Ext(_) => {
            definition_in_ns(db, module, name, ns, source_root).ok_or(ResolutionError::Unresolved)
        }
        ModSymbol::Ast(ast) => {
            let memmap = expanded_module(db, ast, source_root);
            let stash = memmap.stash(db);
            let entries = memmap.entries(db);
            let mut named: Vec<Symbol<'db>> = Vec::new();
            let mut glob_matches: Vec<Symbol<'db>> = Vec::new();

            walk_entries(
                resolver,
                module,
                stash,
                entries,
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
    stash: &sage_stash::Stash,
    entries: sage_stash::Slice<crate::memmap::MemmapEntry<'db>>,
    name: Name<'db>,
    ns: Namespace,
    named: &mut Vec<Symbol<'db>>,
    glob_matches: &mut Vec<Symbol<'db>>,
) {
    use crate::memmap::MemmapEntry;

    let db = resolver.db;
    let scope = ScopeSymbol::Module(module, resolver.source_root);

    for entry in &stash[entries] {
        match entry {
            MemmapEntry::Item(item) => {
                if item_name(db, *item) == Some(name) && item_in_namespace(db, *item, ns) {
                    let sym = Symbol::local(*item, scope);
                    if !named.contains(&sym) {
                        named.push(sym);
                    }
                }
            }
            MemmapEntry::TupleStructCtor(s) => {
                if s.name(db) == name && matches!(ns, Namespace::Value) {
                    let sym = Symbol::tuple_struct_ctor_local(*s, scope);
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
                    let target_vec: Vec<_> = stash[*target].to_vec();
                    if let Ok(sym) = resolver.resolve_segments_in(module, &target_vec, ns) {
                        if !named.contains(&sym) {
                            named.push(sym);
                        }
                    }
                }
            }
            MemmapEntry::Glob { path } => {
                let path_vec: Vec<_> = stash[*path].to_vec();
                let Ok(target) = resolver.resolve_segments_to_module_in(module, &path_vec) else {
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
                for exp in &stash[mu.expansions] {
                    walk_entries(
                        resolver,
                        module,
                        stash,
                        exp.entries,
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
        return Ok(Symbol::external(crate_num, DefIndex(0)));
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
            let ext_mod = ModSymbol::external(cn, DefIndex(0));
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
                let ext_mod = ModSymbol::external(cn, DefIndex(0));
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
        return match start {
            ModSymbol::Ext(ext) => Ok(Symbol::ext(ext)),
            ModSymbol::Ast(_) => Err(ResolutionError::Unresolved),
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
    let std_root = ModSymbol::external(std_crate, DefIndex(0));

    let dummy_root = SourceRoot::new(db, Vec::new());

    let prelude_name = Name::new(db, "prelude".to_owned());
    let prelude_sym = definition(db, std_root, prelude_name, dummy_root)?;
    let prelude_mod = symbol_to_module(db, prelude_sym, dummy_root, std_root)?;

    let v1_name = Name::new(db, "v1".to_owned());
    let v1_sym = definition(db, prelude_mod, v1_name, dummy_root)?;
    let v1_mod = symbol_to_module(db, v1_sym, dummy_root, prelude_mod)?;

    let ext = match v1_mod.data() {
        ModSymbol::Ext(e) => e,
        ModSymbol::Ast(_) => return None,
    };
    let raw = db.tcx().module_children(ext.crate_num, ext.def_index);
    let name_text = name.text(db);
    raw.into_iter()
        .find(|c| c.name == *name_text && c.namespace == ns)
        .map(|c| Symbol::external(c.crate_num, c.def_index))
}

/// Try to convert a Symbol into a ModSymbol (for walking into child segments).
pub(crate) fn symbol_to_module<'db>(
    db: &'db dyn Db,
    sym: Symbol<'db>,
    source_root: SourceRoot,
    parent: ModSymbol<'db>,
) -> Option<ModSymbol<'db>> {
    match sym {
        SymbolData::Mod(m) => match m {
            ModSymbol::Ast(decl) => resolve_mod(db, parent, decl, source_root),
            ModSymbol::Ext(ext) => Some(ModSymbol::external(ext.crate_num, ext.def_index)),
        },
        SymbolData::Unknown(ext) => {
            if !db.tcx().is_module(ext.crate_num, ext.def_index) {
                return None;
            }
            Some(ModSymbol::external(ext.crate_num, ext.def_index))
        }
        _ => None,
    }
}
