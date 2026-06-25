//! `TyFolder`: cross-stash type mapping.

use rustc_hash::FxHashMap;
use sage_stash::{Ptr, Slice, Stash};

use crate::generic_param::GenericParam;
use crate::ty::*;

// ---------------------------------------------------------------------------
// TyFolder trait
// ---------------------------------------------------------------------------

pub trait TyFolder<'db> {
    fn target(&mut self) -> &mut Stash;
    fn source(&self) -> &Stash;

    fn fold_ty(&mut self, ty: Ty<'db>) -> Ty<'db>
    where
        Self: Sized,
    {
        default_fold_ty(self, ty)
    }
}

pub fn default_fold_ty<'db>(folder: &mut impl TyFolder<'db>, ty: Ty<'db>) -> Ty<'db> {
    match ty {
        Ty::Adt(sym, args) => {
            let args = fold_ptr_slice(folder, args);
            Ty::Adt(sym, args)
        }
        Ty::Ref(inner, m, lt) => {
            let inner_ty = folder.fold_ty(folder.source()[inner]);
            let inner = folder.target().alloc(inner_ty);
            Ty::Ref(inner, m, lt)
        }
        Ty::Tuple(elems) => {
            let elems = fold_ptr_slice(folder, elems);
            Ty::Tuple(elems)
        }
        Ty::Slice(inner) => {
            let inner_ty = folder.fold_ty(folder.source()[inner]);
            let inner = folder.target().alloc(inner_ty);
            Ty::Slice(inner)
        }
        Ty::Array(inner, c) => {
            let inner_ty = folder.fold_ty(folder.source()[inner]);
            let inner = folder.target().alloc(inner_ty);
            Ty::Array(inner, c)
        }
        Ty::FnPtr(params, ret) => {
            let params = fold_ptr_slice(folder, params);
            let ret_ty = folder.fold_ty(folder.source()[ret]);
            let ret = folder.target().alloc(ret_ty);
            Ty::FnPtr(params, ret)
        }
        Ty::Bool => Ty::Bool,
        Ty::Char => Ty::Char,
        Ty::Int(int_ty) => Ty::Int(int_ty),
        Ty::Uint(uint_ty) => Ty::Uint(uint_ty),
        Ty::Float(float_ty) => Ty::Float(float_ty),
        Ty::Str => Ty::Str,
        Ty::Param(generic_param) => Ty::Param(generic_param),
        Ty::InferVar(infer_var_index) => Ty::InferVar(infer_var_index),
        Ty::Never => Ty::Never,
        Ty::Error(e) => Ty::Error(e),
    }
}

pub fn fold_ptr_slice<'db>(
    folder: &mut impl TyFolder<'db>,
    slice: Slice<Ptr<Ty<'db>>>,
) -> Slice<Ptr<Ty<'db>>> {
    let src_ptrs: Vec<_> = folder.source()[slice].to_vec();
    let ptrs: Vec<_> = src_ptrs
        .iter()
        .map(|ptr| {
            let ty = folder.fold_ty(folder.source()[*ptr]);
            folder.target().alloc(ty)
        })
        .collect();
    folder.target().alloc_slice(&ptrs)
}

// ---------------------------------------------------------------------------
// Fold helpers for signature types
// ---------------------------------------------------------------------------

pub fn fold_fn_sig<'db>(folder: &mut impl TyFolder<'db>, sig: FnSig<'db>) -> FnSig<'db> {
    let params = fold_ptr_slice(folder, sig.params);
    let ret_ty = folder.fold_ty(folder.source()[sig.ret]);
    let ret = folder.target().alloc(ret_ty);
    FnSig { params, ret }
}

pub fn fold_struct_sig<'db>(
    _folder: &mut impl TyFolder<'db>,
    sig: StructSig<'db>,
) -> StructSig<'db> {
    let StructSig { dummy } = sig;
    StructSig { dummy }
}

// ---------------------------------------------------------------------------
// SubstTarget — what a generic param maps to during substitution
// ---------------------------------------------------------------------------

#[derive(Copy, Clone, Debug)]
pub enum SubstTarget<'db> {
    Ty(Ty<'db>),
    Lifetime(Lifetime<'db>),
    Const(Const<'db>),
}

// ---------------------------------------------------------------------------
// Substitute — replace GenericParam references with concrete types
// ---------------------------------------------------------------------------

pub struct Substitute<'a, 'db> {
    source: &'a Stash,
    target: &'a mut Stash,
    subst: FxHashMap<GenericParam<'db>, SubstTarget<'db>>,
}

impl<'a, 'db> Substitute<'a, 'db> {
    pub fn new(
        source: &'a Stash,
        target: &'a mut Stash,
        subst: FxHashMap<GenericParam<'db>, SubstTarget<'db>>,
    ) -> Self {
        Self {
            source,
            target,
            subst,
        }
    }
}

impl<'db> TyFolder<'db> for Substitute<'_, 'db> {
    fn target(&mut self) -> &mut Stash {
        self.target
    }

    fn source(&self) -> &Stash {
        self.source
    }

    fn fold_ty(&mut self, ty: Ty<'db>) -> Ty<'db> {
        if let Ty::Param(param) = ty
            && let Some(SubstTarget::Ty(t)) = self.subst.get(&param)
        {
            *t
        } else {
            default_fold_ty(self, ty)
        }
    }
}

// ---------------------------------------------------------------------------
// Convenience: instantiate a Binder<FnSig> / Binder<StructSig>
// ---------------------------------------------------------------------------

pub fn instantiate_fn_sig<'db>(
    source: &Stash,
    target: &mut Stash,
    binder: &Binder<'db, FnSig<'db>>,
    args: Vec<Ty<'db>>,
) -> FnSig<'db> {
    let subst = build_subst_map(source, binder.generics, &args);
    let mut folder = Substitute::new(source, target, subst);
    fold_fn_sig(&mut folder, binder.value)
}

pub fn instantiate_struct_sig<'db>(
    source: &Stash,
    target: &mut Stash,
    binder: &Binder<'db, StructSig<'db>>,
    args: Vec<Ty<'db>>,
) -> StructSig<'db> {
    let subst = build_subst_map(source, binder.generics, &args);
    let mut folder = Substitute::new(source, target, subst);
    fold_struct_sig(&mut folder, binder.value)
}

fn build_subst_map<'db>(
    source: &Stash,
    generics: Slice<GenericParam<'db>>,
    args: &[Ty<'db>],
) -> FxHashMap<GenericParam<'db>, SubstTarget<'db>> {
    let params = &source[generics];
    let mut subst = FxHashMap::default();
    for (param, arg) in params.iter().zip(args.iter()) {
        subst.insert(*param, SubstTarget::Ty(*arg));
    }
    subst
}
