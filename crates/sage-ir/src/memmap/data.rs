//! Data model types for the MEM-map.
//!
//! Given this Rust source:
//! ```text
//! struct Foo;
//! use bar::Baz;
//! macro_rules! m { () => { struct X; } }
//! m!();
//! ```
//!
//! The module's MEM-map contains:
//! - `Named { name: "Foo", ns: Type, kind: Item(...) }`
//! - `Named { name: "Foo", ns: Value, kind: Item(...) }` (unit struct constructor)
//! - `Named { name: "Baz", ns: Type, kind: Redirect { target: "bar::Baz" } }`
//! - `Named { name: "m", ns: Macro(Bang), kind: MacroDef(...) }`
//! - `MacroUse { path: "m", state: Expanded([Named { name: "X", ns: Type, ... }]) }`

use crate::item::{Item, MacroDefItem};
use crate::module::Module;
use crate::name::Name;
use crate::resolve::Namespace;
use crate::types::Path;

/// A single entry in the MEM-map.
#[derive(Clone, Debug, PartialEq, Eq, Hash, salsa::Update)]
pub enum MemmapEntry<'db> {
    /// A named member (item, use-redirect, or macro definition).
    Named(NamedMember<'db>),
    /// A macro invocation with its resolution state.
    MacroUse(MacroUse<'db>),
    /// A glob import stem (`use foo::*` → records the source module).
    Glob(GlobStem<'db>),
    /// An anonymous item (e.g. `impl` blocks) that introduces no names.
    Anon(Item<'db>),
}

/// A named member in the module's MEM-map.
#[derive(Clone, Debug, PartialEq, Eq, Hash, salsa::Update)]
pub struct NamedMember<'db> {
    pub name: Name<'db>,
    pub ns: Namespace,
    pub kind: NamedMemberKind<'db>,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash, salsa::Update)]
pub enum NamedMemberKind<'db> {
    /// A regular item (struct, enum, fn, mod, const, etc.)
    Item(Item<'db>),
    /// A use-redirect: `use foo::bar` or `use foo::bar as baz`
    Redirect { target: Path<'db> },
    /// A `macro_rules!` definition
    MacroDef(MacroDefItem<'db>),
}

/// A macro invocation and its resolution/expansion state.
#[derive(Clone, Debug, PartialEq, Eq, Hash, salsa::Update)]
pub struct MacroUse<'db> {
    pub path: Path<'db>,
    pub state: MacroUseState<'db>,
}

/// State machine for macro invocation resolution.
///
/// Transitions: `Unresolved` → `Expanded` (success) or `Ambiguous`/`Error` (failure).
/// `Unexpanded` is reserved for future use (macros that declare output names without expansion).
#[derive(Clone, Debug, PartialEq, Eq, Hash, salsa::Update)]
pub enum MacroUseState<'db> {
    /// Not yet resolved — path hasn't been looked up.
    Unresolved,
    /// Resolved but not yet expanded (reserved for future use).
    Unexpanded(MacroDefItem<'db>),
    /// Successfully expanded — contains the resulting entries.
    Expanded(Vec<MemmapEntry<'db>>),
    /// Multiple macro candidates found via globs.
    Ambiguous,
    /// Resolution failed (depth limit exceeded or unresolvable).
    Error,
}

/// A glob import stem: records which module a `use foo::*` imports from.
#[derive(Clone, Debug, PartialEq, Eq, Hash, salsa::Update)]
pub struct GlobStem<'db> {
    pub source_module: Module<'db>,
}
