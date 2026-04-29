pub mod body;
pub mod db;
pub mod display;
pub mod item;
pub mod lower;
pub mod name;
pub mod source;
pub mod span;
pub mod types;

/// The salsa database trait for sage-ir.
#[salsa::db]
pub trait Db: salsa::Database {}
