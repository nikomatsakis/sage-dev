use crate::name::Name;
use crate::sig_ast::StaticSigAst;
use crate::span::AbsoluteSpan;
use crate::types::Attr;

#[salsa::tracked(debug)]
pub struct LocalStaticSym<'db> {
    pub name: Name<'db>,

    #[tracked]
    #[returns(ref)]
    pub attrs: Vec<Attr<'db>>,

    #[tracked]
    #[returns(ref)]
    pub signature: StaticSigAst<'db>,

    #[tracked]
    pub is_mut: bool,

    #[tracked]
    pub span: AbsoluteSpan<'db>,
}
