use sage_stash::StashDirect;

use crate::local_syms::consts::LocalConstSym;
use crate::local_syms::enums::LocalEnumSym;
use crate::local_syms::fns::LocalFnSym;
use crate::local_syms::impls::LocalImplSym;
use crate::local_syms::macro_defs::LocalMacroDefSym;
use crate::local_syms::macro_invocations::LocalMacroInvocationSym;
use crate::local_syms::mods::LocalModSym;
use crate::local_syms::statics::LocalStaticSym;
use crate::local_syms::structs::LocalStructSym;
use crate::local_syms::traits::LocalTraitSym;
use crate::local_syms::type_aliases::LocalTypeAliasSym;
use crate::local_syms::uses::LocalUseSym;
use crate::source::SourceFile;
use crate::span::{AbsoluteSpan, ParseSource};

/// Thin enum over all item kinds. `Copy` because salsa tracked struct
/// handles are just IDs.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, salsa::Update)]
pub enum LocalModItemSym<'db> {
    Function(LocalFnSym<'db>),
    Struct(LocalStructSym<'db>),
    Enum(LocalEnumSym<'db>),
    Trait(LocalTraitSym<'db>),
    Impl(LocalImplSym<'db>),
    TypeAlias(LocalTypeAliasSym<'db>),
    Const(LocalConstSym<'db>),
    Static(LocalStaticSym<'db>),
    Mod(LocalModSym<'db>),
    Use(LocalUseSym<'db>),
    MacroDef(LocalMacroDefSym<'db>),
    MacroInvocation(LocalMacroInvocationSym<'db>),
    /// Unrecognized or unsupported item node.
    Error(AbsoluteSpan<'db>),
}

impl StashDirect for LocalModItemSym<'_> {}

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
