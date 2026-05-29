//! Module symbols: workspace-local (`ModAst`) and external (`SymExt`),
//! unified by the `ModSymbol` wrapper-of-enum.
//!
//! `ModSymbol` is a plain `Copy` newtype around a `ModSymbolData` enum.
//! Identity for the local arm comes from `ModAst`'s salsa tracked-struct
//! id (per-resolution-site); identity for the external arm is structural
//! (`(CrateNum, DefIndex)` via `SymExt`).

use sage_stash::StashDirect;

use crate::source::SourceFile;
use crate::symbol::{SymExt, SymExtKind};

/// Opaque crate number (matches rustc's CrateNum).
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, salsa::Update)]
pub struct CrateNum(pub u32);

impl StashDirect for CrateNum {}

/// Opaque definition index within a crate.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, salsa::Update)]
pub struct DefIndex(pub u32);

impl StashDirect for DefIndex {}

/// A module symbol — a `Copy` wrapper-of-enum unifying local
/// (`ModAst`) and external (`SymExt`) modules.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, salsa::Update)]
pub struct ModSymbol<'db> {
    data: ModSymbolData<'db>,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, salsa::Update)]
pub enum ModSymbolData<'db> {
    Ast(crate::item::ModAst<'db>),
    Ext(SymExt),
}

impl<'db> From<crate::item::ModAst<'db>> for ModSymbol<'db> {
    fn from(ast: crate::item::ModAst<'db>) -> Self {
        Self {
            data: ModSymbolData::Ast(ast),
        }
    }
}

impl From<SymExt> for ModSymbol<'_> {
    fn from(ext: SymExt) -> Self {
        Self {
            data: ModSymbolData::Ext(ext),
        }
    }
}

impl<'db> ModSymbol<'db> {
    pub fn ast(ast: crate::item::ModAst<'db>) -> Self {
        Self::from(ast)
    }

    pub fn ext(ext: SymExt) -> Self {
        Self::from(ext)
    }

    pub fn external(crate_num: CrateNum, def_index: DefIndex) -> Self {
        Self::ext(SymExt::new(crate_num, def_index, SymExtKind::Mod))
    }

    pub fn data(self) -> ModSymbolData<'db> {
        self.data
    }

    /// The source file that backs this module's contents.
    /// Walks up through inline-mod parents (which have `file = None`)
    /// to find the enclosing file. Returns `None` for external modules.
    pub fn containing_file(self, db: &'db dyn crate::Db) -> Option<SourceFile> {
        let mut current = match self.data {
            ModSymbolData::Ast(ast) => ast,
            ModSymbolData::Ext(_) => return None,
        };
        loop {
            if let Some(file) = current.file(db) {
                return Some(file);
            }
            match current.parent(db).map(|p| p.data) {
                Some(ModSymbolData::Ast(parent_ast)) => current = parent_ast,
                _ => return None,
            }
        }
    }

    /// The module one level up. `None` for the crate root and for
    /// external modules.
    pub fn parent(self, db: &'db dyn crate::Db) -> Option<ModSymbol<'db>> {
        match self.data {
            ModSymbolData::Ast(ast) => ast.parent(db),
            ModSymbolData::Ext(_) => None,
        }
    }

    /// Walk up the parent chain to the crate root. For external
    /// modules, returns the external crate's root module.
    pub fn crate_root(self, db: &'db dyn crate::Db) -> ModSymbol<'db> {
        match self.data {
            ModSymbolData::Ast(_) => {
                let mut current = self;
                while let Some(p) = current.parent(db) {
                    current = p;
                }
                current
            }
            ModSymbolData::Ext(ext) => ModSymbol::external(ext.crate_num, DefIndex(0)),
        }
    }
}
