use sage_stash::{Slice, StashDirect, Stashed};

use crate::cst::attrs::AttrCst;
use crate::local_syms::LocalModItemSym;
use crate::name::Name;
use crate::scope::ScopeSymbol;
use crate::source::SourceFile;
use crate::span::{AbsoluteSpan, ParseSource};

/// A module written in (or synthesized for) the local workspace.
#[salsa::tracked(debug)]
pub struct LocalModSym<'db> {
    pub name: Name<'db>,

    /// The enclosing module, if any. `None` only for the crate root.
    pub parent: Option<ScopeSymbol<'db>>,

    #[returns(ref)]
    pub body_source: ModBodySource,

    #[returns(ref)]
    pub attrs: Stashed<Slice<AttrCst<'db>>>,

    #[tracked]
    pub span: AbsoluteSpan<'db>,
}

/// Where a module's body (its items) comes from.
#[derive(Clone, Debug, Hash, salsa::Update)]
pub enum ModBodySource {
    /// File-backed: `mod foo;` — items are parsed from the file.
    File(SourceFile),
    /// Inline: `mod foo { ... }` — items are `specify`'d at parse time.
    Inline,
}

impl StashDirect for LocalModSym<'_> {}

impl<'db> LocalModSym<'db> {
    pub fn file(self, db: &'db dyn crate::Db) -> Option<SourceFile> {
        match self.body_source(db) {
            ModBodySource::File(f) => Some(*f),
            ModBodySource::Inline => None,
        }
    }

    pub fn source_root(self, db: &'db dyn crate::Db) -> crate::resolve::SourceRoot {
        self.parent(db)
            .expect("source_root called on crate root")
            .source_root(db)
    }

    pub fn unexpanded_items(self, db: &'db dyn crate::Db) -> &'db [LocalModItemSym<'db>] {
        unexpanded_items(db, self)
    }
}

#[salsa::tracked(specify, returns(ref))]
pub fn unexpanded_items<'db>(
    db: &'db dyn crate::Db,
    module: LocalModSym<'db>,
) -> Vec<LocalModItemSym<'db>> {
    match module.body_source(db) {
        ModBodySource::File(f) => {
            let source = ParseSource::SourceFile(*f);
            let scope = module
                .parent(db)
                .unwrap_or_else(|| panic!("file-backed module has no parent scope"));
            crate::parse::parse_str_to_cst(db, source, f.text(db), scope)
        }
        ModBodySource::Inline => {
            panic!("unexpanded_items should be specify'd for inline modules")
        }
    }
}
