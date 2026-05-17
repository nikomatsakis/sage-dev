//! Resolved type representation.
//!
//! All types are stash-allocated (`Copy`, `AllocStashData`). They live in the
//! same stash as the signature or body they belong to. No global interning.

use sage_stash::{AllocStashData, Ptr, Slice};

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
    Adt(Symbol<'db>, Slice<Ty<'db>>),
    Ref(Ptr<Ty<'db>>, Mutability, Lifetime),
    Tuple(Slice<Ty<'db>>),
    Slice(Ptr<Ty<'db>>),
    Array(Ptr<Ty<'db>>, Const<'db>),
    FnPtr(Slice<Ty<'db>>, Ptr<Ty<'db>>),
    BoundVar(BoundVar),
    Never,
    Error,
}

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
pub enum Lifetime {
    BoundVar(BoundVar),
    Static,
    Erased,
}

// ---------------------------------------------------------------------------
// Const
// ---------------------------------------------------------------------------

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, AllocStashData)]
pub enum Const<'db> {
    Literal(u64),
    Other(Symbol<'db>),
}

// ---------------------------------------------------------------------------
// BoundVar
// ---------------------------------------------------------------------------

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, AllocStashData)]
pub struct BoundVar {
    pub binder_index: u32,
    pub param_index: u32,
}

// ---------------------------------------------------------------------------
// Binder<T>
// ---------------------------------------------------------------------------

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
#[repr(C)]
pub struct Binder<'db, T> {
    pub value: T,
    pub bound_vars: Slice<BoundVarInfo>,
    _marker: std::marker::PhantomData<&'db ()>,
}

unsafe impl<'db, T: Copy + PartialEq + std::hash::Hash + sage_stash::StashHash + 'static>
    sage_stash::StashData<'db> for Binder<'db, T>
{
    fn static_type_id() -> std::any::TypeId {
        std::any::TypeId::of::<Binder<'static, T>>()
    }
}

impl<'db, T: Copy + PartialEq + std::hash::Hash + sage_stash::StashHash + 'static>
    AllocStashData<'db> for Binder<'db, T>
{
}

impl<'db, T: sage_stash::StashHash + std::hash::Hash> sage_stash::StashHash for Binder<'db, T> {
    fn stash_hash(&self, stash: &sage_stash::Stash, hasher: &mut impl sage_stash::StashHasher) {
        self.value.stash_hash(stash, hasher);
        sage_stash::StashHash::stash_hash(&self.bound_vars, stash, hasher);
    }
}

impl<'db, T> Binder<'db, T> {
    pub fn new(value: T, bound_vars: Slice<BoundVarInfo>) -> Self {
        Self {
            value,
            bound_vars,
            _marker: std::marker::PhantomData,
        }
    }
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, AllocStashData)]
pub struct BoundVarInfo {
    pub kind: BoundVarKind,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, AllocStashData)]
pub enum BoundVarKind {
    Type,
    Lifetime,
    Const,
}

// ---------------------------------------------------------------------------
// Signature types
// ---------------------------------------------------------------------------

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, AllocStashData)]
pub struct FnSig<'db> {
    pub params: Slice<Ty<'db>>,
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
