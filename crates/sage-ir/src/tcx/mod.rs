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

    /// True iff the given external definition is a module (crate
    /// root, `mod foo`, etc.). Modules are the only DefIds on which
    /// `module_children` is valid to call — asking on a struct or
    /// function makes rustc's `module_children` query panic.
    ///
    /// Callers that convert a `Symbol::External(cn, di)` into a
    /// `ModSymbol::External(cn, di)` must gate the conversion on this
    /// check.
    fn is_module(&self, crate_num: CrateNum, def_index: DefIndex) -> bool;

    fn is_builtin_derive(&self, crate_num: CrateNum, def_index: DefIndex) -> bool;

    /// Human-readable path for an external definition, e.g. `"core::option::Option::Some"`.
    fn def_path(&self, crate_num: CrateNum, def_index: DefIndex) -> Option<String>;

    /// Expand a proc-macro derive. Returns the expanded source text.
    fn expand_proc_macro_derive(
        &self,
        crate_num: CrateNum,
        def_index: DefIndex,
        item_source: &str,
    ) -> Option<String>;
}
