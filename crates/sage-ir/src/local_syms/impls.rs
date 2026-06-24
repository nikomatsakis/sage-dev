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

impl<'db> LocalImplSym<'db> {
    pub fn attrs(
        self,
        db: &'db dyn crate::Db,
    ) -> (
        &'db sage_stash::Stash,
        &'db [crate::cst::attrs::AttrCst<'db>],
    ) {
        let (stash, data) = self.cst(db).open_deref();
        (stash, &stash[data.attrs])
    }
}
