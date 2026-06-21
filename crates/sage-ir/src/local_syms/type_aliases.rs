use sage_stash::StashDirect;

use crate::cst::type_aliases::TypeAliasCst;
use crate::name::Name;
use crate::scope::ScopeSymbol;
use crate::span::AbsoluteSpan;

#[salsa::tracked(debug)]
pub struct LocalTypeAliasSym<'db> {
    pub name: Name<'db>,
    pub scope: ScopeSymbol<'db>,

    #[returns(ref)]
    pub cst: TypeAliasCst<'db>,

    #[tracked]
    pub span: AbsoluteSpan<'db>,
}

impl StashDirect for LocalTypeAliasSym<'_> {}

impl<'db> LocalTypeAliasSym<'db> {
    pub fn attrs(self, db: &'db dyn crate::Db) -> (&'db sage_stash::Stash, &'db [crate::cst::attrs::AttrCst<'db>]) {
        let (stash, data) = self.cst(db).open_deref();
        (stash, &stash[data.attrs])
    }
}
