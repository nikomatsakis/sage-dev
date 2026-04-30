use crate::item::Item;
use crate::module::{CrateNum, DefIndex};

/// A resolved symbol — either a local item or an external definition.
#[salsa::interned]
pub struct Symbol<'db> {
    pub source: SymbolSource<'db>,
}

/// Where a symbol's definition comes from.
#[derive(Copy, Clone, PartialEq, Eq, Hash, salsa::Update)]
pub enum SymbolSource<'db> {
    Local(Item<'db>),
    External(CrateNum, DefIndex),
}
