//! `TyFolder`: cross-stash type mapping with binder instantiation.

use sage_stash::{Slice, Stash};

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
            let args = fold_generic_arg_slice(folder, args);
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

pub fn fold_generic_arg_slice<'db>(
    folder: &mut impl TyFolder<'db>,
    slice: Slice<GenericArg<'db>>,
) -> Slice<GenericArg<'db>> {
    let src_args: Vec<_> = folder.source()[slice].to_vec();
    let args: Vec<_> = src_args
        .iter()
        .map(|arg| match arg {
            GenericArg::Type(ty) => GenericArg::Type(folder.fold_ty(*ty)),
            GenericArg::Lifetime(lt) => GenericArg::Lifetime(*lt),
            GenericArg::Const(c) => GenericArg::Const(*c),
        })
        .collect();
    folder.target().alloc_slice(&args)
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
// Instantiate — substitute BoundVars at the outermost binder
// ---------------------------------------------------------------------------

pub struct Instantiate<'a, 'db> {
    source: &'a Stash,
    target: &'a mut Stash,
    args: Vec<GenericArg<'db>>,
}

impl<'a, 'db> Instantiate<'a, 'db> {
    pub fn new(source: &'a Stash, target: &'a mut Stash, args: Vec<GenericArg<'db>>) -> Self {
        Self {
            source,
            target,
            args,
        }
    }
}

impl<'db> TyFolder<'db> for Instantiate<'_, 'db> {
    fn target(&mut self) -> &mut Stash {
        self.target
    }

    fn source(&self) -> &Stash {
        self.source
    }

    fn fold_ty(&mut self, ty: Ty<'db>) -> Ty<'db> {
        match ty.data {
            TyData::BoundVar(bv) if bv.binder_index == 0 => {
                match self.args[bv.param_index as usize] {
                    GenericArg::Type(ty) => ty,
                    _ => Ty {
                        data: TyData::Error,
                    },
                }
            }
            TyData::BoundVar(bv) => Ty {
                data: TyData::BoundVar(BoundVar {
                    binder_index: bv.binder_index - 1,
                    ..bv
                }),
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
    args: Vec<GenericArg<'db>>,
) -> FnSig<'db> {
    let mut folder = Instantiate::new(source, target, args);
    fold_fn_sig(&mut folder, binder.value)
}

pub fn instantiate_struct_sig<'db>(
    source: &Stash,
    target: &mut Stash,
    binder: &Binder<'db, StructSig<'db>>,
    args: Vec<GenericArg<'db>>,
) -> StructSig<'db> {
    let mut folder = Instantiate::new(source, target, args);
    fold_struct_sig(&mut folder, binder.value)
}
