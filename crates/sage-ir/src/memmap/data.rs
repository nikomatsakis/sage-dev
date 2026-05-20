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
//! - `Item(StructAst("Foo"))` — namespace is derived from the item at
//!   lookup time (struct → Type and Value)
//! - `Redirect { name: "Baz", target: bar::Baz }` — namespace resolved
//!   dynamically by resolving the target
//! - `MacroDef(m)` — always `Namespace::Macro(Bang)`
//! - `MacroUse { path: "m", input_tokens: "", state: Expanded([
//!       Expansion { callee: Rules(m), entries: [Item(StructAst("X"))] }
//!   ]) }`
//!
//! Namespace information is never stored on entries — it's always
//! derivable from the variant shape (Item → `item_in_namespace`, MacroDef
//! → `Macro(Bang)`, Redirect → resolve target).

use crate::item::{ItemAst, MacroDefAst, StructAst};
use crate::module::{CrateNum, DefIndex};
use crate::name::Name;
use crate::span::AbsoluteSpan;

/// A macro invocation's input tokens, created during parsing/lowering.
/// Has stable salsa identity from the parse site — never mutated.
#[salsa::tracked(debug)]
pub struct MacroInput<'db> {
    #[returns(ref)]
    pub tokens: String,
    pub span: AbsoluteSpan<'db>,
}

/// A single entry in the MEM-map.
#[derive(Clone, Debug, PartialEq, Eq, Hash, salsa::Update)]
pub enum MemmapEntry<'db> {
    /// A declared item — struct, fn, impl, mod, macro_rules!, etc.
    ///
    /// Name is via `item_name()` and is `None` for anonymous items
    /// (`ItemAst::Impl`, `ItemAst::Use`, `ItemAst::MacroInvocation`, `ItemAst::Error`).
    /// Namespace is via `item_in_namespace()`.
    ///
    /// `ItemAst::MacroDef` is split out into a dedicated `MacroDef` variant
    /// below so callers can filter macro definitions without going through
    /// `Item`. `ItemAst::Use` and `ItemAst::MacroInvocation` are never emitted
    /// here — seeding transforms them into `Redirect`/`Glob`/`MacroUse`
    /// entries.
    Item(ItemAst<'db>),

    /// Implicit constructor for a tuple struct or unit struct.
    /// Lives in `Namespace::Value` only. The `StructAst` provides the
    /// field types from which a callable signature is derived.
    TupleStructCtor(StructAst<'db>),

    /// A `macro_rules!` definition. Name via `def.name()`. Always lives in
    /// `Namespace::Macro(Bang)`.
    MacroDef(MacroDefAst<'db>),

    /// A `use foo::bar [as baz]` import. `name` is the alias (or the last
    /// segment of `target`). Namespace is determined dynamically by
    /// resolving `target` at lookup time, because it depends on what the
    /// target ends up being (type, value, or macro).
    Redirect {
        name: Name<'db>,
        target: Vec<Name<'db>>,
    },

    /// A `use foo::*` glob import. `path` is the source module's path.
    /// Resolved to a module dynamically at lookup time — **not** during
    /// seeding, so globs whose target is created by macro expansion are
    /// picked up correctly.
    Glob { path: Vec<Name<'db>> },

    /// A macro invocation with its resolution/expansion state.
    MacroUse(MacroUse<'db>),
}

/// A macro invocation at item position.
#[derive(Clone, Debug, PartialEq, Eq, Hash, salsa::Update)]
pub struct MacroUse<'db> {
    /// The invocation's path segments (e.g. `[foo, bar, m]`).
    pub path: Vec<Name<'db>>,

    /// The tracked input tokens — stable salsa identity from the parse site.
    pub input: MacroInput<'db>,

    /// Expansions discovered so far. Starts empty; grows as the fixpoint
    /// loop resolves callees and expands them. Each expansion pairs a
    /// callee with the entries it produced.
    pub expansions: Vec<Expansion<'db>>,
}

/// Resolution state for a `MacroUse` — used only by the validator to
/// distinguish "never resolved" from "resolved but produced no entries".
///
/// This is a computed property derived from `MacroUse.expansions`:
/// - `expansions.is_empty()` → Unresolved
/// - `expansions.len() >= 1` → Expanded
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

impl<'db> MacroUse<'db> {
    /// Compute the state for validation purposes.
    pub fn state(&self) -> MacroUseState<'db> {
        if self.expansions.is_empty() {
            MacroUseState::Unresolved
        } else {
            MacroUseState::Expanded(self.expansions.clone())
        }
    }
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
/// Broader than `MacroDefAst` because derives and proc-macros aren't
/// `macro_rules!` definitions.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, salsa::Update)]
pub enum MacroCallee<'db> {
    /// Local `macro_rules!` definition.
    Rules(MacroDefAst<'db>),

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
