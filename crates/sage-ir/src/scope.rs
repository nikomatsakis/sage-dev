use crate::Db;
use crate::item::ModAst;
use crate::resolve::SourceRoot;

/// A local crate: bundles the root module with its source root.
/// The driver creates one of these per workspace crate.
#[salsa::tracked(debug)]
pub struct LocalCrateSymbol<'db> {
    pub root_mod: ModAst<'db>,
    pub source_root: SourceRoot,
}

/// Create a `LocalCrateSymbol`. Tracked-struct creation requires a
/// tracked fn context — use this instead of `LocalCrateSymbol::new` directly.
#[salsa::tracked]
pub fn local_crate<'db>(
    db: &'db dyn Db,
    root_mod: ModAst<'db>,
    source_root: SourceRoot,
) -> LocalCrateSymbol<'db> {
    LocalCrateSymbol::new(db, root_mod, source_root)
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, salsa::Update)]
pub enum ScopeSymbol<'db> {
    Crate(LocalCrateSymbol<'db>),
    Module(ModAst<'db>),
}
