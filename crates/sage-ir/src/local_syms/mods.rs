use sage_stash::StashDirect;

use crate::local_syms::LocalModItemSym;
use crate::name::Name;
use crate::scope::ScopeSymbol;
use crate::source::SourceFile;
use crate::span::AbsoluteSpan;

/// A module written in (or synthesized for) the local workspace.
#[salsa::tracked(debug)]
pub struct LocalModSym<'db> {
    pub name: Name<'db>,

    /// The enclosing module, if any. `None` only for the crate root.
    pub parent: Option<ScopeSymbol<'db>>,

    #[returns(ref)]
    pub source: ModSource<'db>,

    #[tracked]
    pub span: AbsoluteSpan<'db>,
}

#[derive(Clone, Debug, Hash, salsa::Update)]
pub enum ModSource<'db> {
    File(SourceFile),
    Inline(Vec<LocalModItemSym<'db>>),
}

impl StashDirect for LocalModSym<'_> {}

#[salsa::tracked]
impl<'db> LocalModSym<'db> {}
