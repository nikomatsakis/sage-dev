use sage_stash::{Slice, StashDirect, Stashed};

use crate::cst::attrs::AttrCst;
use crate::name::Name;
use crate::scope::ScopeSymbol;
use crate::span::AbsoluteSpan;

/// A `macro_rules!` definition at item level.
#[salsa::tracked(debug)]
pub struct LocalMacroDefSym<'db> {
    pub name: Name<'db>,
    pub scope: ScopeSymbol<'db>,

    #[tracked]
    #[returns(ref)]
    pub attrs: Stashed<Slice<AttrCst<'db>>>,

    /// The RHS of the first (and only supported) rule, with outer braces
    /// stripped. Only the `() => { ... }` form is handled; empty if the
    /// LHS pattern is non-trivial.
    #[tracked]
    #[returns(ref)]
    pub body_tokens: String,

    #[tracked]
    pub span: AbsoluteSpan<'db>,
}

impl StashDirect for LocalMacroDefSym<'_> {}

impl<'db> LocalMacroDefSym<'db> {
    pub fn get_attrs(
        self,
        db: &'db dyn crate::Db,
    ) -> (&'db sage_stash::Stash, &'db [AttrCst<'db>]) {
        let (stash, attrs) = self.attrs(db).open();
        (stash, &stash[attrs])
    }
}
