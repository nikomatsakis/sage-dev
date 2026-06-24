use sage_stash::StashDirect;

use crate::name::Name;
use crate::scope::ScopeSymbol;
use crate::span::AbsoluteSpan;

/// A `macro_rules!` definition at item level.
#[salsa::tracked(debug)]
pub struct LocalMacroDefSym<'db> {
    pub name: Name<'db>,
    pub scope: ScopeSymbol<'db>,

    /// The RHS of the first (and only supported) rule, with outer braces
    /// stripped. Only the `() => { ... }` form is handled; empty if the
    /// LHS pattern is non-trivial.
    #[tracked]
    #[returns(ref)]
    pub body_tokens: String,

    #[tracked]
    pub span: AbsoluteSpan<'db>,
}

impl StashDirect for LocalMacroDefSym<'_> {}
