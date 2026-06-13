use crate::name::Name;
use crate::scope::ScopeSymbol;
use crate::span::AbsoluteSpan;

/// An item-level macro invocation (e.g. `m!()` or `foo::bar::m!()`).
#[salsa::tracked(debug)]
pub struct LocalMacroInvocationSym<'db> {
    pub scope: ScopeSymbol<'db>,

    #[returns(ref)]
    pub path: Vec<Name<'db>>,

    #[returns(ref)]
    pub input_tokens: String,

    #[tracked]
    pub span: AbsoluteSpan<'db>,
}
