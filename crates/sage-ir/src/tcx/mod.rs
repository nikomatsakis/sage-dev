mod noop;
mod proxy;

pub use noop::NoopTcxDb;
pub use proxy::{ProxyTcxDb, TcxRequest};

use crate::resolve::Namespace;
use crate::symbol::{CrateNum, DefIndex, SymExtKind};
use crate::ty::{FloatTy, IntTy, UintTy};

/// A single child of an external module — raw owned data, no salsa interning.
#[derive(Clone, Debug)]
pub struct RawChild {
    pub name: String,
    pub crate_num: CrateNum,
    pub def_index: DefIndex,
    pub namespace: Namespace,
    pub kind: SymExtKind,
}

/// Structured external def path for oracle-compatible normalization.
#[derive(Clone, Debug)]
pub struct ExternalDefPath {
    pub krate: String,
    pub segments: Vec<ExternalDefPathSegment>,
}

#[derive(Clone, Debug)]
pub struct ExternalDefPathSegment {
    pub name: String,
    /// The actual definition kind of this segment, not merely its namespace.
    pub kind: SymExtKind,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum RawGenericParamKind {
    Type,
    Lifetime,
    Const,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RawGenericParam {
    pub index: u32,
    pub name: Option<String>,
    pub kind: RawGenericParamKind,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RawDefId {
    pub crate_num: CrateNum,
    pub def_index: DefIndex,
    pub kind: SymExtKind,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RawTy {
    Bool,
    Char,
    Int(IntTy),
    Uint(UintTy),
    Float(FloatTy),
    Str,
    Adt(RawDefId, Vec<RawTy>),
    Ref(Box<RawTy>, crate::cst::Mutability),
    Tuple(Vec<RawTy>),
    Slice(Box<RawTy>),
    Array(Box<RawTy>, u64),
    Param(u32),
    Never,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RawTraitPredicate {
    pub self_ty: RawTy,
    pub trait_def: RawDefId,
    /// Type arguments other than `Self`; lifetime arguments are erased to
    /// `Lifetime::Dummy` and are not carried by Sage's type-only trait ref.
    pub args: Vec<RawTy>,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum RawTraitSemantics {
    Ordinary,
    Sized,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RawTraitSignature {
    pub generics: Vec<RawGenericParam>,
    pub self_param_index: u32,
    pub predicates: Vec<RawTraitPredicate>,
    pub semantics: RawTraitSemantics,
    /// False when metadata contains a predicate or generic form outside the
    /// represented subset. Incomplete signatures are never solver candidates.
    pub complete: bool,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum RawAssociatedItemKind {
    Function,
    Type,
    Const,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RawAssociatedItem {
    pub def: RawDefId,
    pub name: String,
    pub kind: RawAssociatedItemKind,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RawAssociatedItems {
    pub items: Vec<RawAssociatedItem>,
    /// False when a user-nameable associated item may be absent from the
    /// represented set. Anonymous compiler-generated RPITIT projection items
    /// do not affect name-discovery completeness.
    pub complete: bool,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum RawReceiver {
    Value,
    Ref(crate::cst::Mutability),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RawFnSignature {
    pub generics: Vec<RawGenericParam>,
    /// The owning trait fact, including its instantiated `Self` type. This is
    /// absent for free functions and inherent methods.
    pub owner_trait: Option<RawTraitPredicate>,
    pub receiver: Option<RawReceiver>,
    pub params: Vec<RawTy>,
    pub ret: RawTy,
    pub predicates: Vec<RawTraitPredicate>,
    /// False when metadata contains a signature form outside the represented
    /// subset. Incomplete signatures are never method candidates.
    pub complete: bool,
}

/// External crate metadata interface.
///
/// Returns only owned, `'static` data. The caller is responsible for
/// interning into salsa types (`Name`, `Symbol`). This keeps the trait
/// free of salsa lifetimes, enabling channel-based implementations.
pub trait TcxDb: Send + Sync {
    fn extern_crate(&self, name: &str) -> Option<CrateNum>;

    fn module_children(&self, crate_num: CrateNum, def_index: DefIndex) -> Vec<RawChild>;

    /// Return the name of the item with the given id or None if it is something anonymous (e.g., an impl).
    fn item_name(&self, crate_num: CrateNum, def_index: DefIndex) -> Option<String>;

    /// True iff the given external definition is a module (crate
    /// root, `mod foo`, etc.). Modules are the only DefIds on which
    /// `module_children` is valid to call — asking on a struct or
    /// function makes rustc's `module_children` query panic.
    ///
    /// Callers that convert a `Symbol::External(cn, di)` into a
    /// `ModSymbol::External(cn, di)` must gate the conversion on this
    /// check.
    fn is_module(&self, crate_num: CrateNum, def_index: DefIndex) -> bool;

    fn is_builtin_derive(&self, crate_num: CrateNum, def_index: DefIndex) -> bool;

    /// Human-readable path for an external definition, e.g. `"core::option::Option::Some"`.
    fn def_path(&self, crate_num: CrateNum, def_index: DefIndex) -> Option<String>;

    /// Structured def path with crate name and per-segment namespace info.
    fn structured_def_path(
        &self,
        crate_num: CrateNum,
        def_index: DefIndex,
    ) -> Option<ExternalDefPath>;

    /// Checked signature data for an external trait, expressed without rustc
    /// or Salsa lifetimes. Missing/incomplete data cannot justify a proof.
    fn trait_signature(
        &self,
        _crate_num: CrateNum,
        _def_index: DefIndex,
    ) -> Option<RawTraitSignature> {
        None
    }

    /// Return the associated items of an external trait without loading their
    /// signatures. Keeping enumeration separate preserves narrow incremental
    /// dependencies for name-based method lookup.
    fn associated_items(
        &self,
        _crate_num: CrateNum,
        _def_index: DefIndex,
    ) -> Option<RawAssociatedItems> {
        None
    }

    /// Checked signature data for an external function or method.
    fn fn_signature(&self, _crate_num: CrateNum, _def_index: DefIndex) -> Option<RawFnSignature> {
        None
    }

    /// Whether every instantiation of this external ADT is sized. `Some(false)`
    /// is not necessarily a proof of unsizedness for a particular instantiation.
    fn adt_is_always_sized(&self, _crate_num: CrateNum, _def_index: DefIndex) -> Option<bool> {
        None
    }

    /// Expand a proc-macro derive. Returns the expanded source text.
    fn expand_proc_macro_derive(
        &self,
        crate_num: CrateNum,
        def_index: DefIndex,
        item_source: &str,
    ) -> Option<String>;

    /// Expand a proc-macro bang macro (`foo!(tokens)`). Returns the expanded source text.
    fn expand_proc_macro_bang(
        &self,
        crate_num: CrateNum,
        def_index: DefIndex,
        input_tokens: &str,
    ) -> Option<String>;

    /// Expand an attribute proc-macro (`#[attr] item`). Returns the transformed item source.
    fn expand_proc_macro_attr(
        &self,
        crate_num: CrateNum,
        def_index: DefIndex,
        attr_args: &str,
        item_source: &str,
    ) -> Option<String>;
}
