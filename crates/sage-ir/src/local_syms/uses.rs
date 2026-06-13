use crate::span::AbsoluteSpan;
use crate::types::{Attr, UseImports};

/// A use declaration, desugared into flat imports.
#[salsa::tracked(debug)]
pub struct LocalUseSym<'db> {
    #[returns(ref)]
    pub attrs: Vec<Attr<'db>>,

    #[tracked]
    #[returns(ref)]
    pub imports: UseImports<'db>,

    #[tracked]
    pub span: AbsoluteSpan<'db>,
}
