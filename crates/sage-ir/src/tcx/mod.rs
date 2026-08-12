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

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub struct RawDefId {
    pub crate_num: CrateNum,
    pub def_index: DefIndex,
    pub kind: SymExtKind,
}

/// A conservative rigid outer shape used to refine a trait-keyed impl query.
/// `None` means that no lossless refinement is available.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub enum RawSelfTypeHead {
    Adt(RawDefId),
    Bool,
    Char,
    Int,
    Uint,
    Float,
    Str,
    Ref,
    Tuple,
    Slice,
    Array,
    FnPtr,
    Never,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RawRelevantImpls {
    pub impls: Vec<RawDefId>,
    /// Whether every explicit upstream impl relevant to this trait/head is
    /// represented. Individual headers retain their own completeness.
    pub complete: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RawImplSignature {
    pub generics: Vec<RawGenericParam>,
    pub trait_ref: RawTraitPredicate,
    pub predicates: Vec<RawTraitPredicate>,
    pub complete: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RawAssociatedTypeValue {
    pub value: RawTy,
    pub complete: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RawProjectionTy {
    pub associated_ty: RawDefId,
    pub self_ty: Box<RawTy>,
    pub trait_def: RawDefId,
    pub trait_args: Vec<RawTy>,
    pub args: Vec<RawTy>,
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
    Associated(RawProjectionTy),
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
    MetaSized,
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

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RawInherentMethodCandidate {
    pub function: RawDefId,
    pub impl_def: RawDefId,
    /// External inherent methods are callable from Sage's local crate only
    /// when rustc reports public visibility. Non-public same-name items remain
    /// candidates so lookup cannot incorrectly fall through to a trait method.
    pub externally_visible: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RawInherentMethodCandidates {
    pub candidates: Vec<RawInherentMethodCandidate>,
    /// Whether every receiver-bearing inherent method with this name on the
    /// rigid external ADT is represented. Associated functions without a
    /// `self` parameter are deliberately outside this dot-call boundary.
    pub complete: bool,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum RawReceiver {
    Value,
    Ref(crate::cst::Mutability),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RawFnSignature {
    /// Generics introduced by the owning trait or impl.
    pub owner_generics: Vec<RawGenericParam>,
    /// Generics introduced by the function itself.
    pub method_generics: Vec<RawGenericParam>,
    /// The owning trait or inherent impl self type. This is absent for free
    /// functions and associated functions outside a represented owner.
    pub owner_self_ty: Option<RawTy>,
    /// The owning trait fact, including its instantiated `Self` type. This is
    /// absent for free functions and inherent methods.
    pub owner_trait: Option<RawTraitPredicate>,
    pub receiver: Option<RawReceiver>,
    pub params: Vec<RawTy>,
    pub ret: RawTy,
    pub predicates: Vec<RawTraitPredicate>,
    /// False when the ordinary non-const call contract contains a signature
    /// form outside the represented subset. Incomplete signatures are never
    /// method candidates.
    pub ordinary_complete: bool,
    /// Whether const-only call conditions are fully represented. This is
    /// tracked separately and does not make an ordinary call ineligible.
    pub const_call_complete: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RawAdtSignature {
    pub generics: Vec<RawGenericParam>,
    /// One entry per generic parameter. Lifetime and const parameters have no
    /// type default and therefore use `Absent`.
    pub defaults: Vec<RawGenericDefault>,
    pub predicates: Vec<RawTraitPredicate>,
    /// Whether ordinary non-const type formation is fully represented.
    pub ordinary_complete: bool,
    /// Whether deferred const and higher-ranked parts of the declaration are
    /// also fully represented.
    pub deferred_complete: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RawGenericDefault {
    Absent,
    Type(RawTy),
    /// The declaration has a default, but it is outside the owned metadata
    /// model and therefore cannot be treated as an omitted required argument.
    Unsupported,
}

/// External crate metadata interface.
///
/// Returns only owned, `'static` data. The caller is responsible for
/// interning into salsa types (`Name`, `Symbol`). This keeps the trait
/// free of salsa lifetimes, enabling channel-based implementations.
// ANCHOR: architecture_external_metadata_interface
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
    // ANCHOR_END: architecture_external_metadata_interface

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

    /// Generic parameters, trailing type defaults, and ordinary predicates
    /// for an external nominal type. This intentionally excludes fields,
    /// variants, inherent items, and impls.
    fn adt_signature(&self, _crate_num: CrateNum, _def_index: DefIndex) -> Option<RawAdtSignature> {
        None
    }

    /// Return receiver-bearing inherent methods on one rigid external ADT with
    /// exactly the requested source name. Associated functions without a
    /// `self` parameter are excluded. This name-keyed boundary is used both to
    /// audit trait-method shadowing and to select inherent methods.
    fn inherent_method_candidates(
        &self,
        _crate_num: CrateNum,
        _def_index: DefIndex,
        _method_name: &str,
    ) -> Option<RawInherentMethodCandidates> {
        None
    }

    /// Deterministic explicit upstream impl identities for one fixed trait.
    /// A supplied self head may remove only provably disjoint rigid headers;
    /// blanket and unclassifiable headers remain in the result.
    fn relevant_trait_impls(
        &self,
        _crate_num: CrateNum,
        _def_index: DefIndex,
        _self_head: Option<RawSelfTypeHead>,
    ) -> Option<RawRelevantImpls> {
        None
    }

    /// Binder-aware header for one external trait impl. Associated values and
    /// impl items are intentionally separate metadata operations.
    fn impl_signature(
        &self,
        _crate_num: CrateNum,
        _def_index: DefIndex,
    ) -> Option<RawImplSignature> {
        None
    }

    /// Value of one requested associated type in one external impl. This is
    /// intentionally separate from the impl header and item enumeration.
    fn associated_type_value(
        &self,
        _impl_crate_num: CrateNum,
        _impl_def_index: DefIndex,
        _associated_crate_num: CrateNum,
        _associated_def_index: DefIndex,
    ) -> Option<RawAssociatedTypeValue> {
        None
    }

    /// Whether every instantiation of this external ADT is sized. `Some(false)`
    /// is not necessarily a proof of unsizedness for a particular instantiation.
    fn adt_is_always_sized(&self, _crate_num: CrateNum, _def_index: DefIndex) -> Option<bool> {
        None
    }

    /// Whether this external nominal type is fundamental for Rust's orphan
    /// rules. Kept separate from its signature so orphan pruning does not
    /// depend on defaults, predicates, or completeness.
    fn adt_is_fundamental(&self, _crate_num: CrateNum, _def_index: DefIndex) -> Option<bool> {
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
