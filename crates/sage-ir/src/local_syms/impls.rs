use sage_stash::StashDirect;

use crate::item::LocalModItemSym;
use crate::sig_ast::*;
use crate::span::AbsoluteSpan;
use crate::types::Attr;

#[salsa::tracked(debug)]
pub struct LocalImplSym<'db> {
    #[returns(ref)]
    pub attrs: Vec<Attr<'db>>,

    #[tracked]
    #[returns(ref)]
    pub signature: ImplSigAst<'db>,

    #[returns(ref)]
    pub items: Vec<LocalModItemSym<'db>>,

    #[tracked]
    pub span: AbsoluteSpan<'db>,
}

impl StashDirect for LocalImplSym<'_> {}
