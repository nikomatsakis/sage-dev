//! Data model for the MEM-map.
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
//! - `Item(StructItem("Foo"))` — namespace is derived from the item at
//!   lookup time (struct → Type and Value)
//! - `Redirect { name: "Baz", target: bar::Baz }` — namespace resolved
//!   dynamically by resolving the target
//! - `MacroDef(m)` — always `Namespace::Macro(Bang)`
//! - `MacroUse { path: "m", input_tokens: "", state: Expanded([
//!       Expansion { callee: Rules(m), entries: [Item(StructItem("X"))] }
//!   ]) }`
//!
//! Namespace information is never stored on entries — it's always
//! derivable from the variant shape (Item → `item_in_namespace`, MacroDef
//! → `Macro(Bang)`, Redirect → resolve target).

use crate::item::{Item, MacroDefItem};
use crate::module::{CrateNum, DefIndex};
use crate::name::Name;
use crate::types::Path;

/// A single entry in the MEM-map.
#[derive(Clone, Debug, PartialEq, Eq, Hash, salsa::Update)]
pub enum MemmapEntry<'db> {
    /// A declared item — struct, fn, impl, mod, macro_rules!, etc.
    ///
    /// Name is via `item_name()` and is `None` for anonymous items
    /// (`Item::Impl`, `Item::Use`, `Item::MacroInvocation`, `Item::Error`).
    /// Namespace is via `item_in_namespace()`.
    ///
    /// `Item::MacroDef` is split out into a dedicated `MacroDef` variant
    /// below so callers can filter macro definitions without going through
    /// `Item`. `Item::Use` and `Item::MacroInvocation` are never emitted
    /// here — seeding transforms them into `Redirect`/`Glob`/`MacroUse`
    /// entries.
    Item(Item<'db>),

    /// A `macro_rules!` definition. Name via `def.name()`. Always lives in
    /// `Namespace::Macro(Bang)`.
    MacroDef(MacroDefItem<'db>),

    /// A `use foo::bar [as baz]` import. `name` is the alias (or the last
    /// segment of `target`). Namespace is determined dynamically by
    /// resolving `target` at lookup time, because it depends on what the
    /// target ends up being (type, value, or macro).
    Redirect { name: Name<'db>, target: Path<'db> },

    /// A `use foo::*` glob import. `path` is the source module's path.
    /// Resolved to a module dynamically at lookup time — **not** during
    /// seeding, so globs whose target is created by macro expansion are
    /// picked up correctly.
    Glob { path: Path<'db> },

    /// A macro invocation with its resolution/expansion state.
    MacroUse(MacroUse<'db>),
}

/// A macro invocation at item position.
#[derive(Clone, Debug, PartialEq, Eq, Hash, salsa::Update)]
pub struct MacroUse<'db> {
    /// The invocation's path (e.g. `foo::bar::m`).
    pub path: Path<'db>,

    /// The argument tokens from `m!(...)` (outer delimiters stripped).
    /// Empty for no-arg invocations. Property of the invocation, not of
    /// any candidate def — so it lives here, not on `Expansion`.
    pub input_tokens: String,

    /// Resolution and expansion state.
    pub state: MacroUseState<'db>,
}

/// Resolution state for a `MacroUse`.
///
/// Monotonic progression:
/// ```text
/// Unresolved ──► Resolved(callees)    (callees don't introduce names)
///            └► Expanded(branches)    (one branch per callee)
/// ```
/// Once in `Resolved` or `Expanded`, a MacroUse never regresses.
#[derive(Clone, Debug, PartialEq, Eq, Hash, salsa::Update)]
pub enum MacroUseState<'db> {
    /// Path hasn't resolved yet (still iterating), or converged without
    /// any candidate def. The validator reports `UnresolvedMacro` after
    /// convergence.
    Unresolved,

    /// Resolved to one or more candidate callees, but no MEM-map entries
    /// were produced. Two legitimate reasons:
    ///
    /// 1. The macro's output doesn't contribute names to the enclosing
    ///    module (e.g. `#[derive(Debug)]` generates anonymous impls).
    /// 2. Expansion is deferred to a later phase.
    ///
    /// `len() > 1` is an ambiguous resolution — same E0659 semantics as
    /// the `Expanded` case, reported by the validator.
    Resolved(Vec<MacroCallee<'db>>),

    /// Resolved and expanded. Each `Expansion` pairs the chosen callee
    /// with the entries that expansion produced. `len() > 1` is fan-out
    /// (time-travel / cross-level ambiguity); each branch is
    /// independently usable, and the validator reports `AmbiguousMacro`.
    Expanded(Vec<Expansion<'db>>),
}

/// One branch of an expanded `MacroUse`.
#[derive(Clone, Debug, PartialEq, Eq, Hash, salsa::Update)]
pub struct Expansion<'db> {
    /// The callee that produced this branch.
    pub callee: MacroCallee<'db>,
    /// The entries produced by expanding `callee` against the enclosing
    /// `MacroUse`'s `input_tokens`.
    pub entries: Vec<MemmapEntry<'db>>,
}

/// Anything that can appear as the "target" of a macro invocation.
///
/// Broader than `MacroDefItem` because derives and proc-macros aren't
/// `macro_rules!` definitions.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, salsa::Update)]
pub enum MacroCallee<'db> {
    /// Local `macro_rules!` definition.
    Rules(MacroDefItem<'db>),

    /// Builtin macro, identified by `tcx.classify_builtin_macro`. The
    /// kind directly tells us whether expansion can contribute names
    /// (most derives don't; most bangs do).
    Builtin(BuiltinMacroKind),

    /// External proc-macro (bang, derive, or attribute) that is not a
    /// builtin. Can emit arbitrary items, so expansion must run for
    /// name resolution.
    Proc {
        crate_num: CrateNum,
        def_index: DefIndex,
    },
}

/// Compile-time-known builtin macros. Enumeration mirrors rustc's
/// `#[rustc_builtin_macro]` set.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, salsa::Update)]
pub enum BuiltinMacroKind {
    // -- Derives (produce anonymous impls; never introduce names) --
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    Hash,
    PartialOrd,
    Ord,
    Default,

    // -- Bang macros --
    Println,
    Print,
    Eprintln,
    Eprint,
    Vec,
    Format,
    FormatArgs,
    Dbg,
    Assert,
    DebugAssert,
    Matches,
    Todo,
    Unreachable,
    Unimplemented,
    Stringify,
    Concat,
    Env,
    OptionEnv,
    File,
    Line,
    Column,
    ModulePath,
    Include,
    IncludeStr,
    IncludeBytes,
    CompileError,
    ThreadLocal,
}
