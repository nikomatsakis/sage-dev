use sage_stash::StashDirect;

use crate::Db;
use crate::local_syms::macro_invocations::LocalMacroInvocationSym;
use crate::name::Name;
use crate::scope::ScopeSymbol;
use crate::span::{AbsoluteSpan, MacroExpansion};

/// A `macro_rules!` definition at item level.
#[salsa::tracked(debug)]
pub struct LocalMacroDefSym<'db> {
    pub name: Name<'db>,
    pub scope: ScopeSymbol<'db>,

    #[tracked]
    #[returns(ref)]
    pub body_tokens: String,

    #[tracked]
    pub span: AbsoluteSpan<'db>,
}

impl StashDirect for LocalMacroDefSym<'_> {}

#[salsa::tracked]
impl<'db> LocalMacroDefSym<'db> {
    #[salsa::tracked]
    pub fn apply_to(
        self,
        db: &'db dyn Db,
        invocation: LocalMacroInvocationSym<'db>,
    ) -> MacroExpansion<'db> {
        unimplemented!(
            "we need to parse the body as macro rules, parse the input according to the arms, etc"
        )
    }
}
