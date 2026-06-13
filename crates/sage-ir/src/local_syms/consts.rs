use crate::name::Name;
use crate::sig_ast::ConstSigAst;
use crate::span::AbsoluteSpan;
use crate::types::Attr;

#[salsa::tracked(debug)]
pub struct LocalConstSym<'db> {
    pub name: Name<'db>,

    #[tracked]
    #[returns(ref)]
    pub attrs: Vec<Attr<'db>>,

    #[tracked]
    #[returns(ref)]
    pub signature: ConstSigAst<'db>,

    #[tracked]
    pub span: AbsoluteSpan<'db>,
}
