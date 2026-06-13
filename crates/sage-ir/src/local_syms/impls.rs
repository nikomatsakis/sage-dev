use sage_stash::StashDirect;

use crate::cst::impls::ImplCst;
use crate::scope::ScopeSymbol;
use crate::span::AbsoluteSpan;

#[salsa::tracked(debug)]
pub struct LocalImplSym<'db> {
    pub scope: ScopeSymbol<'db>,

    #[returns(ref)]
    pub cst: ImplCst<'db>,

    #[tracked]
    pub span: AbsoluteSpan<'db>,
}

impl StashDirect for LocalImplSym<'_> {}
