//! Builtin derive expansion (Debug, Clone, Default, Copy, etc.).
//!
//! TODO: This module needs to be rewritten against the new architecture.
//! The old implementation depended on `crate::body`, `crate::item`,
//! and `crate::sig_ast`, all of which have been removed.
