//! Lower tree-sitter CST nodes into salsa tracked IR structs.
//!
//! TODO: This module needs to be rewritten against the new architecture.
//! The old implementation depended on `crate::item`, `crate::sig_ast`,
//! and `crate::body`, all of which have been removed.
//!
//! Public entry points that other modules depended on:
//! - `parse_source_file(db, file) -> Vec<LocalModItemSym>`
//! - `parse_macro_expansion(db, exp) -> Vec<LocalModItemSym>`
//! - `parse_source_text(db, source, text) -> Vec<LocalModItemSym>`
