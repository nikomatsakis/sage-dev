use sage_stash::StashDirect;

use crate::cst::consts::ConstCst;
use crate::local_syms::LocalAssociatedOwner;
use crate::name::Name;
use crate::scope::ScopeSymbol;
use crate::span::AbsoluteSpan;

#[salsa::tracked(debug)]
pub struct LocalConstSym<'db> {
    pub name: Name<'db>,
    pub scope: ScopeSymbol<'db>,
    pub owner: Option<LocalAssociatedOwner<'db>>,

    #[tracked]
    #[returns(ref)]
    pub cst: ConstCst<'db>,

    #[tracked]
    pub span: AbsoluteSpan<'db>,
}

impl StashDirect for LocalConstSym<'_> {}

impl<'db> LocalConstSym<'db> {
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
