use crate::source::SourceFile;

/// Opaque crate number (matches rustc's CrateNum).
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, salsa::Update)]
pub struct CrateNum(pub u32);

/// Opaque definition index within a crate.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, salsa::Update)]
pub struct DefIndex(pub u32);

/// A resolved module — either a local source file or an external crate module.
#[salsa::interned(debug)]
pub struct Module<'db> {
    pub source: ModuleSource<'db>,
}

/// Where a module's content comes from.
#[derive(Clone, Debug, PartialEq, Eq, Hash, salsa::Update)]
pub enum ModuleSource<'db> {
    /// Workspace module backed by a source file.
    Local {
        file: SourceFile,
        parent: Option<Module<'db>>,
    },
    /// External crate module, queryable via TcxDb.
    External(CrateNum, DefIndex),
}
