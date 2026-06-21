use sage_stash::StashDirect;

use crate::cst::statics::StaticCst;
use crate::name::Name;
use crate::scope::ScopeSymbol;
use crate::span::AbsoluteSpan;

#[salsa::tracked(debug)]
pub struct LocalStaticSym<'db> {
    pub name: Name<'db>,
    pub scope: ScopeSymbol<'db>,

    #[returns(ref)]
    pub cst: StaticCst<'db>,

    #[tracked]
    pub span: AbsoluteSpan<'db>,
}

impl StashDirect for LocalStaticSym<'_> {}

impl<'db> LocalStaticSym<'db> {
    pub fn attrs(self, db: &'db dyn crate::Db) -> (&'db sage_stash::Stash, &'db [crate::cst::attrs::AttrCst<'db>]) {
        let (stash, data) = self.cst(db).open_deref();
        (stash, &stash[data.attrs])
    }
}
