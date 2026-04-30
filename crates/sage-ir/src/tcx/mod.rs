mod noop;
mod proxy;

pub use noop::NoopTcxDb;
pub use proxy::{ProxyTcxDb, TcxRequest};

use crate::module::{CrateNum, DefIndex};
use crate::resolve::Namespace;

/// A single child of an external module — raw owned data, no salsa interning.
#[derive(Clone, Debug)]
pub struct RawChild {
    pub name: String,
    pub crate_num: CrateNum,
    pub def_index: DefIndex,
    pub namespace: Namespace,
}

/// External crate metadata interface.
///
/// Returns only owned, `'static` data. The caller is responsible for
/// interning into salsa types (`Name`, `Symbol`). This keeps the trait
/// free of salsa lifetimes, enabling channel-based implementations.
pub trait TcxDb: Send + Sync {
    fn extern_crate(&self, name: &str) -> Option<CrateNum>;

    fn module_children(&self, crate_num: CrateNum, def_index: DefIndex) -> Vec<RawChild>;

    fn is_builtin_derive(&self, crate_num: CrateNum, def_index: DefIndex) -> bool;
}
