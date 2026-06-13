use crate::name::Name;
use crate::sig_ast::*;
use crate::span::AbsoluteSpan;
use crate::types::Attr;

#[salsa::tracked(debug)]
pub struct LocalTypeAliasSym<'db> {
    pub name: Name<'db>,

    #[returns(ref)]
    pub attrs: Vec<Attr<'db>>,

    #[tracked]
    #[returns(ref)]
    pub signature: TypeAliasSigAst<'db>,

    #[tracked]
    pub span: AbsoluteSpan<'db>,
}
