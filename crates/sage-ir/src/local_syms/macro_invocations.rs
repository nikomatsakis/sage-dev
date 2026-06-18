use sage_stash::StashDirect;

use crate::cst::macro_invocations::MacroInvocationCst;
use crate::scope::ScopeSymbol;
use crate::span::AbsoluteSpan;

/// An item-level macro invocation (e.g. `m!()` or `foo::bar::m!()`).
#[salsa::tracked(debug)]
pub struct LocalMacroInvocationSym<'db> {
    pub scope: ScopeSymbol<'db>,

    pub cst: MacroInvocationCst<'db>,

    #[tracked]
    pub span: AbsoluteSpan<'db>,
}

impl StashDirect for LocalMacroInvocationSym<'_> {}
