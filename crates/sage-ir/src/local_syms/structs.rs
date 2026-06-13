use sage_stash::StashDirect;

use crate::name::Name;
use crate::scope::ScopeSymbol;
use crate::sig_ast::*;
use crate::span::AbsoluteSpan;
use crate::types::Attr;

/// Struct declaration like `struct Foo<A, B> { f: Ty, g: Ty }`
#[salsa::tracked(debug)]
pub struct LocalStructSym<'db> {
    /// Struct name
    pub name: Name<'db>,

    /// Scope in which the struct is declared (module, function, etc)
    pub scope: ScopeSymbol<'db>,

    /// Tuple vs unit vs braced
    pub kind: StructKind,

    /// Attributes like `#[repr(...)]` and so forth
    #[returns(ref)]
    pub attrs: Vec<Attr<'db>>,

    ///
    #[returns(ref)]
    pub signature: StructSigAst<'db>,

    #[tracked]
    pub span: AbsoluteSpan<'db>,
}

impl StashDirect for LocalStructSym<'_> {}

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, salsa::Update)]
pub enum StructKind {
    Tuple,
    Unit,
    Braced,
}
