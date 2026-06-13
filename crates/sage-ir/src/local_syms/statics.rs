use crate::cst::statics::StaticCst;
use crate::name::Name;
use crate::scope::ScopeSymbol;
use crate::span::AbsoluteSpan;

#[salsa::tracked(debug)]
pub struct LocalStaticSym<'db> {
    pub name: Name<'db>,
    pub scope: ScopeSymbol<'db>,

    #[returns(ref)]
    pub cst: StaticCst<'db>,

    #[tracked]
    pub span: AbsoluteSpan<'db>,
}
