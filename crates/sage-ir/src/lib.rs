pub mod check;
pub mod cst;
pub mod db;
pub mod derive;
pub mod display;
pub mod dump;
pub mod generic_param;
pub mod local_syms;
pub mod lower;
pub mod name;
pub mod parse;
pub mod resolve;
pub mod ribs;
pub mod scope;
pub mod source;
pub mod span;
pub mod symbol;
pub mod tcx;
mod ts_helpers;
pub mod ty;
pub mod ty_fold;
pub mod types;
pub mod tytree;

/// The salsa database trait for sage-ir.
#[salsa::db]
pub trait Db: salsa::Database {
    fn tcx(&self) -> &dyn tcx::TcxDb;
    fn log_query(&self, entry: String);
}
