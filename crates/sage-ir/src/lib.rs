#![feature(register_tool)]
#![register_tool(sage)]
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
pub mod well_known;

/// The salsa database trait for sage-ir.
#[salsa::db]
pub trait Db: salsa::Database {
    fn configuration(&self) -> db::Configuration;
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

// Source anchors for architectural modules and distributed representations.
mod sage_model {
    /// Architectural module anchor.
    #[sage::item(crate::checking)]
    mod checking_model {}
    /// Groups expression and pattern checking implementations across CST types.
    #[sage::item(crate::checking::BodyCheck)]
    mod checking_body_check_model {}
    /// Architectural crate anchor.
    #[sage::item(crate)]
    mod crate_model {}
    /// Architectural module anchor.
    #[sage::item(crate::inference)]
    mod inference_model {}
    /// Architectural module anchor.
    #[sage::item(crate::metadata)]
    mod metadata_model {}
    /// Groups the keyed lowering functions in external_syms.rs.
    #[sage::item(crate::metadata::ExternalFacts)]
    mod metadata_external_facts_model {}
    /// Architectural module anchor.
    #[sage::item(crate::modules)]
    mod modules_model {}
    /// Groups module expansion and derive functions; no Expansion type.
    #[sage::item(crate::modules::Expansion)]
    mod modules_expansion_model {}
    /// Architectural module anchor.
    #[sage::item(crate::resolution)]
    mod resolution_model {}
    /// Architectural module anchor.
    #[sage::item(crate::solver)]
    mod solver_model {}
    /// Groups canonical solver functions in check::solve; no Solver type.
    #[sage::item(crate::solver::Solver)]
    mod solver_solver_model {}
    /// Architectural module anchor.
    #[sage::item(crate::symbols)]
    mod symbols_model {}
    /// Architectural module anchor.
    #[sage::item(crate::types)]
    mod types_model {}
    /// Logical representation; implementation details are documented in the Sage declaration.
    #[sage::item(crate::types::Owner)]
    mod types_owner_model {}
    /// Groups per-item CST types; there is no single Rust Syntax type.
    #[sage::item(crate::types::Syntax)]
    mod types_syntax_model {}
    /// Review-only sum of the separate signature result types in ty.rs.
    #[sage::item(crate::types::Signature)]
    mod types_signature_model {}
    /// Logical representation; implementation details are documented in the Sage declaration.
    #[sage::item(crate::types::Items)]
    mod types_items_model {}
    /// Groups fields, associated values, and declared types returned by distinct Rust operations.
    #[sage::item(crate::types::Value)]
    mod types_value_model {}
    /// Groups resolved values and failure outcomes across resolver operations.
    #[sage::item(crate::types::Resolution)]
    mod types_resolution_model {}
    /// Groups lexical ribs, generic parameters, and solver assumptions; no single environment type.
    #[sage::item(crate::types::Environment)]
    mod types_environment_model {}
    /// Groups LocalImplCandidates and ExternalImplCandidates with their completeness evidence.
    #[sage::item(crate::types::Candidates)]
    mod types_candidates_model {}
    /// Groups the owned Raw metadata types supplied by TcxDb.
    #[sage::item(crate::types::Metadata)]
    mod types_metadata_model {}
    /// Logical representation; implementation details are documented in the Sage declaration.
    #[sage::item(crate::types::Unit)]
    mod types_unit_model {}
    /// Groups file and generated sources, their text, and module body provenance.
    #[sage::item(crate::types::Source)]
    mod types_source_model {}
    /// Groups external SymExt and scoped identity families.
    #[sage::item(crate::types::Identity)]
    mod types_identity_model {}
    /// Logical representation; implementation details are documented in the Sage declaration.
    #[sage::item(crate::types::Boolean)]
    mod types_boolean_model {}
    /// Logical representation; implementation details are documented in the Sage declaration.
    #[sage::item(crate::types::OptionalScope)]
    mod types_optional_scope_model {}
}
