use sage_stash::StashDirect;

use crate::name::Name;
use crate::sig_ast::*;
use crate::span::AbsoluteSpan;
use crate::types::Attr;

#[salsa::tracked(debug)]
pub struct LocalEnumSym<'db> {
    /// Struct name
    pub name: Name<'db>,

    /// Attributes like `#[repr(...)]` and so forth
    #[returns(ref)]
    pub attrs: Vec<Attr<'db>>,

    #[returns(ref)]
    pub signature: EnumSigAst<'db>,

    #[tracked]
    pub span: AbsoluteSpan<'db>,
}

impl StashDirect for LocalEnumSym<'_> {}
