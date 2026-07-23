use crate::Db;
use crate::local_syms::mods::LocalModSym;
use sage_macros_from_impls::FromImpls;

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, salsa::Update)]
pub enum Edition {
    Rust2015,
    Rust2018,
    Rust2021,
    Rust2024,
}

impl Edition {
    pub fn parse(text: &str) -> Option<Self> {
        match text {
            "2015" => Some(Self::Rust2015),
            "2018" => Some(Self::Rust2018),
            "2021" => Some(Self::Rust2021),
            "2024" => Some(Self::Rust2024),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Rust2015 => "2015",
            Self::Rust2018 => "2018",
            Self::Rust2021 => "2021",
            Self::Rust2024 => "2024",
        }
    }

    pub(crate) fn prelude_module(self) -> &'static str {
        match self {
            Self::Rust2015 => "rust_2015",
            Self::Rust2018 => "rust_2018",
            Self::Rust2021 => "rust_2021",
            Self::Rust2024 => "rust_2024",
        }
    }
}

/// A local crate: bundles the root module with its source root.
/// The driver creates one of these per workspace crate.
#[salsa::tracked(debug)]
pub struct LocalCrateSymbol<'db> {
    pub root_mod: LocalModSym<'db>,
    pub edition: Edition,
}

impl sage_stash::StashDirect for LocalCrateSymbol<'_> {}

/// Create a `LocalCrateSymbol`. Tracked-struct creation requires a
/// tracked fn context — use this instead of `LocalCrateSymbol::new` directly.
#[salsa::tracked]
pub fn local_crate<'db>(db: &'db dyn Db, root_mod: LocalModSym<'db>) -> LocalCrateSymbol<'db> {
    local_crate_with_edition(db, root_mod, Edition::Rust2021)
}

#[salsa::tracked]
pub fn local_crate_with_edition<'db>(
    db: &'db dyn Db,
    root_mod: LocalModSym<'db>,
    edition: Edition,
) -> LocalCrateSymbol<'db> {
    assert_eq!(
        root_mod.edition(db),
        edition,
        "a crate and its root module must use the same Rust edition"
    );
    LocalCrateSymbol::new(db, root_mod, edition)
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
