use sage_stash::{Slice, StashDirect, Stashed};

use crate::cst::attrs::AttrCst;
use crate::local_syms::LocalModItemSym;
use crate::local_syms::macro_invocations::LocalMacroInvocationSym;
use crate::name::Name;
use crate::resolve::{Namespace, ResolvePhase};
use crate::scope::ScopeSymbol;
use crate::source::SourceFile;
use crate::span::{AbsoluteSpan, ParseSource};
use crate::symbol::{Symbol, SymbolData};
use crate::{Db, resolve};

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

    pub fn source_root(self, db: &'db dyn Db) -> crate::resolve::SourceRoot {
        self.parent(db)
            .expect("source_root called on crate root")
            .source_root(db)
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

            LocalModItemSym::Function(..)
            | LocalModItemSym::Struct(..)
            | LocalModItemSym::Enum(..)
            | LocalModItemSym::Trait(..)
            | LocalModItemSym::Impl(..)
            | LocalModItemSym::TypeAlias(..)
            | LocalModItemSym::Const(..)
            | LocalModItemSym::Static(..)
            | LocalModItemSym::Mod(..)
            | LocalModItemSym::Use(..)
            | LocalModItemSym::MacroDef(..)
            | LocalModItemSym::Error(..) => {
                entries.push(item.into());
            }
        }
    }
}

/// Maximum nesting depth for macro expansion (same as rustc's default).
const MAX_EXPANSION_DEPTH: usize = 128;

fn expand_macro<'db>(
    db: &'db dyn Db,
    module: LocalModSym<'db>,
    macro_invocation_sym: LocalMacroInvocationSym<'db>,
    entries: &mut Vec<Symbol<'db>>,
) {
    let (macro_stash, macro_cst) = macro_invocation_sym.cst(db).open_deref();
    let macro_path = macro_stash[macro_cst.path];

    let macro_syms = resolve::resolve_path(
        db,
        ResolvePhase::MacroExpansion,
        module.into(),
        macro_stash,
        macro_path,
        Namespace::Macro,
    );

    for sym in macro_syms {
        match sym.data(db) {
            SymbolData::MacroDefSymbol(macro_def_symbol) => {
                let expansion = macro_def_symbol.apply_to(db, macro_invocation_sym);
                crate::parse::parse_str_to_cst(db, expansion.into(), f.text(db), scope)
                    .into_iter()
                    .collect()
            }

            SymbolData::FnSymbol(..)
            | SymbolData::StructSymbol(..)
            | SymbolData::EnumSymbol(..)
            | SymbolData::TraitSymbol(..)
            | SymbolData::TypeAliasSymbol(..)
            | SymbolData::ConstSymbol(..)
            | SymbolData::StaticSymbol(..)
            | SymbolData::ImplSymbol(..)
            | SymbolData::ModSymbol(..)
            | SymbolData::UseSymbol(..)
            | SymbolData::IntrinsicTypeSymbol(..) => {
                panic!("expected only symbols with macro namespace");
            }
        }
    }
}
