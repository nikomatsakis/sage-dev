use crate::Db;
use crate::local_syms::mods::LocalModSym;
use crate::resolve::SourceRoot;

/// A local crate: bundles the root module with its source root.
/// The driver creates one of these per workspace crate.
#[salsa::tracked(debug)]
pub struct LocalCrateSymbol<'db> {
    pub root_mod: LocalModSym<'db>,
    pub source_root: SourceRoot,
}

/// Create a `LocalCrateSymbol`. Tracked-struct creation requires a
/// tracked fn context — use this instead of `LocalCrateSymbol::new` directly.
#[salsa::tracked]
pub fn local_crate<'db>(
    db: &'db dyn Db,
    root_mod: LocalModSym<'db>,
    source_root: SourceRoot,
) -> LocalCrateSymbol<'db> {
    LocalCrateSymbol::new(db, root_mod, source_root)
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, salsa::Update)]
pub enum ScopeSymbol<'db> {
    Crate(LocalCrateSymbol<'db>),
    Module(LocalModSym<'db>, SourceRoot),
}

impl<'db> ScopeSymbol<'db> {
    pub fn source_root(&self, db: &'db dyn Db) -> SourceRoot {
        match self {
            ScopeSymbol::Crate(c) => c.source_root(db),
            ScopeSymbol::Module(_, sr) => *sr,
        }
    }

    pub fn module(&self, db: &'db dyn Db) -> LocalModSym<'db> {
        match self {
            ScopeSymbol::Crate(c) => c.root_mod(db),
            ScopeSymbol::Module(m, _) => *m,
        }
    }
}
