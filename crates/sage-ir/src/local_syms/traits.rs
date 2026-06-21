use sage_stash::StashDirect;

use crate::cst::traits::TraitCst;
use crate::name::Name;
use crate::scope::ScopeSymbol;
use crate::span::AbsoluteSpan;

#[salsa::tracked(debug)]
pub struct LocalTraitSym<'db> {
    pub name: Name<'db>,
    pub scope: ScopeSymbol<'db>,

    #[returns(ref)]
    pub cst: TraitCst<'db>,

    #[tracked]
    pub span: AbsoluteSpan<'db>,
}

impl StashDirect for LocalTraitSym<'_> {}

impl<'db> LocalTraitSym<'db> {
    pub fn attrs(self, db: &'db dyn crate::Db) -> (&'db sage_stash::Stash, &'db [crate::cst::attrs::AttrCst<'db>]) {
        let (stash, data) = self.cst(db).open_deref();
        (stash, &stash[data.attrs])
    }
}
