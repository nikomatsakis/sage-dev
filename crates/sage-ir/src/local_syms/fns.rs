use crate::body::FunctionBody;
use crate::name::Name;
use crate::sig_ast::*;
use crate::span::AbsoluteSpan;
use crate::types::Attr;

#[salsa::tracked(debug)]
pub struct LocalFnSym<'db> {
    pub name: Name<'db>,

    #[tracked]
    #[returns(ref)]
    pub attrs: Vec<Attr<'db>>,

    #[tracked]
    #[returns(ref)]
    pub signature: FnSigAst<'db>,

    #[tracked]
    pub is_async: bool,

    #[tracked]
    pub is_unsafe: bool,

    #[tracked]
    #[returns(ref)]
    pub body: FunctionBody<'db>,

    #[tracked]
    pub span: AbsoluteSpan<'db>,
}
