//! Derive macro expansion.

use crate::local_syms::LocalModItemSym;
use crate::name::Name;
use crate::span::AbsoluteSpan;
use crate::symbol::MacroDefSymbol;

/// Parseable source synthesized by one derive invocation.
///
/// Identity is the stable source item, attribute/list occurrence, resolved
/// macro definition, and derive name. Generated text and origin coordinates
/// are deliberately not identity fields: moving an item must not remint all
/// symbols produced by its derive.
#[salsa::interned(debug)]
pub struct DeriveExpansion<'db> {
    pub derive_name: Name<'db>,
    pub source_item: LocalModItemSym<'db>,
    pub attribute_index: u32,
    pub derive_index: u32,
    pub macro_def: MacroDefSymbol<'db>,
}

impl<'db> DeriveExpansion<'db> {
    pub fn origin(self, db: &'db dyn crate::Db) -> AbsoluteSpan<'db> {
        self.source_item(db).absolute_span(db)
    }

    pub fn text(self, db: &'db dyn crate::Db) -> Option<&'db str> {
        builtin_derive_output(db, self).as_deref()
    }
}

#[salsa::tracked]
impl<'db> DeriveExpansion<'db> {
    /// Parse generated items behind a stable tracked boundary so the module
    /// expansion fixed point observes the same symbol identities on retries.
    #[salsa::tracked(returns(ref))]
    pub fn parse_output(self, db: &'db dyn crate::Db) -> Vec<LocalModItemSym<'db>> {
        let Some(output) = self.text(db) else {
            return Vec::new();
        };
        let scope = match self.source_item(db) {
            LocalModItemSym::Struct(struct_sym) => struct_sym.scope(db),
            LocalModItemSym::Enum(enum_sym) => enum_sym.scope(db),
            LocalModItemSym::Function(_)
            | LocalModItemSym::Trait(_)
            | LocalModItemSym::Impl(_)
            | LocalModItemSym::TypeAlias(_)
            | LocalModItemSym::Const(_)
            | LocalModItemSym::Static(_)
            | LocalModItemSym::Mod(_)
            | LocalModItemSym::Use(_)
            | LocalModItemSym::MacroDef(_)
            | LocalModItemSym::MacroInvocation(_)
            | LocalModItemSym::Error(_) => return Vec::new(),
        };
        crate::parse::parse_str_to_cst(db, crate::span::ParseSource::Derive(self), output, scope)
    }
}

#[salsa::tracked(returns(ref))]
pub(crate) fn builtin_derive_output<'db>(
    db: &'db dyn crate::Db,
    expansion: DeriveExpansion<'db>,
) -> Option<String> {
    crate::derive::builtins::expand_builtin_derive(
        db,
        expansion.derive_name(db).text(db),
        expansion.source_item(db),
    )
}

pub mod builtins;
