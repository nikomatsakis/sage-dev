use crate::Db;
use crate::module::{CrateNum, DefIndex};
use crate::name::Name;
use crate::symbol::Symbol;

/// External crate metadata interface.
///
/// The `sage` binary crate implements this backed by `TyCtxt`.
/// Tests use `NoopTcxDb` which returns empty results.
pub trait TcxDb {
    fn extern_crate(&self, name: &str) -> Option<CrateNum>;

    fn module_children<'db>(
        &self,
        db: &'db dyn Db,
        crate_num: CrateNum,
        def_index: DefIndex,
    ) -> Vec<(Name<'db>, Symbol<'db>)>;

    fn is_builtin_derive(&self, crate_num: CrateNum, def_index: DefIndex) -> bool;
}

/// No-op implementation for tests without rustc.
pub struct NoopTcxDb;

impl TcxDb for NoopTcxDb {
    fn extern_crate(&self, _name: &str) -> Option<CrateNum> {
        None
    }

    fn module_children<'db>(
        &self,
        _db: &'db dyn Db,
        _crate_num: CrateNum,
        _def_index: DefIndex,
    ) -> Vec<(Name<'db>, Symbol<'db>)> {
        Vec::new()
    }

    fn is_builtin_derive(&self, _crate_num: CrateNum, _def_index: DefIndex) -> bool {
        false
    }
}
