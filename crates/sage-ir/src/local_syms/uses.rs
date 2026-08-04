use sage_stash::{Slice, StashDirect, Stashed};

use crate::cst::attrs::AttrCst;
use crate::cst::uses::UseImports;
use crate::scope::ScopeSymbol;
use crate::span::AbsoluteSpan;

/// A use declaration, desugared into flat imports.
#[salsa::tracked(debug)]
pub struct LocalUseSym<'db> {
    pub scope: ScopeSymbol<'db>,

    #[tracked]
    #[returns(ref)]
    pub attrs: Stashed<Slice<AttrCst<'db>>>,

    #[tracked]
    #[returns(ref)]
    pub imports: UseImports<'db>,

    #[tracked]
    pub span: AbsoluteSpan<'db>,
}

impl StashDirect for LocalUseSym<'_> {}

impl<'db> LocalUseSym<'db> {
    pub fn get_attrs(
        self,
        db: &'db dyn crate::Db,
    ) -> (&'db sage_stash::Stash, &'db [AttrCst<'db>]) {
        let (stash, attrs) = self.attrs(db).open();
        (stash, &stash[attrs])
    }
}
