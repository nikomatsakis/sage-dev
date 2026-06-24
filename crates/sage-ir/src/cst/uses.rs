use sage_stash::{AllocStashData, Ptr, Slice, Stashed};

use crate::cst::paths::Path;
use crate::name::Name;
use crate::span::RelativeSpan;

/// A single flattened use import (stash-allocated).
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, AllocStashData)]
pub struct UseImportCst<'db> {
    /// The full path as a recursive Path CST node.
    pub path: Ptr<Path<'db>>,
    pub kind: UseKind<'db>,
    pub(crate) span: RelativeSpan,
}

/// The stashed collection of use imports for a `use` declaration.
pub type UseImports<'db> = Stashed<Slice<UseImportCst<'db>>>;

/// What a use import brings into scope.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, salsa::Update, AllocStashData)]
pub enum UseKind<'db> {
    /// `use foo::bar` or `use foo::bar as baz` — imports under the given name.
    Named(Name<'db>),
    /// `use foo::bar::*` — glob import.
    Glob,
    /// `use foo::Bar as _` — unnamed import.
    Unnamed,
}
