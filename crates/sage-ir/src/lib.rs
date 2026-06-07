pub mod body;
pub mod body_resolve;
pub mod db;
pub mod derive;
pub mod display;
pub mod dump;
pub mod generic_param;
pub mod item;
pub mod lower;
pub mod memmap;
pub mod module;
pub mod name;
pub mod resolve;
pub mod resolved;
pub mod ribs;
pub mod sig_ast;
pub mod sig_lower;
pub mod source;
pub mod span;
pub mod symbol;
pub mod tcx;
mod ts_helpers;
pub mod ty;
pub mod ty_fold;
pub mod types;

/// The salsa database trait for sage-ir.
#[salsa::db]
pub trait Db: salsa::Database {
    fn tcx(&self) -> &dyn tcx::TcxDb;
    fn log_query(&self, entry: String);
}
