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
        Ty::Alias(alias) => Ty::Alias(fold_alias_ty(folder, alias)),
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

pub fn fold_alias_ty<'db>(folder: &mut impl TyFolder<'db>, alias: AliasTy<'db>) -> AliasTy<'db> {
    match alias {
        AliasTy::Named(alias) => AliasTy::Named(NamedAliasTy {
            def: alias.def,
            args: fold_ptr_slice(folder, alias.args),
        }),
        AliasTy::Associated(projection) => {
            let self_ty = folder.fold_ty(folder.source()[projection.self_ty]);
            AliasTy::Associated(ProjectionTy {
                associated_ty: projection.associated_ty,
                self_ty: folder.target().alloc(self_ty),
                trait_ref: fold_trait_ref(folder, projection.trait_ref),
                args: fold_ptr_slice(folder, projection.args),
            })
        }
        AliasTy::Opaque(alias) => AliasTy::Opaque(OpaqueAliasTy {
            def: alias.def,
            args: fold_ptr_slice(folder, alias.args),
        }),
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
    let owner_self_ty = sig.owner_self_ty.map(|owner_self_ty| {
        let ty = folder.fold_ty(folder.source()[owner_self_ty]);
        folder.target().alloc(ty)
    });
    let receiver = sig.receiver.map(|receiver| CheckedReceiver {
        owner_self_ty: owner_self_ty.expect("a receiver must have an associated owner type"),
        form: receiver.form,
    });
    let params = fold_ptr_slice(folder, sig.params);
    let ret_ty = folder.fold_ty(folder.source()[sig.ret]);
    let ret = folder.target().alloc(ret_ty);
    let parameter_env = fold_parameter_env(folder, sig.parameter_env);
    FnSig {
        owner_generic_count: sig.owner_generic_count,
        owner_self_ty,
        receiver,
        params,
        ret,
        parameter_env,
        method_candidate_eligibility: sig.method_candidate_eligibility,
        const_call_complete: sig.const_call_complete,
    }
}

pub fn fold_struct_sig<'db>(
    folder: &mut impl TyFolder<'db>,
    sig: StructSig<'db>,
) -> StructSig<'db> {
    StructSig {
        parameter_env: fold_parameter_env(folder, sig.parameter_env),
    }
}

pub fn fold_enum_sig<'db>(folder: &mut impl TyFolder<'db>, sig: EnumSig<'db>) -> EnumSig<'db> {
    let source_variants = folder.source()[sig.variants].to_vec();
    let variants: Vec<_> = source_variants
        .into_iter()
        .map(|variant| {
            let source_fields = folder.source()[variant.fields].to_vec();
            let fields: Vec<_> = source_fields
                .into_iter()
                .map(|field| {
                    let ty = folder.fold_ty(folder.source()[field.ty]);
                    FieldSig {
                        name: field.name,
                        ty: folder.target().alloc(ty),
                    }
                })
                .collect();
            VariantSig {
                name: variant.name,
                fields: folder.target().alloc_slice(&fields),
            }
        })
        .collect();
    EnumSig {
        variants: folder.target().alloc_slice(&variants),
        parameter_env: fold_parameter_env(folder, sig.parameter_env),
    }
}

pub fn fold_trait_ref<'db>(
    folder: &mut impl TyFolder<'db>,
    trait_ref: TraitRef<'db>,
) -> TraitRef<'db> {
    TraitRef {
        trait_sym: trait_ref.trait_sym,
        args: fold_ptr_slice(folder, trait_ref.args),
    }
}

pub fn fold_where_predicate<'db>(
    folder: &mut impl TyFolder<'db>,
    predicate: WherePredicate<'db>,
) -> WherePredicate<'db> {
    let self_ty = folder.fold_ty(folder.source()[predicate.self_ty]);
    WherePredicate {
        self_ty: folder.target().alloc(self_ty),
        trait_ref: fold_trait_ref(folder, predicate.trait_ref),
    }
}

pub fn fold_parameter_env<'db>(
    folder: &mut impl TyFolder<'db>,
    env: CheckedParameterEnv<'db>,
) -> CheckedParameterEnv<'db> {
    let source = folder.source()[env.where_clauses].to_vec();
    let predicates: Vec<_> = source
        .into_iter()
        .map(|predicate| fold_where_predicate(folder, predicate))
        .collect();
    CheckedParameterEnv {
        where_clauses: folder.target().alloc_slice(&predicates),
        solver_eligibility: env.solver_eligibility,
    }
}

// ---------------------------------------------------------------------------
// SubstTarget — what a generic param maps to during substitution
// ---------------------------------------------------------------------------

#[derive(Copy, Clone, Debug)]
pub enum SubstTarget<'db> {
    Ty(Ty<'db>),
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
    db: &'db dyn crate::Db,
    source: &Stash,
    target: &mut Stash,
    binder: &Binder<'db, FnSig<'db>>,
    args: Vec<Ty<'db>>,
) -> FnSig<'db> {
    let subst = build_type_subst_map(db, source, binder.generics, &args);
    let mut folder = Substitute::new(source, target, subst);
    fold_fn_sig(&mut folder, binder.value)
}

pub fn instantiate_struct_sig<'db>(
    db: &'db dyn crate::Db,
    source: &Stash,
    target: &mut Stash,
    binder: &Binder<'db, StructSig<'db>>,
    args: Vec<Ty<'db>>,
) -> StructSig<'db> {
    let subst = build_type_subst_map(db, source, binder.generics, &args);
    let mut folder = Substitute::new(source, target, subst);
    fold_struct_sig(&mut folder, binder.value)
}

pub fn instantiate_enum_sig<'db>(
    db: &'db dyn crate::Db,
    source: &Stash,
    target: &mut Stash,
    binder: &Binder<'db, EnumSig<'db>>,
    args: Vec<Ty<'db>>,
) -> EnumSig<'db> {
    let subst = build_type_subst_map(db, source, binder.generics, &args);
    let mut folder = Substitute::new(source, target, subst);
    fold_enum_sig(&mut folder, binder.value)
}

fn build_type_subst_map<'db>(
    db: &'db dyn crate::Db,
    source: &Stash,
    generics: Slice<GenericParam<'db>>,
    args: &[Ty<'db>],
) -> FxHashMap<GenericParam<'db>, SubstTarget<'db>> {
    let params = &source[generics];
    let mut subst = FxHashMap::default();
    for (param, arg) in params
        .iter()
        .filter(|parameter| parameter.kind(db) == crate::generic_param::GenericParamKind::Type)
        .zip(args.iter())
    {
        subst.insert(*param, SubstTarget::Ty(*arg));
    }
    subst
}
