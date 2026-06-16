use sage_stash::StashDirect;

use crate::scope::ScopeSymbol;
use crate::span::AbsoluteSpan;
use crate::types::UseImports;

/// A use declaration, desugared into flat imports.
#[salsa::tracked(debug)]
pub struct LocalUseSym<'db> {
    pub scope: ScopeSymbol<'db>,

    #[tracked]
    #[returns(ref)]
    pub imports: UseImports<'db>,

    #[tracked]
    pub span: AbsoluteSpan<'db>,
}

impl StashDirect for LocalUseSym<'_> {}
