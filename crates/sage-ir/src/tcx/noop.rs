use crate::symbol::{CrateNum, DefIndex};

use super::{RawChild, TcxDb};

/// No-op implementation for tests without rustc.
pub struct NoopTcxDb;

impl TcxDb for NoopTcxDb {
    fn extern_crate(&self, _name: &str) -> Option<CrateNum> {
        None
    }

    fn module_children(&self, _crate_num: CrateNum, _def_index: DefIndex) -> Vec<RawChild> {
        Vec::new()
    }

    fn item_name(&self, _crate_num: CrateNum, _def_index: DefIndex) -> Option<String> {
        None
    }

    fn is_module(&self, _crate_num: CrateNum, _def_index: DefIndex) -> bool {
        // Noop has no crates, so nothing is a module. In tests that
        // do care, use a mock TcxDb.
        false
    }

    fn is_builtin_derive(&self, _crate_num: CrateNum, _def_index: DefIndex) -> bool {
        false
    }

    fn def_path(&self, _crate_num: CrateNum, _def_index: DefIndex) -> Option<String> {
        None
    }

    fn expand_proc_macro_derive(
        &self,
        _crate_num: CrateNum,
        _def_index: DefIndex,
        _item_source: &str,
    ) -> Option<String> {
        None
    }

    fn expand_proc_macro_bang(
        &self,
        _crate_num: CrateNum,
        _def_index: DefIndex,
        _input_tokens: &str,
    ) -> Option<String> {
        None
    }

    fn expand_proc_macro_attr(
        &self,
        _crate_num: CrateNum,
        _def_index: DefIndex,
        _attr_args: &str,
        _item_source: &str,
    ) -> Option<String> {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn noop_expand_returns_none() {
        let tcx = NoopTcxDb;
        let result = tcx.expand_proc_macro_derive(CrateNum(1), DefIndex(0), "struct Foo;");
        assert!(result.is_none());
    }
}
