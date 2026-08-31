pub mod check;
pub mod cst;
pub mod db;
pub mod derive;
pub mod diagnostic;
pub mod display;
pub mod dump;
pub mod external_syms;
pub mod generic_param;
pub mod local_syms;
pub mod lower;
pub mod name;
pub mod parse;
pub use check::resolve;
pub mod scope;
pub mod source;
pub mod span;
pub mod symbol;
pub mod tcx;
pub mod tokens;
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
    fn log_inspection_phase(&self, phase: &'static str, entering: bool);
    fn log_inspection_span(
        &self,
        operation: &'static str,
        source: db::InspectionSource,
        child_order: db::InspectionChildOrder,
        entering: bool,
    );
    fn source_file(&self, path: &str) -> Option<source::SourceFile>;
}
