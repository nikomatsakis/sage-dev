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

use sage_stash::{AllocStashData, Slice, Stash, StashDirect, Stashed};

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

impl StashDirect for MacroInput<'_> {}
impl StashDirect for BuiltinMacroKind {}

/// A single entry in the MEM-map.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, AllocStashData)]
pub enum MemmapEntry<'db> {
    /// A declared item — struct, fn, impl, mod, macro_rules!, etc.
    Item(ItemAst<'db>),

    /// Implicit constructor for a tuple struct or unit struct.
    TupleStructCtor(StructAst<'db>),

    /// A `macro_rules!` definition.
    MacroDef(MacroDefAst<'db>),

    /// A `use foo::bar [as baz]` import.
    Redirect {
        name: Name<'db>,
        target: Slice<Name<'db>>,
    },

    /// A `use foo::*` glob import.
    Glob { path: Slice<Name<'db>> },

    /// A macro invocation with its resolution/expansion state.
    MacroUse(MacroUse<'db>),
}

/// A macro invocation at item position.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, AllocStashData)]
pub struct MacroUse<'db> {
    /// The invocation's path segments (e.g. `[foo, bar, m]`).
    pub path: Slice<Name<'db>>,

    /// The tracked input tokens — stable salsa identity from the parse site.
    pub input: MacroInput<'db>,

    /// Expansions discovered so far. Starts empty; grows as the fixpoint
    /// loop resolves callees and expands them.
    pub expansions: Slice<Expansion<'db>>,
}

/// Resolution state for a `MacroUse` — used only by the validator to
/// distinguish "never resolved" from "resolved but produced no entries".
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum MacroUseState<'db> {
    Unresolved,
    Resolved(Vec<MacroCallee<'db>>),
    Expanded(Vec<Expansion<'db>>),
}

impl<'db> MacroUse<'db> {
    /// Compute the state for validation purposes.
    pub fn state(&self, stash: &Stash) -> MacroUseState<'db> {
        let expansions = &stash[self.expansions];
        if expansions.is_empty() {
            MacroUseState::Unresolved
        } else {
            MacroUseState::Expanded(expansions.to_vec())
        }
    }
}

/// One branch of an expanded `MacroUse`.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, AllocStashData)]
pub struct Expansion<'db> {
    /// The callee that produced this branch.
    pub callee: MacroCallee<'db>,
    /// The entries produced by expanding `callee` against the enclosing
    /// `MacroUse`'s `input_tokens`.
    pub entries: Slice<MemmapEntry<'db>>,
}

/// Anything that can appear as the "target" of a macro invocation.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, AllocStashData, salsa::Update)]
pub enum MacroCallee<'db> {
    /// Local `macro_rules!` definition.
    Rules(MacroDefAst<'db>),

    /// Builtin macro.
    Builtin(BuiltinMacroKind),

    /// External proc-macro.
    Proc {
        crate_num: CrateNum,
        def_index: DefIndex,
    },
}

/// Compile-time-known builtin macros.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, salsa::Update)]
pub enum BuiltinMacroKind {
    // -- Derives --
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

/// Type alias for the root MEM-map handle.
pub type Memmap<'db> = Stashed<Slice<MemmapEntry<'db>>>;
