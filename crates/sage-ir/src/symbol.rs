//! `Symbol`: a `Copy` wrapper-of-enum unifying workspace-local
//! (`ItemAst`) and external (`SymExt`) definitions.
//!
//! The wrapper isn't interned — identity flows from the inner data:
//! local symbols inherit the underlying `ItemAst`'s salsa id; external
//! symbols use the structural `(CrateNum, DefIndex)` pair.

use sage_stash::{AllocStashData, StashDirect};

use crate::item::{ItemAst, StructAst};
use crate::module::{CrateNum, DefIndex, ModExt, ModSymbol};

/// A resolved symbol — local item or external definition.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, salsa::Update, AllocStashData)]
pub struct Symbol<'db> {
    data: SymbolData<'db>,
}

impl StashDirect for Symbol<'_> {}

/// External symbol — a thin handle into rustc's metadata. Plain
/// `Copy` struct, structural identity.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, salsa::Update)]
pub struct SymExt {
    pub crate_num: CrateNum,
    pub def_index: DefIndex,
}

impl SymExt {
    pub const fn new(crate_num: CrateNum, def_index: DefIndex) -> Self {
        Self {
            crate_num,
            def_index,
        }
    }
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, salsa::Update)]
pub enum SymbolData<'db> {
    Ast(ItemAst<'db>),
    TupleStructCtor(StructAst<'db>),
    Ext(SymExt),
}

impl<'db> Symbol<'db> {
    pub fn ast(item: ItemAst<'db>) -> Self {
        Self {
            data: SymbolData::Ast(item),
        }
    }

    pub fn tuple_struct_ctor(s: StructAst<'db>) -> Self {
        Self {
            data: SymbolData::TupleStructCtor(s),
        }
    }

    pub fn ext(ext: SymExt) -> Self {
        Self {
            data: SymbolData::Ext(ext),
        }
    }

    pub fn external(crate_num: CrateNum, def_index: DefIndex) -> Self {
        Self::ext(SymExt::new(crate_num, def_index))
    }

    pub fn data(self) -> SymbolData<'db> {
        self.data
    }
}

impl<'db> From<ItemAst<'db>> for Symbol<'db> {
    fn from(item: ItemAst<'db>) -> Self {
        Self::ast(item)
    }
}

impl From<SymExt> for Symbol<'_> {
    fn from(ext: SymExt) -> Self {
        Self::ext(ext)
    }
}

impl<'db> From<ModSymbol<'db>> for Symbol<'db> {
    fn from(m: ModSymbol<'db>) -> Self {
        match m.data() {
            crate::module::ModSymbolData::Ast(ast) => Symbol::ast(ItemAst::Mod(ast)),
            crate::module::ModSymbolData::Ext(ext) => {
                Symbol::ext(SymExt::new(ext.crate_num, ext.def_index))
            }
        }
    }
}

impl From<ModExt> for SymExt {
    fn from(ext: ModExt) -> Self {
        SymExt::new(ext.crate_num, ext.def_index)
    }
}
