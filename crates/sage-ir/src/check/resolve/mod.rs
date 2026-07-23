//! Name resolution.
//!
//! The [`Resolver`] struct is the public interface: create one per resolution
//! context (signature lowering, body checking, macro expansion) and call its
//! methods to resolve paths and names.
//!
//! Internally, the resolver tracks an `in_flight` stack of
//! (module, name, namespace) triples to detect cycles through `use` chains
//! (including globs).

mod ribs;

pub use ribs::{Resolution, Ribs};

use sage_stash::Stash;

use crate::Db;
use crate::cst::paths::{Path, PathAnchorKind, PathSegment};
use crate::cst::uses::UseKind;
use crate::local_syms::intrinsic_types::IntrinsicTypeSym;
use crate::name::Name;
use crate::scope::ScopeSymbol;
use crate::symbol::intrinsic::Intrinsic;
use crate::symbol::{
    DefIndex, ModSymbol, SymExt, SymExtKind, Symbol, SymbolData, TraitSymbol, UseSymbol,
};

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, salsa::Update)]
pub enum MacroKind {
    /// `foo!()`
    Bang,
    /// `#[foo]`
    Attr,
    /// `#[derive(Foo)]`
    Derive,
}

/// The namespace a name lives in. Each module maps names independently per namespace,
/// so the same identifier can resolve to different items in different namespaces.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, salsa::Update)]
pub enum Namespace {
    /// Types, traits, modules, type aliases, enum variants (as types).
    Type,
    /// Functions, constants, statics, enum variant constructors, local bindings.
    Value,
    /// Macros, subdivided by kind. Rustc uses a single `MacroNS` and applies a
    /// sub-namespace filter at lookup time (see `sub_namespace_match` in
    /// `rustc_resolve`). We instead model these as distinct namespace variants:
    /// the observable behavior is equivalent — a bang macro and a derive of the
    /// same name coexist without ambiguity — but separate variants let each name
    /// occupy exactly one slot per namespace with no post-hoc filtering at lookup.
    Macro(MacroKind),
}

#[derive(Copy, Clone)]
pub(crate) enum ResolvePhase {
    MacroExpansion,
    Normal,
}

// ---------------------------------------------------------------------------
// Resolver
// ---------------------------------------------------------------------------

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
struct InFlightQuery<'db> {
    module: ModSymbol<'db>,
    name: Name<'db>,
    namespace: Namespace,
}

/// Stateful name-resolution context.
///
/// Create one per top-level resolution request (e.g., per signature lowered
/// or per body resolved). All resolution calls within that scope share the
/// same cycle-detection context.
#[derive(Clone)]
pub(crate) struct Resolver<'db> {
    db: &'db dyn Db,
    phase: ResolvePhase,
    scope: ScopeSymbol<'db>,
    in_flight: Vec<InFlightQuery<'db>>,
    pub ribs: Ribs<'db>,
}

impl<'db> Resolver<'db> {
    pub fn new(db: &'db dyn Db, scope: ScopeSymbol<'db>) -> Self {
        let mut ribs = Ribs::new();
        ribs.push_scope();
        Self {
            db,
            phase: ResolvePhase::Normal,
            scope,
            in_flight: Vec::new(),
            ribs,
        }
    }

    pub fn new_for_macro_expansion(
        db: &'db dyn Db,
        module: crate::local_syms::mods::LocalModSym<'db>,
    ) -> Self {
        Self {
            db,
            phase: ResolvePhase::MacroExpansion,
            scope: ScopeSymbol::Module(module),
            in_flight: Vec::new(),
            ribs: Ribs::new(),
        }
    }

    fn module(&self) -> ModSymbol<'db> {
        self.scope.module(self.db).into()
    }

    fn edition(&self) -> crate::scope::Edition {
        self.scope.module(self.db).edition(self.db)
    }

    pub(crate) fn local_crate(&self) -> crate::scope::LocalCrateSymbol<'db> {
        self.scope.local_crate(self.db)
    }

    pub fn resolve_path(
        &mut self,
        stash: &Stash,
        path: Path<'db>,
        namespace: Namespace,
    ) -> Vec<Resolution<'db>> {
        // FIXME: This isn't quite right. We need to be able to resolve
        // other paths from the ribs, e.g., `T::Item`. But to do that
        // we will need to integrate with the type checker.
        match path {
            Path::Anchored(anchor, members)
                if anchor.kind == crate::cst::paths::PathAnchorKind::Self_
                    && stash[members].is_empty()
                    && namespace == Namespace::Value =>
            {
                self.ribs
                    .lookup(Name::new(self.db, "self".to_owned()), namespace)
                    .into_iter()
                    .collect()
            }
            Path::Relative(first, rest) if stash[rest].is_empty() => {
                // Single-segment unqualified path: check ribs first.
                if let Some(entry) = self.ribs.lookup(first.name, namespace) {
                    return vec![entry];
                }
                let module = self.module();
                self.flexibly_resolve_name_from_module(module, first.name, namespace)
                    .into_iter()
                    .map(Resolution::Sym)
                    .collect()
            }
            _ => {
                let module = self.module();
                self.resolve_path_in_module(module, stash, path, namespace)
                    .into_iter()
                    .map(Resolution::Sym)
                    .collect()
            }
        }
    }

    pub fn resolve_name_from_scope(
        &mut self,
        name: Name<'db>,
        namespace: Namespace,
    ) -> Vec<Resolution<'db>> {
        if let Some(entry) = self.ribs.lookup(name, namespace) {
            return vec![entry];
        }
        let module = self.module();
        self.flexibly_resolve_name_from_module(module, name, namespace)
            .into_iter()
            .map(Resolution::Sym)
            .collect()
    }

    // ANCHOR: example_traits_in_method_scope
    /// Enumerate the trait definitions directly available for method lookup.
    ///
    /// This covers traits defined in the current module, explicitly imported
    /// or glob-imported traits, and traits re-exported by the edition's
    /// standard prelude. The boolean is false when an unresolved import or
    /// macro could contribute another trait; callers must not interpret that
    /// subset as an exhaustive negative.
    pub(crate) fn traits_in_method_scope(&mut self) -> (Vec<(TraitSymbol<'db>, bool)>, bool) {
        let mut traits = Vec::new();
        let mut complete = match self.module() {
            ModSymbol::Local(module) => {
                crate::local_syms::mods::module_expansion_complete_for_method_providers(
                    self.db, module,
                )
            }
            ModSymbol::Ext(_) => false,
        };
        let mut push_module_traits = |module: ModSymbol<'db>, definitely_in_scope: bool| {
            for trait_sym in trait_symbols_in_module(self.db, module) {
                if let Some((_, definite)) = traits
                    .iter_mut()
                    .find(|(candidate, _)| *candidate == trait_sym)
                {
                    *definite |= definitely_in_scope;
                } else {
                    traits.push((trait_sym, definitely_in_scope));
                }
            }
        };

        push_module_traits(self.module(), true);
        drop(push_module_traits);

        // A `use` can place a trait in method scope even when the trait name is
        // never mentioned by the call. Resolve each import as an import edge;
        // successful non-trait imports are complete and add no provider.
        for symbol in self.module().expanded_module_items(self.db) {
            let imports = match symbol.data(self.db) {
                SymbolData::UseSymbol(UseSymbol::Local(imports)) => imports,
                SymbolData::UseSymbol(UseSymbol::Ext(_)) => {
                    complete = false;
                    continue;
                }
                SymbolData::FnSymbol(_)
                | SymbolData::StructSymbol(_)
                | SymbolData::EnumSymbol(_)
                | SymbolData::VariantSymbol(_)
                | SymbolData::VariantCtorSymbol(_)
                | SymbolData::TraitSymbol(_)
                | SymbolData::TypeAliasSymbol(_)
                | SymbolData::ConstSymbol(_)
                | SymbolData::StaticSymbol(_)
                | SymbolData::ImplSymbol(_)
                | SymbolData::ModSymbol(_)
                | SymbolData::MacroDefSymbol(_)
                | SymbolData::IntrinsicTypeSymbol(_)
                | SymbolData::MacroInvocationSymbol(_) => continue,
            };
            let (stash, imports) = imports.imports(self.db).open();
            for import in &stash[imports] {
                let resolved = self.resolve_use(self.module(), stash, import.path, Namespace::Type);
                match import.kind {
                    UseKind::Named(_) | UseKind::Unnamed => {
                        let value_resolved = if resolved.is_empty() {
                            self.resolve_use(self.module(), stash, import.path, Namespace::Value)
                        } else {
                            Vec::new()
                        };
                        if resolved.is_empty() && value_resolved.is_empty() {
                            complete = false;
                        }
                        for symbol in resolved {
                            if let Some(trait_sym) = symbol.trait_symbol(self.db)
                                && !traits.iter().any(|(candidate, _)| *candidate == trait_sym)
                            {
                                traits.push((trait_sym, true));
                            }
                        }
                    }
                    UseKind::Glob => {
                        let modules: Vec<_> = resolved
                            .into_iter()
                            .filter_map(|symbol| symbol.module(self.db))
                            .collect();
                        if modules.is_empty() {
                            complete = false;
                        }
                        for module in modules {
                            match module {
                                ModSymbol::Local(_) => {
                                    // A local glob can expose reexports and
                                    // macro-produced providers. Until those
                                    // export edges are enumerated recursively,
                                    // it is not an exhaustive provider source.
                                    complete = false;
                                }
                                ModSymbol::Ext(_) => {
                                    for trait_sym in trait_symbols_in_module(self.db, module) {
                                        if !traits
                                            .iter()
                                            .any(|(candidate, _)| *candidate == trait_sym)
                                        {
                                            traits.push((trait_sym, true));
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }

        let std_name = Name::new(self.db, "std".to_owned());
        if let Some(std_module) = lookup_extern_prelude(self.db, std_name, Namespace::Type)
            .and_then(|symbol| symbol.module(self.db))
        {
            let prelude_name = Name::new(self.db, "prelude".to_owned());
            let prelude_modules: Vec<_> = self
                .resolve_name_from_module(std_module, prelude_name, Namespace::Type)
                .into_iter()
                .filter_map(|symbol| symbol.module(self.db))
                .collect();
            if prelude_modules.len() != 1 {
                complete = false;
            }
            let edition = self.edition().prelude_module();
            let mut edition_modules = Vec::new();
            for module in &prelude_modules {
                edition_modules.extend(
                    self.resolve_name_from_module(
                        *module,
                        Name::new(self.db, edition.to_owned()),
                        Namespace::Type,
                    )
                    .into_iter()
                    .filter_map(|edition_module| edition_module.module(self.db)),
                );
            }
            if edition_modules.len() != 1 {
                complete = false;
            }
            for edition_module in edition_modules {
                for trait_sym in trait_symbols_in_module(self.db, edition_module) {
                    if !traits.iter().any(|(candidate, _)| *candidate == trait_sym) {
                        traits.push((trait_sym, true));
                    }
                }
            }
        } else {
            complete = false;
        }

        (traits, complete)
    }
    // ANCHOR_END: example_traits_in_method_scope

    // -----------------------------------------------------------------------
    // Internal
    // -----------------------------------------------------------------------

    fn resolve_path_in_module(
        &mut self,
        module: ModSymbol<'db>,
        stash: &Stash,
        path: Path<'db>,
        namespace: Namespace,
    ) -> Vec<Symbol<'db>> {
        match path {
            Path::Relative(first, rest) => {
                if stash[rest].is_empty() {
                    return self.flexibly_resolve_name_from_module(module, first.name, namespace);
                }

                let symbols =
                    self.flexibly_resolve_name_from_module(module, first.name, Namespace::Type);

                self.resolve_remaining_segments(stash, symbols, &stash[rest], namespace)
            }
            Path::Anchored(anchor, members) => {
                let anchor_modules = resolve_anchor(self.db, module, stash, anchor.kind);
                let rest = &stash[members];

                if rest.is_empty() {
                    if namespace != Namespace::Type {
                        return vec![];
                    }
                    return anchor_modules
                        .into_iter()
                        .map(|m| mod_to_symbol(m))
                        .collect();
                }

                let symbols: Vec<Symbol<'db>> =
                    anchor_modules.into_iter().map(mod_to_symbol).collect();
                self.resolve_remaining_segments(stash, symbols, rest, namespace)
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
    fn flexibly_resolve_name_from_module(
        &mut self,
        module: ModSymbol<'db>,
        name: Name<'db>,
        namespace: Namespace,
    ) -> Vec<Symbol<'db>> {
        let results = self.resolve_name_from_module(module, name, namespace);
        if !results.is_empty() {
            return results;
        }

        if let Some(sym) = lookup_extern_prelude(self.db, name, namespace) {
            return vec![sym];
        }

        let results = self.lookup_std_prelude(name, namespace);
        if !results.is_empty() {
            return results;
        }

        if let Some(sym) = lookup_lang_prelude(self.db, name, namespace) {
            return vec![sym];
        }

        vec![]
    }

    fn resolve_remaining_segments(
        &mut self,
        stash: &Stash,
        symbols: Vec<Symbol<'db>>,
        rest: &[PathSegment<'db>],
        namespace: Namespace,
    ) -> Vec<Symbol<'db>> {
        assert!(!rest.is_empty(), "`rest` must be non-empty");

        symbols
            .into_iter()
            .flat_map(|s| match rest {
                [final_segment] => self.resolve_name_in(s, final_segment.name, namespace),

                [next_segment, rest @ ..] => {
                    let next_symbols = self.resolve_name_in(s, next_segment.name, Namespace::Type);
                    self.resolve_remaining_segments(stash, next_symbols, rest, namespace)
                }

                [] => unreachable!(),
            })
            .collect()
    }

    /// Look up a name inside a "container" symbol. For modules this goes
    /// through use-import resolution; for enums it's a flat child lookup.
    fn resolve_name_in(
        &mut self,
        sym: Symbol<'db>,
        name: Name<'db>,
        namespace: Namespace,
    ) -> Vec<Symbol<'db>> {
        if let Some(m) = sym.module(self.db) {
            return self.resolve_name_from_module(m, name, namespace);
        }
        let children = sym.children(self.db).unwrap_or_default();
        children
            .iter()
            .filter(|item| {
                item.name(self.db)
                    .is_some_and(|(n, ns)| n == name && ns == namespace)
            })
            .copied()
            .collect()
    }

    fn resolve_name_from_module(
        &mut self,
        module: ModSymbol<'db>,
        name: Name<'db>,
        namespace: Namespace,
    ) -> Vec<Symbol<'db>> {
        let query = InFlightQuery {
            module,
            name,
            namespace,
        };
        if self.in_flight.contains(&query) {
            return Vec::new();
        }
        self.in_flight.push(query);

        let results = match self.phase {
            ResolvePhase::MacroExpansion => self.lookup_in_module(
                LookupFilter {
                    named: true,
                    globs: true,
                },
                module,
                name,
                namespace,
            ),
            ResolvePhase::Normal => {
                let results = self.lookup_in_module(
                    LookupFilter {
                        named: true,
                        globs: false,
                    },
                    module,
                    name,
                    namespace,
                );
                if !results.is_empty() {
                    results
                } else {
                    self.lookup_in_module(
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
        };

        self.in_flight.pop();
        results
    }

    fn lookup_in_module(
        &mut self,
        filter: LookupFilter,
        module: ModSymbol<'db>,
        name: Name<'db>,
        namespace: Namespace,
    ) -> Vec<Symbol<'db>> {
        if let (ModSymbol::Ext(external), Namespace::Macro(_)) = (module, namespace) {
            // Macro namespace is retained on the exported-child edge; a bare
            // external macro definition does not encode its macro flavor.
            return if filter.named {
                external.named_children(self.db, name, namespace).to_vec()
            } else {
                Vec::new()
            };
        }
        let module_expanded_items = module.expanded_module_items(self.db);

        let mut results = vec![];
        for &item in module_expanded_items {
            match item.data(self.db) {
                SymbolData::FnSymbol(..)
                | SymbolData::StructSymbol(..)
                | SymbolData::EnumSymbol(..)
                | SymbolData::VariantSymbol(..)
                | SymbolData::VariantCtorSymbol(..)
                | SymbolData::TraitSymbol(..)
                | SymbolData::TypeAliasSymbol(..)
                | SymbolData::ConstSymbol(..)
                | SymbolData::StaticSymbol(..)
                | SymbolData::ImplSymbol(..)
                | SymbolData::ModSymbol(..)
                | SymbolData::MacroDefSymbol(..)
                | SymbolData::IntrinsicTypeSymbol(..)
                | SymbolData::MacroInvocationSymbol(..) => {
                    if filter.named
                        && let Some((n, nspace)) = item.name(self.db)
                        && n == name
                        && nspace == namespace
                    {
                        results.push(item);
                    }
                }

                SymbolData::UseSymbol(sym) => match sym {
                    UseSymbol::Local(sym) => {
                        if crate::local_syms::mods::item_has_unexpanded_active_attribute(
                            self.db,
                            crate::local_syms::LocalModItemSym::Use(sym),
                        ) {
                            continue;
                        }
                        let (stash, imports) = sym.imports(self.db).open();
                        for import in &stash[imports] {
                            match import.kind {
                                UseKind::Named(n) => {
                                    if filter.named && n == name {
                                        results.extend(self.resolve_use(
                                            module,
                                            stash,
                                            import.path,
                                            namespace,
                                        ));
                                    }
                                }
                                UseKind::Glob => {
                                    if filter.globs {
                                        let glob_from = self.resolve_use(
                                            module,
                                            stash,
                                            import.path,
                                            Namespace::Type,
                                        );
                                        results
                                            .extend(self.resolve_glob(&glob_from, name, namespace));
                                    }
                                }
                                UseKind::Unnamed => {}
                            }
                        }
                    }

                    UseSymbol::Ext(_) => {
                        panic!("Not yet implemented: external use items");
                    }
                },
            }
        }

        results
    }

    /// Resolve the target of a `use` import path, with cycle detection.
    fn resolve_use(
        &mut self,
        origin_module: ModSymbol<'db>,
        stash: &Stash,
        path_ptr: sage_stash::Ptr<Path<'db>>,
        namespace: Namespace,
    ) -> Vec<Symbol<'db>> {
        let path = stash[path_ptr];
        match path {
            Path::Relative(first, rest) => {
                if stash[rest].is_empty() {
                    return self.flexibly_resolve_name_from_module(
                        origin_module,
                        first.name,
                        namespace,
                    );
                }
                let symbols = self.flexibly_resolve_name_from_module(
                    origin_module,
                    first.name,
                    Namespace::Type,
                );
                self.resolve_use_remaining_segments(stash, symbols, &stash[rest], namespace)
            }
            Path::Anchored(anchor, members) => {
                let anchor_modules = resolve_anchor(self.db, origin_module, stash, anchor.kind);
                let rest = &stash[members];
                if rest.is_empty() {
                    return if namespace == Namespace::Type {
                        anchor_modules.into_iter().map(mod_to_symbol).collect()
                    } else {
                        Vec::new()
                    };
                }
                let symbols = anchor_modules.into_iter().map(mod_to_symbol).collect();
                self.resolve_use_remaining_segments(stash, symbols, rest, namespace)
            }
        }
    }

    fn resolve_use_remaining_segments(
        &mut self,
        stash: &Stash,
        symbols: Vec<Symbol<'db>>,
        rest: &[PathSegment<'db>],
        namespace: Namespace,
    ) -> Vec<Symbol<'db>> {
        symbols
            .into_iter()
            .flat_map(|symbol| match rest {
                [final_segment] => self.resolve_name_in_use(symbol, final_segment.name, namespace),
                [next_segment, rest @ ..] => {
                    let next = self.resolve_name_in_use(symbol, next_segment.name, Namespace::Type);
                    self.resolve_use_remaining_segments(stash, next, rest, namespace)
                }
                [] => Vec::new(),
            })
            .collect()
    }

    fn resolve_name_in_use(
        &mut self,
        symbol: Symbol<'db>,
        name: Name<'db>,
        namespace: Namespace,
    ) -> Vec<Symbol<'db>> {
        match symbol.module(self.db) {
            Some(ModSymbol::Ext(external)) => {
                external.named_children(self.db, name, namespace).to_vec()
            }
            Some(ModSymbol::Local(module)) => {
                self.resolve_name_from_module(ModSymbol::Local(module), name, namespace)
            }
            None => self.resolve_name_in(symbol, name, namespace),
        }
    }

    fn resolve_glob(
        &mut self,
        glob_from: &[Symbol<'db>],
        name: Name<'db>,
        namespace: Namespace,
    ) -> Vec<Symbol<'db>> {
        let modules: Vec<ModSymbol<'db>> =
            glob_from.iter().filter_map(|s| s.module(self.db)).collect();

        modules
            .into_iter()
            .flat_map(|module| self.resolve_name_from_module(module, name, namespace))
            .collect()
    }

    fn lookup_std_prelude(&mut self, name: Name<'db>, namespace: Namespace) -> Vec<Symbol<'db>> {
        let std_name = Name::new(self.db, "std".to_owned());
        let Some(std_sym) = lookup_extern_prelude(self.db, std_name, Namespace::Type) else {
            return vec![];
        };
        let Some(std_mod) = std_sym.module(self.db) else {
            return vec![];
        };

        let prelude_name = Name::new(self.db, "prelude".to_owned());
        let prelude_syms = self.resolve_name_from_module(std_mod, prelude_name, Namespace::Type);

        let edition_name = Name::new(self.db, self.edition().prelude_module().to_owned());
        let edition_mods: Vec<Symbol<'db>> = prelude_syms
            .iter()
            .filter_map(|s| s.module(self.db))
            .flat_map(|m| self.resolve_name_from_module(m, edition_name, Namespace::Type))
            .collect();

        self.resolve_glob(&edition_mods, name, namespace)
    }
}

fn trait_symbols_in_module<'db>(
    db: &'db dyn crate::Db,
    module: ModSymbol<'db>,
) -> Vec<TraitSymbol<'db>> {
    match module {
        ModSymbol::Local(_) => module
            .expanded_module_items(db)
            .iter()
            .filter_map(|symbol| symbol.trait_symbol(db))
            .collect(),
        ModSymbol::Ext(external) => external.trait_children(db).to_vec(),
    }
}

// ---------------------------------------------------------------------------
// Free helpers (no cycle tracking needed)
// ---------------------------------------------------------------------------

struct LookupFilter {
    named: bool,
    globs: bool,
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
            SymExt::new(db, ext.crate_num(db), DefIndex(0), SymExtKind::Mod).into()
        }
    }
}
