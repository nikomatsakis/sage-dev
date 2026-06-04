//! `TyFolder`: cross-stash type mapping.

use rustc_hash::FxHashMap;
use sage_stash::{Slice, Stash};

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
    let data = match ty.data {
        TyData::Adt(sym, args) => {
            let args = fold_slice(folder, args);
            TyData::Adt(sym, args)
        }
        TyData::Ref(inner, m, lt) => {
            let inner_ty = folder.fold_ty(folder.source()[inner]);
            let inner = folder.target().alloc(inner_ty);
            TyData::Ref(inner, m, lt)
        }
        TyData::Tuple(elems) => {
            let elems = fold_slice(folder, elems);
            TyData::Tuple(elems)
        }
        TyData::Slice(inner) => {
            let inner_ty = folder.fold_ty(folder.source()[inner]);
            let inner = folder.target().alloc(inner_ty);
            TyData::Slice(inner)
        }
        TyData::Array(inner, c) => {
            let inner_ty = folder.fold_ty(folder.source()[inner]);
            let inner = folder.target().alloc(inner_ty);
            TyData::Array(inner, c)
        }
        TyData::FnPtr(params, ret) => {
            let params = fold_slice(folder, params);
            let ret_ty = folder.fold_ty(folder.source()[ret]);
            let ret = folder.target().alloc(ret_ty);
            TyData::FnPtr(params, ret)
        }
        leaf => leaf,
    };
    Ty { data }
}

pub fn fold_slice<'db>(folder: &mut impl TyFolder<'db>, slice: Slice<Ty<'db>>) -> Slice<Ty<'db>> {
    let src_tys: Vec<_> = folder.source()[slice].to_vec();
    let tys: Vec<_> = src_tys.iter().map(|ty| folder.fold_ty(*ty)).collect();
    folder.target().alloc_slice(&tys)
}

// ---------------------------------------------------------------------------
// Fold helpers for signature types
// ---------------------------------------------------------------------------

pub fn fold_fn_sig<'db>(folder: &mut impl TyFolder<'db>, sig: FnSig<'db>) -> FnSig<'db> {
    let params = fold_slice(folder, sig.params);
    let ret_ty = folder.fold_ty(folder.source()[sig.ret]);
    let ret = folder.target().alloc(ret_ty);
    FnSig { params, ret }
}

pub fn fold_struct_sig<'db>(
    folder: &mut impl TyFolder<'db>,
    sig: StructSig<'db>,
) -> StructSig<'db> {
    let src_fields: Vec<_> = folder.source()[sig.fields].to_vec();
    let field_sigs: Vec<_> = src_fields
        .iter()
        .map(|f| {
            let src_ty = folder.source()[f.ty];
            let ty_val = folder.fold_ty(src_ty);
            let ty = folder.target().alloc(ty_val);
            FieldSig { name: f.name, ty }
        })
        .collect();
    let fields = folder.target().alloc_slice(&field_sigs);
    StructSig { fields }
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
        match ty.data {
            TyData::Param(param) => match self.subst.get(&param) {
                Some(SubstTarget::Ty(t)) => *t,
                _ => ty,
            },
            _ => default_fold_ty(self, ty),
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
