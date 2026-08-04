use sage_stash::{Slice, StashDirect, Stashed};

use crate::Db;
use crate::cst::attrs::{AttrCst, AttrCstKind, is_known_inert_item_attribute};
use crate::local_syms::LocalModItemSym;
use crate::local_syms::macro_invocations::LocalMacroInvocationSym;
use crate::name::Name;
use crate::resolve::{MacroKind, Namespace, Resolver};
use crate::scope::ScopeSymbol;
use crate::source::SourceFile;
use crate::span::{AbsoluteSpan, ParseSource};
use crate::symbol::{MacroDefSymbol, Symbol, SymbolData};

/// A module written in (or synthesized for) the local workspace.
#[salsa::tracked(debug)]
pub struct LocalModSym<'db> {
    pub name: Name<'db>,

    /// The crate edition is copied onto modules so resolution performed while
    /// expanding the root module does not require a cyclic root-to-crate edge.
    pub edition: crate::scope::Edition,

    /// The enclosing module, if any. `None` only for the crate root.
    pub parent: Option<ScopeSymbol<'db>>,

    #[returns(ref)]
    pub body_source: ModBodySource,

    #[returns(ref)]
    pub attrs: Stashed<Slice<AttrCst<'db>>>,

    #[tracked]
    pub span: AbsoluteSpan<'db>,
}

/// Where a module's body (its items) comes from.
#[derive(Clone, Debug, Hash, salsa::Update)]
pub enum ModBodySource {
    /// File-backed: `mod foo;` — items are parsed from the file.
    File(SourceFile),
    /// Inline: `mod foo { ... }` — items are `specify`'d at parse time.
    Inline,
}

impl StashDirect for LocalModSym<'_> {}

impl<'db> LocalModSym<'db> {
    pub fn file(self, db: &'db dyn Db) -> Option<SourceFile> {
        match self.body_source(db) {
            ModBodySource::File(f) => Some(*f),
            ModBodySource::Inline => None,
        }
    }

    pub fn get_attrs(self, db: &'db dyn Db) -> (&'db sage_stash::Stash, &'db [AttrCst<'db>]) {
        let (stash, slice) = self.attrs(db).open();
        (stash, &stash[slice])
    }

    pub fn unexpanded_items(self, db: &'db dyn Db) -> &'db [LocalModItemSym<'db>] {
        unexpanded_items(db, self)
    }
}

#[salsa::tracked(specify, returns(ref))]
pub fn unexpanded_items<'db>(
    db: &'db dyn Db,
    module: LocalModSym<'db>,
) -> Vec<LocalModItemSym<'db>> {
    match module.body_source(db) {
        ModBodySource::File(f) => {
            let source = ParseSource::SourceFile(*f);
            let scope = ScopeSymbol::Module(module);
            crate::parse::parse_str_to_cst(db, source, f.text(db), scope)
                .into_iter()
                .collect()
        }
        ModBodySource::Inline => {
            panic!("unexpanded_items should be specify'd for inline modules")
        }
    }
}

/// Compute the macro-expanded items for a local module.
///
/// Note that this may recursively access the macro-expanded items for `module`,
/// in which case it relies on salsa's fixed point iteration.
// ANCHOR: example_expanded_module_query
#[salsa::tracked(returns(ref), cycle_initial = expanded_module_initial)]
pub fn local_expanded_module_items<'db>(
    db: &'db dyn Db,
    module: LocalModSym<'db>,
) -> Vec<Symbol<'db>> {
    let mut entries: Vec<Symbol<'db>> = Vec::new();

    let items = module.unexpanded_items(db);
    expand_unexpanded_items(db, module, items, &mut entries);

    entries
}
// ANCHOR_END: example_expanded_module_query

/// Cycle recovery initial value.
fn expanded_module_initial<'db>(
    _db: &'db dyn Db,
    _id: salsa::Id,
    _module: LocalModSym<'db>,
) -> Vec<Symbol<'db>> {
    vec![]
}

// ANCHOR: example_expand_items
fn expand_unexpanded_items<'db>(
    db: &'db dyn Db,
    module: LocalModSym<'db>,
    unexpanded_items: &[LocalModItemSym<'db>],
    entries: &mut Vec<Symbol<'db>>,
) {
    expand_unexpanded_items_at_depth(db, module, unexpanded_items, entries, 0);
}

fn expand_unexpanded_items_at_depth<'db>(
    db: &'db dyn Db,
    module: LocalModSym<'db>,
    unexpanded_items: &[LocalModItemSym<'db>],
    entries: &mut Vec<Symbol<'db>>,
    depth: usize,
) {
    for &item in unexpanded_items {
        match item {
            LocalModItemSym::MacroInvocation(sym) => {
                if depth < MAX_EXPANSION_DEPTH && !item_has_unexpanded_active_attribute(db, item) {
                    expand_macro(db, module, sym, entries, depth);
                }
            }

            // Expansion failures are omitted from the symbol list. Candidate
            // completeness observes the same failure and prevents logical No.
            LocalModItemSym::Error(..) => {}

            LocalModItemSym::Struct(..)
            | LocalModItemSym::Enum(..)
            | LocalModItemSym::Function(..)
            | LocalModItemSym::Trait(..)
            | LocalModItemSym::Impl(..)
            | LocalModItemSym::TypeAlias(..)
            | LocalModItemSym::Const(..)
            | LocalModItemSym::Static(..)
            | LocalModItemSym::Mod(..)
            | LocalModItemSym::Use(..)
            | LocalModItemSym::MacroDef(..) => {
                expand_attribute_macros_and_derives(db, module, item, entries);
            }
        }
    }
}
// ANCHOR_END: example_expand_items

/// Maximum nesting depth for macro expansion (same as rustc's default).
const MAX_EXPANSION_DEPTH: usize = 128;

// ANCHOR: example_expand_macro
fn expand_macro<'db>(
    db: &'db dyn Db,
    module: LocalModSym<'db>,
    macro_invocation_sym: LocalMacroInvocationSym<'db>,
    entries: &mut Vec<Symbol<'db>>,
    depth: usize,
) {
    let Some(macro_def_symbol) = resolve_bang_macro(db, module, macro_invocation_sym) else {
        return;
    };
    if let MacroDefSymbol::Local(local) = macro_def_symbol
        && item_has_unexpanded_active_attribute(db, LocalModItemSym::MacroDef(local))
    {
        return;
    }

    let unexpanded_items = macro_invocation_sym.parse_output(db, macro_def_symbol);
    expand_unexpanded_items_at_depth(db, module, &unexpanded_items, entries, depth + 1);
}
// ANCHOR_END: example_expand_macro

fn resolve_bang_macro<'db>(
    db: &'db dyn Db,
    module: LocalModSym<'db>,
    macro_invocation_sym: LocalMacroInvocationSym<'db>,
) -> Option<MacroDefSymbol<'db>> {
    let (macro_stash, macro_cst) = macro_invocation_sym.cst(db).open_deref();
    let macro_path = macro_stash[macro_cst.path];

    let mut resolver = Resolver::new_for_macro_expansion(db, module);
    let macros: Vec<_> = resolver
        .resolve_path(macro_stash, macro_path, Namespace::Macro(MacroKind::Bang))
        .into_iter()
        .filter_map(|resolution| resolution.sym())
        .filter_map(|symbol| symbol_macro_definition(db, symbol))
        .collect();
    let [macro_def] = macros.as_slice() else {
        return None;
    };
    Some(*macro_def)
}

fn expand_attribute_macros_and_derives<'db>(
    db: &'db dyn Db,
    module: LocalModSym<'db>,
    item: LocalModItemSym<'db>,
    entries: &mut Vec<Symbol<'db>>,
) {
    // Until an active attribute is expanded, neither its input item nor any
    // derives attached to that input are definite program items.
    if item_has_unexpanded_active_attribute(db, item) {
        return;
    }

    let Some((attrs_stash, attrs)) = item.attrs(db) else {
        entries.push(item.into());
        return;
    };

    if attrs.is_empty() {
        entries.push(item.into());
        return;
    }

    let mut derives = Vec::new();
    for (attribute_index, attr) in attrs.iter().enumerate() {
        let path = attrs_stash[attr.path];

        // Look for built-in attribute names (single-segment relative paths)
        if let crate::cst::paths::Path::Relative(first, rest) = path {
            if attrs_stash[rest].is_empty() {
                let text: &str = first.name.text(db);

                if is_known_inert_item_attribute(text) {
                    continue;
                }

                if text == "derive" {
                    let args = &attrs_stash[attr.args];
                    derives.extend(parse_derive_names(args).into_iter().enumerate().map(
                        |(derive_index, name)| (attribute_index as u32, derive_index as u32, name),
                    ));
                }
            }
        }

        // Unknown and qualified paths were rejected by the active-attribute
        // guard above. Only inert attributes and derives reach this point.
    }

    // Derives append sibling impls; they do not replace the source item.
    entries.push(item.into());
    for (attribute_index, derive_index, derive_name) in derives {
        expand_derive(
            db,
            module,
            attribute_index,
            derive_index,
            derive_name,
            item,
            entries,
        );
    }
}

fn parse_derive_names(args: &[u8]) -> Vec<&str> {
    let Ok(args) = std::str::from_utf8(args) else {
        return Vec::new();
    };
    let args = args
        .strip_prefix('(')
        .and_then(|args| args.strip_suffix(')'))
        .unwrap_or(args);
    args.split(',')
        .map(str::trim)
        // Keep qualified or otherwise unsupported paths. Expansion may not
        // handle them yet, but completeness must observe rather than erase
        // them so fixed-trait search remains conservative.
        .filter(|name| !name.is_empty())
        .collect()
}

/// Whether an item has an active attribute whose transformation Sage has not
/// represented.
///
/// Such an item cannot safely participate in impl search: the attribute may
/// delete or replace it. The same attribute also makes candidate discovery
/// incomplete because it may emit other impls.
pub(crate) fn item_has_unexpanded_active_attribute<'db>(
    db: &'db dyn Db,
    item: LocalModItemSym<'db>,
) -> bool {
    let Some((stash, attrs)) = item.attrs(db) else {
        return false;
    };

    attrs.iter().any(|attr| {
        if attr.kind == AttrCstKind::DocComment {
            return false;
        }

        let crate::cst::paths::Path::Relative(first, rest) = stash[attr.path] else {
            return true;
        };
        if !stash[rest].is_empty() {
            return true;
        }

        let name = first.name.text(db);
        if name == "derive" {
            return !matches!(item, LocalModItemSym::Struct(_) | LocalModItemSym::Enum(_));
        }
        !is_known_inert_item_attribute(&name)
    })
}

fn symbol_local_item<'db>(db: &'db dyn Db, symbol: Symbol<'db>) -> Option<LocalModItemSym<'db>> {
    match symbol.data(db) {
        SymbolData::FnSymbol(crate::symbol::FnSymbol::Local(item)) => {
            Some(LocalModItemSym::Function(item))
        }
        SymbolData::StructSymbol(crate::symbol::StructSymbol::Local(item)) => {
            Some(LocalModItemSym::Struct(item))
        }
        SymbolData::EnumSymbol(crate::symbol::EnumSymbol::Local(item)) => {
            Some(LocalModItemSym::Enum(item))
        }
        SymbolData::TraitSymbol(crate::symbol::TraitSymbol::Local(item)) => {
            Some(LocalModItemSym::Trait(item))
        }
        SymbolData::ImplSymbol(crate::symbol::ImplSymbol::Local(item)) => {
            Some(LocalModItemSym::Impl(item))
        }
        SymbolData::TypeAliasSymbol(crate::symbol::TypeAliasSymbol::Local(item)) => {
            Some(LocalModItemSym::TypeAlias(item))
        }
        SymbolData::ConstSymbol(crate::symbol::ConstSymbol::Local(item)) => {
            Some(LocalModItemSym::Const(item))
        }
        SymbolData::StaticSymbol(crate::symbol::StaticSymbol::Local(item)) => {
            Some(LocalModItemSym::Static(item))
        }
        SymbolData::ModSymbol(crate::symbol::ModSymbol::Local(item)) => {
            Some(LocalModItemSym::Mod(item))
        }
        SymbolData::MacroDefSymbol(crate::symbol::MacroDefSymbol::Local(item)) => {
            Some(LocalModItemSym::MacroDef(item))
        }
        SymbolData::UseSymbol(crate::symbol::UseSymbol::Local(item)) => {
            Some(LocalModItemSym::Use(item))
        }
        SymbolData::MacroInvocationSymbol(item) => Some(LocalModItemSym::MacroInvocation(item)),
        SymbolData::FnSymbol(crate::symbol::FnSymbol::Ext(_))
        | SymbolData::StructSymbol(crate::symbol::StructSymbol::Ext(_))
        | SymbolData::EnumSymbol(crate::symbol::EnumSymbol::Ext(_))
        | SymbolData::TraitSymbol(crate::symbol::TraitSymbol::Ext(_))
        | SymbolData::ImplSymbol(crate::symbol::ImplSymbol::Ext(_))
        | SymbolData::TypeAliasSymbol(crate::symbol::TypeAliasSymbol::Ext(_))
        | SymbolData::ConstSymbol(crate::symbol::ConstSymbol::Ext(_))
        | SymbolData::StaticSymbol(crate::symbol::StaticSymbol::Ext(_))
        | SymbolData::ModSymbol(crate::symbol::ModSymbol::Ext(_))
        | SymbolData::MacroDefSymbol(crate::symbol::MacroDefSymbol::Ext(_))
        | SymbolData::UseSymbol(crate::symbol::UseSymbol::Ext(_))
        | SymbolData::VariantSymbol(_)
        | SymbolData::VariantCtorSymbol(_)
        | SymbolData::IntrinsicTypeSymbol(_) => None,
    }
}

fn symbol_macro_definition<'db>(
    db: &'db dyn Db,
    symbol: Symbol<'db>,
) -> Option<MacroDefSymbol<'db>> {
    match symbol.data(db) {
        SymbolData::MacroDefSymbol(macro_def) => Some(macro_def),
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
        | SymbolData::UseSymbol(_)
        | SymbolData::IntrinsicTypeSymbol(_)
        | SymbolData::MacroInvocationSymbol(_) => None,
    }
}

fn bang_macro_expansion_complete<'db>(
    db: &'db dyn Db,
    module: LocalModSym<'db>,
    invocation: LocalMacroInvocationSym<'db>,
    depth: usize,
) -> bool {
    if depth >= MAX_EXPANSION_DEPTH
        || item_has_unexpanded_active_attribute(db, LocalModItemSym::MacroInvocation(invocation))
    {
        return false;
    }
    let Some(macro_def) = resolve_bang_macro(db, module, invocation) else {
        return false;
    };
    if let MacroDefSymbol::Local(local) = macro_def
        && item_has_unexpanded_active_attribute(db, LocalModItemSym::MacroDef(local))
    {
        return false;
    }

    invocation.parse_output(db, macro_def).iter().all(|item| {
        if matches!(item, LocalModItemSym::Error(_))
            || item_has_unexpanded_active_attribute(db, *item)
        {
            return false;
        }
        match item {
            LocalModItemSym::MacroInvocation(nested) => {
                bang_macro_expansion_complete(db, module, *nested, depth + 1)
            }
            LocalModItemSym::Error(_) => false,
            LocalModItemSym::Function(_)
            | LocalModItemSym::Struct(_)
            | LocalModItemSym::Enum(_)
            | LocalModItemSym::Trait(_)
            | LocalModItemSym::Impl(_)
            | LocalModItemSym::TypeAlias(_)
            | LocalModItemSym::Const(_)
            | LocalModItemSym::Static(_)
            | LocalModItemSym::Mod(_)
            | LocalModItemSym::Use(_)
            | LocalModItemSym::MacroDef(_) => true,
        }
    })
}

/// Whether the module's represented expansion is known not to hide an impl
/// of `target_trait`.
///
/// A represented expansion is complete because its emitted impl is visible
/// to ordinary impl discovery. An unsupported compiler builtin is complete
/// only for a different hygienic builtin trait. An unresolved, ambiguous, or
/// unexpanded proc-macro derive is conservatively incomplete. Unresolved,
/// ambiguous, failed, or depth-limited item macros and unexpanded active
/// attributes are likewise incomplete.
pub(crate) fn module_expansion_complete_for_trait<'db>(
    db: &'db dyn Db,
    module: LocalModSym<'db>,
    target_trait: crate::symbol::TraitSymbol<'db>,
) -> bool {
    for item in module.unexpanded_items(db) {
        if item_has_unexpanded_active_attribute(db, *item) {
            return false;
        }
        match item {
            LocalModItemSym::MacroInvocation(invocation)
                if !bang_macro_expansion_complete(db, module, *invocation, 0) =>
            {
                return false;
            }
            LocalModItemSym::Error(_) => return false,
            LocalModItemSym::MacroInvocation(_)
            | LocalModItemSym::Function(_)
            | LocalModItemSym::Struct(_)
            | LocalModItemSym::Enum(_)
            | LocalModItemSym::Trait(_)
            | LocalModItemSym::Impl(_)
            | LocalModItemSym::TypeAlias(_)
            | LocalModItemSym::Const(_)
            | LocalModItemSym::Static(_)
            | LocalModItemSym::Mod(_)
            | LocalModItemSym::Use(_)
            | LocalModItemSym::MacroDef(_) => {}
        }
    }

    for &symbol in local_expanded_module_items(db, module) {
        let Some(item) = symbol_local_item(db, symbol) else {
            continue;
        };
        if item_has_unexpanded_active_attribute(db, item) {
            return false;
        }
        if !matches!(item, LocalModItemSym::Struct(_) | LocalModItemSym::Enum(_)) {
            continue;
        }
        let Some((attrs_stash, attrs)) = item.attrs(db) else {
            continue;
        };
        for attr in attrs {
            let path = attrs_stash[attr.path];
            let crate::cst::paths::Path::Relative(first, rest) = path else {
                continue;
            };
            if !attrs_stash[rest].is_empty() || first.name.text(db) != "derive" {
                continue;
            }
            for derive_name in parse_derive_names(&attrs_stash[attr.args]) {
                if !derive_complete_for_trait(db, module, item, derive_name, target_trait) {
                    return false;
                }
            }
        }
    }
    true
}

/// Whether represented expansion is known not to hide a method provider.
///
/// Successful bang-macro output is already part of expanded item discovery.
/// Compiler built-in derives cannot add traits or inherent impls, so they do
/// not hide a provider even when Sage does not represent that derive's output.
/// Proc-macro derives and active attributes remain incomplete because they may
/// emit arbitrary items.
pub(crate) fn module_expansion_complete_for_method_providers<'db>(
    db: &'db dyn Db,
    module: LocalModSym<'db>,
) -> bool {
    for item in module.unexpanded_items(db) {
        if item_has_unexpanded_active_attribute(db, *item) {
            return false;
        }
        match item {
            LocalModItemSym::MacroInvocation(invocation)
                if !bang_macro_expansion_complete(db, module, *invocation, 0) =>
            {
                return false;
            }
            LocalModItemSym::Error(_) => return false,
            LocalModItemSym::MacroInvocation(_)
            | LocalModItemSym::Function(_)
            | LocalModItemSym::Struct(_)
            | LocalModItemSym::Enum(_)
            | LocalModItemSym::Trait(_)
            | LocalModItemSym::Impl(_)
            | LocalModItemSym::TypeAlias(_)
            | LocalModItemSym::Const(_)
            | LocalModItemSym::Static(_)
            | LocalModItemSym::Mod(_)
            | LocalModItemSym::Use(_)
            | LocalModItemSym::MacroDef(_) => {}
        }
    }

    for &symbol in local_expanded_module_items(db, module) {
        let Some(item) = symbol_local_item(db, symbol) else {
            continue;
        };
        if item_has_unexpanded_active_attribute(db, item) {
            return false;
        }
        if let LocalModItemSym::Mod(child) = item
            && !module_expansion_complete_for_method_providers(db, child)
        {
            return false;
        }
        if !matches!(item, LocalModItemSym::Struct(_) | LocalModItemSym::Enum(_)) {
            continue;
        }
        let Some((attrs_stash, attrs)) = item.attrs(db) else {
            continue;
        };
        for attr in attrs {
            let crate::cst::paths::Path::Relative(first, rest) = attrs_stash[attr.path] else {
                continue;
            };
            if !attrs_stash[rest].is_empty() || first.name.text(db) != "derive" {
                continue;
            }
            for derive_name in parse_derive_names(&attrs_stash[attr.args]) {
                if resolve_builtin_derive(db, module, derive_name).is_none() {
                    return false;
                }
            }
        }
    }
    true
}

fn derive_complete_for_trait<'db>(
    db: &'db dyn Db,
    module: LocalModSym<'db>,
    item: LocalModItemSym<'db>,
    derive_name: &str,
    target_trait: crate::symbol::TraitSymbol<'db>,
) -> bool {
    let Some((derive_name, _)) = resolve_builtin_derive(db, module, derive_name) else {
        return false;
    };
    if crate::derive::builtins::expand_builtin_derive(db, derive_name.text(db), item).is_some() {
        return true;
    }

    crate::derive::builtins::builtin_derive_trait(db, derive_name.text(db))
        .is_some_and(|derived_trait| derived_trait != target_trait)
}

/// Expand one `#[derive(Name)]` entry into ordinary sibling items.
fn expand_derive<'db>(
    db: &'db dyn Db,
    module: LocalModSym<'db>,
    attribute_index: u32,
    derive_index: u32,
    derive_name: &str,
    item: LocalModItemSym<'db>,
    entries: &mut Vec<Symbol<'db>>,
) {
    let Some((derive_name, macro_def)) = resolve_builtin_derive(db, module, derive_name) else {
        return;
    };

    let expansion = crate::derive::DeriveExpansion::new(
        db,
        derive_name,
        item,
        attribute_index,
        derive_index,
        macro_def,
    );
    if expansion.text(db).is_none() {
        return;
    }
    let generated = expansion.parse_output(db);
    expand_unexpanded_items(db, module, &generated, entries);
}

fn resolve_builtin_derive<'db>(
    db: &'db dyn Db,
    module: LocalModSym<'db>,
    derive_name: &str,
) -> Option<(Name<'db>, MacroDefSymbol<'db>)> {
    let derive_name = Name::new(db, derive_name.to_owned());
    let mut resolver = Resolver::new_for_macro_expansion(db, module);
    let macros: Vec<_> = resolver
        .resolve_name_from_scope(derive_name, Namespace::Macro(MacroKind::Derive))
        .into_iter()
        .filter_map(|resolution| resolution.sym())
        .filter_map(|symbol| symbol_macro_definition(db, symbol))
        .collect();
    let [macro_def] = macros.as_slice() else {
        return None;
    };
    let crate::symbol::MacroDefSymbol::Ext(external_macro) = *macro_def else {
        return None;
    };
    if !db
        .tcx()
        .is_builtin_derive(external_macro.crate_num(db), external_macro.def_index(db))
    {
        return None;
    }
    Some((derive_name, *macro_def))
}
