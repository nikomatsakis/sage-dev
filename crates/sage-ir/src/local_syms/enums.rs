use sage_stash::StashDirect;

use crate::cst::enums::EnumCst;
use crate::name::Name;
use crate::scope::ScopeSymbol;
use crate::span::AbsoluteSpan;

#[salsa::tracked(debug)]
pub struct LocalEnumSym<'db> {
    pub name: Name<'db>,
    pub scope: ScopeSymbol<'db>,

    #[returns(ref)]
    pub cst: EnumCst<'db>,

    #[tracked]
    pub span: AbsoluteSpan<'db>,
}

impl StashDirect for LocalEnumSym<'_> {}

impl<'db> LocalEnumSym<'db> {
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
