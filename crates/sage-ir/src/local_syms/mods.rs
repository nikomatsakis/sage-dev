use sage_stash::{Slice, StashDirect, Stashed};

use crate::Db;
use crate::cst::attrs::AttrCst;
use crate::local_syms::LocalModItemSym;
use crate::local_syms::macro_invocations::LocalMacroInvocationSym;
use crate::name::Name;
use crate::resolve::{MacroKind, Namespace, Resolver};
use crate::scope::ScopeSymbol;
use crate::source::SourceFile;
use crate::span::{AbsoluteSpan, ParseSource};
use crate::symbol::{Symbol, SymbolData};

/// A module written in (or synthesized for) the local workspace.
#[salsa::tracked(debug)]
pub struct LocalModSym<'db> {
    pub name: Name<'db>,

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
            let scope = module
                .parent(db)
                .unwrap_or_else(|| panic!("file-backed module has no parent scope"));
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

/// Cycle recovery initial value.
fn expanded_module_initial<'db>(
    _db: &'db dyn Db,
    _id: salsa::Id,
    _module: LocalModSym<'db>,
) -> Vec<Symbol<'db>> {
    vec![]
}

fn expand_unexpanded_items<'db>(
    db: &'db dyn Db,
    module: LocalModSym<'db>,
    unexpanded_items: &[LocalModItemSym<'db>],
    entries: &mut Vec<Symbol<'db>>,
) {
    for &item in unexpanded_items {
        match item {
            LocalModItemSym::MacroInvocation(sym) => {
                expand_macro(db, module, sym, entries);
            }

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
            | LocalModItemSym::MacroDef(..)
            | LocalModItemSym::Error(..) => {
                expand_attribute_macros_and_derives(db, module, item, entries);
            }
        }
    }
}

fn expand_macro<'db>(
    db: &'db dyn Db,
    module: LocalModSym<'db>,
    macro_invocation_sym: LocalMacroInvocationSym<'db>,
    entries: &mut Vec<Symbol<'db>>,
) {
    let (macro_stash, macro_cst) = macro_invocation_sym.cst(db).open_deref();
    let macro_path = macro_stash[macro_cst.path];

    let mut resolver = Resolver::new_for_macro_expansion(db, module);
    let macro_resolutions =
        resolver.resolve_path(macro_stash, macro_path, Namespace::Macro(MacroKind::Bang));

    for sym in macro_resolutions.into_iter().filter_map(|r| r.sym()) {
        match sym.data(db) {
            SymbolData::MacroDefSymbol(macro_def_symbol) => {
                let unexpanded_items = macro_invocation_sym.parse_output(db, macro_def_symbol);
                expand_unexpanded_items(db, module, &unexpanded_items, entries);
            }

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
            | SymbolData::UseSymbol(..)
            | SymbolData::IntrinsicTypeSymbol(..)
            | SymbolData::MacroInvocationSymbol(..) => {
                panic!("expected only symbols with macro namespace");
            }
        }
    }
}

const INERT_ATTRIBUTES: &[&str] = &["inline", "repr", "allow", "deny", "warn"];

fn expand_attribute_macros_and_derives<'db>(
    db: &'db dyn Db,
    module: LocalModSym<'db>,
    item: LocalModItemSym<'db>,
    entries: &mut Vec<Symbol<'db>>,
) {
    let Some((attrs_stash, attrs)) = item.attrs(db) else {
        entries.push(item.into());
        return;
    };

    if attrs.is_empty() {
        entries.push(item.into());
        return;
    }

    for index in 0..attrs.len() {
        let attr = &attrs[index];
        let path = attrs_stash[attr.path];

        // Look for built-in attribute names (single-segment relative paths)
        if let crate::cst::paths::Path::Relative(first, rest) = path {
            if attrs_stash[rest].is_empty() {
                let text: &str = first.name.text(db);

                if INERT_ATTRIBUTES.contains(&text) {
                    continue;
                }

                if text == "derive" {
                    let args = &attrs_stash[attr.args];
                    expand_derives(db, module, index, args, item, entries);
                }
            }
        }

        // Otherwise, resolve the path
    }
}

/// Expand `#[derive(...)]` attributes on an item.
///
/// The item's source text is extracted from its span and passed to each
/// derive proc-macro. The expanded output (typically impl blocks) is parsed
/// and added to entries.
fn expand_derives<'db>(
    db: &'db dyn Db,
    _module: LocalModSym<'db>,
    skip_attrs: usize,
    _args: &[u8],
    item: LocalModItemSym<'db>,
    _entries: &mut Vec<Symbol<'db>>,
) {
    let _input = attribute_macro_input(db, skip_attrs, item);
}

/// Return the input string to an attribute macro invocation. It consists
/// of the serialized `item`, skipping the first `skip_attrs` attributes.
fn attribute_macro_input<'db>(
    _db: &'db dyn Db,
    _skip_attrs: usize,
    _item: LocalModItemSym<'db>,
) -> String {
    todo!("attribute macro input serialization")
}
