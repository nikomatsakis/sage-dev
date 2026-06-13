use sage_stash::StashDirect;

use crate::item::LocalModItemSym;
use crate::name::Name;
use crate::sig_ast::*;
use crate::span::AbsoluteSpan;
use crate::types::Attr;

#[salsa::tracked(debug)]
pub struct LocalTraitSym<'db> {
    pub name: Name<'db>,

    #[returns(ref)]
    pub attrs: Vec<Attr<'db>>,

    #[returns(ref)]
    pub signature: TraitSigAst<'db>,

    #[tracked]
    #[returns(ref)]
    pub items: Vec<LocalModItemSym<'db>>,

    #[tracked]
    pub span: AbsoluteSpan<'db>,
}

impl StashDirect for LocalTraitSym<'_> {}
