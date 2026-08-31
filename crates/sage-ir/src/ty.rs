//! Resolved type representation.
//!
//! All types are stash-allocated (`Copy`, `AllocStashData`). They live in the
//! same stash as the signature or body they belong to. No global interning.

use sage_stash::{AllocStashData, Ptr, Slice, StashHash, Stashed};

use crate::cst::Mutability;
use crate::diagnostic::ErrorReported;
use crate::generic_param::GenericParam;
use crate::name::Name;
use crate::symbol::{ConstSymbol, FnSymbol, Symbol, TraitSymbol, TypeAliasSymbol};

// ---------------------------------------------------------------------------
// Ty
// ---------------------------------------------------------------------------

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, AllocStashData, sage_reflect::Reflect)]
pub enum Ty<'db> {
    // --- primitives ---
    Bool,
    Char,
    Int(IntTy),
    Uint(UintTy),
    Float(FloatTy),
    Str,

    // --- compound ---
    Adt(Symbol<'db>, Slice<Ptr<Ty<'db>>>),
    Alias(AliasTy<'db>),
    Ref(Ptr<Ty<'db>>, Mutability, Lifetime),
    Tuple(Slice<Ptr<Ty<'db>>>),
    Slice(Ptr<Ty<'db>>),
    Array(Ptr<Ty<'db>>, Const<'db>),
    FnPtr(Slice<Ptr<Ty<'db>>>, Ptr<Ty<'db>>),

    // --- variables ---
    /// A reference to a generic type parameter (universal variable).
    /// Invariant: param.kind() == Type.
    Param(GenericParam<'db>),
    /// An existential inference variable — a fresh unknown to be resolved.
    InferVar(InferVarIndex),

    // --- other ---
    Never,
    Error(ErrorReported),
}

// ---------------------------------------------------------------------------
// Alias types
// ---------------------------------------------------------------------------

/// A semantic type alias application. Alias identity is retained until a
/// caller explicitly requests normalization.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, AllocStashData, sage_reflect::Reflect)]
pub enum AliasTy<'db> {
    Named(NamedAliasTy<'db>),
    Associated(ProjectionTy<'db>),
    Opaque(OpaqueAliasTy<'db>),
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, AllocStashData, sage_reflect::Reflect)]
pub struct NamedAliasTy<'db> {
    pub def: TypeAliasSymbol<'db>,
    pub args: Slice<Ptr<Ty<'db>>>,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, AllocStashData, sage_reflect::Reflect)]
pub struct ProjectionTy<'db> {
    pub associated_ty: TypeAliasSymbol<'db>,
    pub self_ty: Ptr<Ty<'db>>,
    pub trait_ref: TraitRef<'db>,
    pub args: Slice<Ptr<Ty<'db>>>,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, AllocStashData, sage_reflect::Reflect)]
pub struct OpaqueAliasTy<'db> {
    pub def: TypeAliasSymbol<'db>,
    pub args: Slice<Ptr<Ty<'db>>>,
}

/// Sequential counter for inference variables. Dense, monotonically increasing.
/// Indexes into the per-version variable metadata table.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, sage_reflect::Reflect)]
pub struct InferVarIndex(pub u32);

impl sage_stash::StashDirect for InferVarIndex {}

// ---------------------------------------------------------------------------
// Primitive details
// ---------------------------------------------------------------------------

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, AllocStashData, sage_reflect::Reflect)]
pub enum IntTy {
    I8,
    I16,
    I32,
    I64,
    I128,
    Isize,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, AllocStashData, sage_reflect::Reflect)]
pub enum UintTy {
    U8,
    U16,
    U32,
    U64,
    U128,
    Usize,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, AllocStashData, sage_reflect::Reflect)]
pub enum FloatTy {
    F32,
    F64,
}

// ---------------------------------------------------------------------------
// Lifetime
// ---------------------------------------------------------------------------

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, AllocStashData, sage_reflect::Reflect)]
pub enum Lifetime {
    /// Lifetime semantics are intentionally deferred until Sage introduces
    /// unified type-and-lifetime inference and borrow checking.
    Dummy,
}

// ---------------------------------------------------------------------------
// Const
// ---------------------------------------------------------------------------

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, AllocStashData, sage_reflect::Reflect)]
pub enum Const<'db> {
    Literal(u64),
    /// A reference to a generic const parameter.
    /// Invariant: param.kind() == Const.
    Param(GenericParam<'db>),
    Other(Symbol<'db>),
}

// ---------------------------------------------------------------------------
// Binder<T>
// ---------------------------------------------------------------------------

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, sage_reflect::Reflect)]
pub struct Binder<'db, T> {
    pub value: T,
    pub generics: Slice<GenericParam<'db>>,
}

// Safety: `T: StashData` guarantees the required copyability, lifetime erasure,
// and unique `StaticSelf`; wrapping both `T` and the stash-owned generic slice
// in `Binder` preserves those invariants without adding references of its own.
unsafe impl<'db, T: sage_stash::StashData<'db>> sage_stash::StashData<'db> for Binder<'db, T> {
    type StaticSelf = Binder<'static, T::StaticSelf>;
}

impl<'db, T: sage_stash::StashData<'db> + sage_stash::StashHash + PartialEq> AllocStashData<'db>
    for Binder<'db, T>
{
}

impl<'db, T: sage_stash::StashHash> sage_stash::StashHash for Binder<'db, T> {
    fn stash_hash(&self, stash: &sage_stash::Stash, hasher: &mut impl sage_stash::StashHasher) {
        self.value.stash_hash(stash, hasher);
        sage_stash::StashHash::stash_hash(&self.generics, stash, hasher);
    }
}

impl<'db, T: sage_stash::StashCopy> sage_stash::StashCopy for Binder<'db, T> {
    fn stash_copy(&self, source: &sage_stash::Stash, target: &mut sage_stash::Stash) -> Self {
        Binder {
            value: self.value.stash_copy(source, target),
            generics: self.generics.stash_copy(source, target),
        }
    }
}

impl<'db, T> Binder<'db, T> {
    pub fn new(value: T, generics: Slice<GenericParam<'db>>) -> Self {
        Self { value, generics }
    }
}

pub trait BinderExt<'db> {
    fn iter_symbols(&self) -> impl Iterator<Item = GenericParam<'db>>;
}

impl<'db, T> BinderExt<'db> for Stashed<Binder<'db, T>>
where
    T: StashHash + Copy,
{
    fn iter_symbols(&self) -> impl Iterator<Item = GenericParam<'db>> {
        let stash = self.stash();
        let generics = self.root().generics;
        stash[generics].iter().copied()
    }
}

// ---------------------------------------------------------------------------
// Signature types
// ---------------------------------------------------------------------------

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, AllocStashData, sage_reflect::Reflect)]
pub struct FnSig<'db> {
    /// Number of leading entries in the enclosing binder which belong to the
    /// owning trait or impl; remaining entries are function-level generics.
    pub owner_generic_count: u32,
    pub owner_self_ty: Option<Ptr<Ty<'db>>>,
    pub receiver: Option<CheckedReceiver<'db>>,
    pub params: Slice<Ptr<Ty<'db>>>,
    pub ret: Ptr<Ty<'db>>,
    pub parameter_env: CheckedParameterEnv<'db>,
    pub method_candidate_eligibility: SolverEligibility,
    /// Whether const-only call conditions are fully represented. Ordinary
    /// body checking does not require this bit to be true.
    pub const_call_complete: bool,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, AllocStashData, sage_reflect::Reflect)]
pub struct StructSig<'db> {
    pub parameter_env: CheckedParameterEnv<'db>,
}

/// The declaration data needed to form an external nominal type. Defaults are
/// aligned with `Binder::generics`; only type parameters can contain a value.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, AllocStashData, sage_reflect::Reflect)]
pub struct ExternalAdtSignatureData<'db> {
    pub defaults: Slice<GenericDefault<'db>>,
    pub parameter_env: CheckedParameterEnv<'db>,
    pub ordinary_complete: bool,
    pub deferred_complete: bool,
}

pub type ExternalAdtSignature<'db> = Binder<'db, ExternalAdtSignatureData<'db>>;

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, AllocStashData, sage_reflect::Reflect)]
pub enum GenericDefault<'db> {
    Absent,
    Type(Ptr<Ty<'db>>),
    Unsupported,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, AllocStashData, sage_reflect::Reflect)]
pub struct StructFields<'db> {
    pub fields: Slice<FieldSig<'db>>,
    /// Well-formedness predicates introduced by the declared field types.
    pub parameter_env: CheckedParameterEnv<'db>,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, AllocStashData, sage_reflect::Reflect)]
pub struct FieldSig<'db> {
    pub name: Name<'db>,
    pub ty: Ptr<Ty<'db>>,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, AllocStashData, sage_reflect::Reflect)]
pub struct EnumSig<'db> {
    pub variants: Slice<VariantSig<'db>>,
    pub parameter_env: CheckedParameterEnv<'db>,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, AllocStashData, sage_reflect::Reflect)]
pub struct VariantSig<'db> {
    pub name: Name<'db>,
    pub fields: Slice<FieldSig<'db>>,
}

// ---------------------------------------------------------------------------
// Trait-system signatures
// ---------------------------------------------------------------------------

/// `Trait<A, B>` in a bound or impl header. `Self` is kept separately.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, AllocStashData, sage_reflect::Reflect)]
pub struct TraitRef<'db> {
    pub trait_sym: TraitSymbol<'db>,
    pub args: Slice<Ptr<Ty<'db>>>,
}

/// A positive type predicate, `self_ty: trait_ref`.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, AllocStashData, sage_reflect::Reflect)]
pub struct WherePredicate<'db> {
    pub self_ty: Ptr<Ty<'db>>,
    pub trait_ref: TraitRef<'db>,
}

/// Whether all applicability data can be consumed by the type-only MVP.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, AllocStashData, sage_reflect::Reflect)]
pub enum SolverEligibility {
    Eligible,
    Unsupported,
}

impl SolverEligibility {
    pub fn and(self, other: Self) -> Self {
        if self == Self::Eligible && other == Self::Eligible {
            Self::Eligible
        } else {
            Self::Unsupported
        }
    }

    pub fn is_eligible(self) -> bool {
        self == Self::Eligible
    }
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, AllocStashData, sage_reflect::Reflect)]
pub struct CheckedParameterEnv<'db> {
    pub where_clauses: Slice<WherePredicate<'db>>,
    pub solver_eligibility: SolverEligibility,
}

pub type TraitSignature<'db> = Binder<'db, TraitSignatureData<'db>>;

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, AllocStashData, sage_reflect::Reflect)]
pub struct TraitSignatureData<'db> {
    pub self_param: GenericParam<'db>,
    pub where_clauses: Slice<WherePredicate<'db>>,
    pub solver_eligibility: SolverEligibility,
    pub semantics: TraitSemantics,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, AllocStashData, sage_reflect::Reflect)]
pub enum TraitSemantics {
    Ordinary,
    Sized,
    MetaSized,
}

pub type ImplSignature<'db> = Binder<'db, ImplSignatureData<'db>>;

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, AllocStashData, sage_reflect::Reflect)]
pub struct ImplSignatureData<'db> {
    pub trait_ref: Option<TraitRef<'db>>,
    pub self_ty: Ptr<Ty<'db>>,
    pub where_clauses: Slice<WherePredicate<'db>>,
    pub solver_eligibility: SolverEligibility,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, AllocStashData, sage_reflect::Reflect)]
pub enum TraitItemDef<'db> {
    Function(FnSymbol<'db>),
    Type(TypeAliasSymbol<'db>),
    Const(ConstSymbol<'db>),
}

pub type TraitItems<'db> = Binder<'db, Slice<TraitItemDef<'db>>>;
pub type ImplItems<'db> = Binder<'db, Slice<TraitItemDef<'db>>>;

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, AllocStashData, sage_reflect::Reflect)]
pub enum MethodReceiver {
    Value { mutable_binding: bool },
    Ref { mutability: Mutability },
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, AllocStashData, sage_reflect::Reflect)]
pub struct CheckedReceiver<'db> {
    pub owner_self_ty: Ptr<Ty<'db>>,
    pub form: MethodReceiver,
}
