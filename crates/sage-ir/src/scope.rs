use crate::Db;
use crate::local_syms::mods::LocalModSym;
use sage_macros_from_impls::FromImpls;

/// A local crate: bundles the root module with its source root.
/// The driver creates one of these per workspace crate.
#[salsa::tracked(debug)]
pub struct LocalCrateSymbol<'db> {
    pub root_mod: LocalModSym<'db>,
}

impl sage_stash::StashDirect for LocalCrateSymbol<'_> {}

/// Create a `LocalCrateSymbol`. Tracked-struct creation requires a
/// tracked fn context — use this instead of `LocalCrateSymbol::new` directly.
#[salsa::tracked]
pub fn local_crate<'db>(db: &'db dyn Db, root_mod: LocalModSym<'db>) -> LocalCrateSymbol<'db> {
    LocalCrateSymbol::new(db, root_mod)
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, FromImpls, salsa::Update)]
pub enum ScopeSymbol<'db> {
    Crate(LocalCrateSymbol<'db>),
    Module(LocalModSym<'db>),
}

impl<'db> ScopeSymbol<'db> {
    pub fn module(&self, db: &'db dyn Db) -> LocalModSym<'db> {
        match self {
            ScopeSymbol::Crate(c) => c.root_mod(db),
            ScopeSymbol::Module(m) => *m,
        }
    }

    /// Return the local crate that owns this scope.
    pub fn local_crate(self, db: &'db dyn Db) -> LocalCrateSymbol<'db> {
        let mut scope = self;
        loop {
            match scope {
                ScopeSymbol::Crate(local_crate) => return local_crate,
                ScopeSymbol::Module(module) => {
                    scope = module
                        .parent(db)
                        .expect("a local module must eventually reach its crate scope");
                }
            }
        }
    }
}
