//! Resolved type representation.
//!
//! All types are stash-allocated (`Copy`, `AllocStashData`). They live in the
//! same stash as the signature or body they belong to. No global interning.

use sage_stash::{AllocStashData, Ptr, Slice};

use crate::generic_param::GenericParam;
use crate::name::Name;
use crate::symbol::Symbol;
use crate::types::Mutability;

// ---------------------------------------------------------------------------
// Ty
// ---------------------------------------------------------------------------

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, AllocStashData)]
pub struct Ty<'db> {
    pub data: TyData<'db>,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, AllocStashData)]
pub enum TyData<'db> {
    // --- primitives ---
    Bool,
    Char,
    Int(IntTy),
    Uint(UintTy),
    Float(FloatTy),
    Str,

    // --- compound ---
    Adt(Symbol<'db>, Slice<Ptr<Ty<'db>>>),
    Ref(Ptr<Ty<'db>>, Mutability, Lifetime<'db>),
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
    Error,
}

/// Sequential counter for inference variables. Dense, monotonically increasing.
/// Indexes into the per-version variable metadata table.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct InferVarIndex(pub u32);

impl sage_stash::StashDirect for InferVarIndex {}

// ---------------------------------------------------------------------------
// Primitive details
// ---------------------------------------------------------------------------

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, AllocStashData)]
pub enum IntTy {
    I8,
    I16,
    I32,
    I64,
    I128,
    Isize,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, AllocStashData)]
pub enum UintTy {
    U8,
    U16,
    U32,
    U64,
    U128,
    Usize,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, AllocStashData)]
pub enum FloatTy {
    F32,
    F64,
}

// ---------------------------------------------------------------------------
// Lifetime
// ---------------------------------------------------------------------------

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, AllocStashData)]
pub enum Lifetime<'db> {
    /// Invariant: param.kind() == Lifetime.
    Param(GenericParam<'db>),
    Static,
    Erased,
}

// ---------------------------------------------------------------------------
// Const
// ---------------------------------------------------------------------------

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, AllocStashData)]
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

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub struct Binder<'db, T> {
    pub value: T,
    pub generics: Slice<GenericParam<'db>>,
}

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

// ---------------------------------------------------------------------------
// Signature types
// ---------------------------------------------------------------------------

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, AllocStashData)]
pub struct FnSig<'db> {
    pub params: Slice<Ptr<Ty<'db>>>,
    pub ret: Ptr<Ty<'db>>,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, AllocStashData)]
pub struct StructSig<'db> {
    pub fields: Slice<FieldSig<'db>>,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, AllocStashData)]
pub struct FieldSig<'db> {
    pub name: Name<'db>,
    pub ty: Ptr<Ty<'db>>,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, AllocStashData)]
pub struct EnumSig<'db> {
    pub variants: Slice<VariantSig<'db>>,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, AllocStashData)]
pub struct VariantSig<'db> {
    pub name: Name<'db>,
    pub fields: Slice<FieldSig<'db>>,
}
