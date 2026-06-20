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

use crate::local_syms::LocalModItemSym;
use crate::local_syms::consts::LocalConstSym;
use crate::local_syms::enums::LocalEnumSym;
use crate::local_syms::fns::LocalFnSym;
use crate::local_syms::impls::LocalImplSym;
use crate::local_syms::macro_defs::LocalMacroDefSym;
use crate::local_syms::macro_invocations::LocalMacroInvocationSym;
use crate::local_syms::mods::LocalModSym;
use crate::local_syms::statics::LocalStaticSym;
use crate::local_syms::structs::LocalStructSym;
use crate::local_syms::traits::LocalTraitSym;
use crate::local_syms::type_aliases::LocalTypeAliasSym;
use crate::local_syms::uses::LocalUseSym;
use crate::name::Name;
use crate::span::AbsoluteSpan;
use crate::symbol::{CrateNum, DefIndex, SymExt};

/// Type alias for the root MEM-map handle.
pub type Memmap<'db> = Stashed<Slice<MemmapEntry<'db>>>;

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
    Function(Name<'db>, LocalFnSym<'db>),
    Struct(Name<'db>, LocalStructSym<'db>),
    Enum(Name<'db>, LocalEnumSym<'db>),
    Trait(Name<'db>, LocalTraitSym<'db>),
    Impl(Name<'db>, LocalImplSym<'db>),
    TypeAlias(Name<'db>, LocalTypeAliasSym<'db>),
    Const(Name<'db>, LocalConstSym<'db>),
    Static(Name<'db>, LocalStaticSym<'db>),
    Mod(Name<'db>, LocalModSym<'db>),
    MacroDef(Name<'db>, LocalMacroDefSym<'db>),
    Imports(LocalUseSym<'db>),
    MacroInvocation(MacroInvocation<'db>),
}

/// A macro invocation at item position.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, AllocStashData)]
pub struct MacroInvocation<'db> {
    /// The invocation itself
    pub invocation: LocalMacroInvocationSym<'db>,

    /// Expansions discovered so far. Starts empty; grows as the fixpoint
    /// loop resolves callees and expands them.
    pub expansions: Slice<Expansion<'db>>,
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
    Rules(LocalMacroDefSym<'db>),

    /// External `macro_rules!` definition.
    ExtRules(SymExt<'db>),

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
