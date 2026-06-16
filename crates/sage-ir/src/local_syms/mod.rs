use sage_stash::{AllocStashData, StashDirect};

use crate::source::SourceFile;
use crate::span::{AbsoluteSpan, ParseSource};

/// Thin enum over all item kinds. `Copy` because salsa tracked struct
/// handles are just IDs.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, AllocStashData, salsa::Update)]
pub enum LocalModItemSym<'db> {
    Function(fns::LocalFnSym<'db>),
    Struct(structs::LocalStructSym<'db>),
    Enum(enums::LocalEnumSym<'db>),
    Trait(traits::LocalTraitSym<'db>),
    Impl(impls::LocalImplSym<'db>),
    TypeAlias(type_aliases::LocalTypeAliasSym<'db>),
    Const(consts::LocalConstSym<'db>),
    Static(statics::LocalStaticSym<'db>),
    Mod(mods::LocalModSym<'db>),
    Use(uses::LocalUseSym<'db>),
    MacroDef(macro_defs::LocalMacroDefSym<'db>),
    MacroInvocation(macro_invocations::LocalMacroInvocationSym<'db>),
    /// Unrecognized or unsupported item node.
    Error(AbsoluteSpan<'db>),
}

impl<'db> LocalModItemSym<'db> {
    pub fn absolute_span(&self, db: &'db dyn crate::Db) -> AbsoluteSpan<'db> {
        match *self {
            LocalModItemSym::Function(f) => f.span(db),
            LocalModItemSym::Struct(s) => s.span(db),
            LocalModItemSym::Enum(e) => e.span(db),
            LocalModItemSym::Trait(t) => t.span(db),
            LocalModItemSym::Impl(i) => i.span(db),
            LocalModItemSym::TypeAlias(t) => t.span(db),
            LocalModItemSym::Const(c) => c.span(db),
            LocalModItemSym::Static(s) => s.span(db),
            LocalModItemSym::Mod(m) => m.span(db),
            LocalModItemSym::Use(u) => u.span(db),
            LocalModItemSym::MacroDef(m) => m.span(db),
            LocalModItemSym::MacroInvocation(m) => m.span(db),
            LocalModItemSym::Error(span) => span,
        }
    }

    pub fn parse_source(&self, db: &'db dyn crate::Db) -> ParseSource<'db> {
        self.absolute_span(db).source
    }

    pub fn source_file(&self, db: &'db dyn crate::Db) -> Option<SourceFile> {
        self.absolute_span(db).file()
    }
}

pub mod consts;
pub mod enums;
pub mod fns;
pub mod impls;
pub mod macro_defs;
pub mod macro_invocations;
pub mod mods;
pub mod statics;
pub mod structs;
pub mod traits;
pub mod type_aliases;
pub mod uses;
