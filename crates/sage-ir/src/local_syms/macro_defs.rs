use sage_stash::StashDirect;

use crate::name::Name;
use crate::span::AbsoluteSpan;

/// A `macro_rules!` definition at item level.
#[salsa::tracked(debug)]
pub struct LocalMacroDefSym<'db> {
    pub name: Name<'db>,

    #[tracked]
    #[returns(ref)]
    pub body_tokens: String,

    #[tracked]
    pub span: AbsoluteSpan<'db>,
}

impl StashDirect for LocalMacroDefSym<'_> {}
