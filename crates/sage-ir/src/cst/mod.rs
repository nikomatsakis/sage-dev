//! Concrete syntax tree nodes.
//!
//! CST nodes are stash-allocated, carry relative spans, and mirror
//! TreeSitter structure. Nothing smart — just syntax.

use sage_stash::StashDirect;

pub mod attrs;
pub mod consts;
pub mod enums;
pub mod expr;
pub mod fns;
pub mod generics;
pub mod impls;
pub mod macro_invocations;
pub mod mods;
pub mod paths;
pub mod statics;
pub mod structs;
pub mod traits;
pub mod ty;
pub mod type_aliases;
pub mod uses;
pub mod where_clause;

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, salsa::Update)]
pub enum Mutability {
    Shared,
    Mut,
}

impl StashDirect for Mutability {}
