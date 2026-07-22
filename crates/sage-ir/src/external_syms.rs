//! Typed imports from the owned `TcxDb` metadata boundary.

use rustc_hash::FxHashMap;
use sage_stash::{Ptr, Stash, Stashed};

use crate::generic_param::{ExtGenericParam, GenericParam, GenericParamKind};
use crate::name::Name;
use crate::symbol::{FnSymbol, SymExt, Symbol, TraitSymbol};
use crate::tcx::{
    RawAssociatedItemKind, RawDefId, RawFnSignature, RawGenericParam, RawGenericParamKind,
    RawReceiver, RawTraitSemantics, RawTraitSignature, RawTy,
};
use crate::ty::{
    Binder, CheckedParameterEnv, CheckedReceiver, Const, FnSig, MethodReceiver, SolverEligibility,
    TraitItemDef, TraitItems, TraitRef, TraitSemantics, TraitSignature, TraitSignatureData, Ty,
    WherePredicate,
};

#[salsa::tracked]
pub fn external_trait_signature<'db>(
    db: &'db dyn crate::Db,
    trait_sym: SymExt<'db>,
) -> Option<Stashed<TraitSignature<'db>>> {
    let raw = db
        .tcx()
        .trait_signature(trait_sym.crate_num(db), trait_sym.def_index(db))?;
    Some(lower_trait_signature(db, trait_sym, raw))
}

#[salsa::tracked]
pub fn external_adt_is_always_sized<'db>(db: &'db dyn crate::Db, adt: SymExt<'db>) -> Option<bool> {
    db.tcx()
        .adt_is_always_sized(adt.crate_num(db), adt.def_index(db))
}

// ANCHOR: example_external_trait_items
#[salsa::tracked]
pub fn external_trait_items<'db>(
    db: &'db dyn crate::Db,
    trait_sym: SymExt<'db>,
) -> Option<Stashed<TraitItems<'db>>> {
    let raw = db
        .tcx()
        .associated_items(trait_sym.crate_num(db), trait_sym.def_index(db))?;
    if !raw.complete {
        return None;
    }

    let mut stash = Stash::new();
    let mut items = Vec::with_capacity(raw.items.len());
    for item in raw.items {
        let symbol = lower_def(db, item.def);
        let item = match item.kind {
            RawAssociatedItemKind::Function => TraitItemDef::Function(FnSymbol::Ext(symbol)),
            RawAssociatedItemKind::Type => TraitItemDef::Type(symbol.into()),
            RawAssociatedItemKind::Const => TraitItemDef::Const(symbol.into()),
        };
        items.push(item);
    }
    let items = stash.alloc_slice(&items);
    // Item-name discovery is intentionally independent of the owner's full
    // signature. Candidate evaluation loads and opens that signature only for
    // a matching method.
    let generics = stash.alloc_slice(&[]);
    Some(Stashed::new(stash, Binder::new(items, generics)))
}
// ANCHOR_END: example_external_trait_items

// ANCHOR: example_external_fn_signature
#[salsa::tracked]
pub fn external_fn_signature<'db>(
    db: &'db dyn crate::Db,
    fn_sym: SymExt<'db>,
) -> Option<Stashed<Binder<'db, FnSig<'db>>>> {
    let raw = db
        .tcx()
        .fn_signature(fn_sym.crate_num(db), fn_sym.def_index(db))?;
    Some(lower_fn_signature(db, fn_sym, raw))
}
// ANCHOR_END: example_external_fn_signature

fn lower_trait_signature<'db>(
    db: &'db dyn crate::Db,
    trait_sym: SymExt<'db>,
    raw: RawTraitSignature,
) -> Stashed<TraitSignature<'db>> {
    let parent: Symbol<'db> = TraitSymbol::Ext(trait_sym).into();
    let (generics, by_index) = lower_generics(db, parent, raw.generics);

    let mut stash = Stash::new();
    let mut complete = raw.complete;
    let mut predicates = Vec::new();
    for predicate in raw.predicates {
        match lower_predicate(db, &mut stash, &by_index, predicate) {
            Some(predicate) => predicates.push(predicate),
            None => complete = false,
        }
    }
    let predicates = stash.alloc_slice(&predicates);
    let generics = stash.alloc_slice(&generics);
    let self_param = by_index
        .get(&raw.self_param_index)
        .copied()
        .expect("external trait metadata must name its Self parameter");
    let semantics = match raw.semantics {
        RawTraitSemantics::Ordinary => TraitSemantics::Ordinary,
        RawTraitSemantics::Sized => TraitSemantics::Sized,
    };
    let signature = Binder::new(
        TraitSignatureData {
            self_param,
            where_clauses: predicates,
            solver_eligibility: if complete {
                SolverEligibility::Eligible
            } else {
                SolverEligibility::Unsupported
            },
            semantics,
        },
        generics,
    );
    Stashed::new(stash, signature)
}

fn lower_fn_signature<'db>(
    db: &'db dyn crate::Db,
    fn_sym: SymExt<'db>,
    raw: RawFnSignature,
) -> Stashed<Binder<'db, FnSig<'db>>> {
    let parent: Symbol<'db> = FnSymbol::Ext(fn_sym).into();
    let (generics, by_index) = lower_generics(db, parent, raw.generics);

    let mut stash = Stash::new();
    let mut complete = raw.complete;
    let had_owner = raw.owner_trait.is_some();
    let owner_predicate = raw
        .owner_trait
        .and_then(|predicate| lower_predicate(db, &mut stash, &by_index, predicate));
    if had_owner && owner_predicate.is_none() {
        complete = false;
    }
    let owner_self_ty = owner_predicate.map(|predicate| predicate.self_ty);

    let receiver = match (raw.receiver, owner_self_ty) {
        (Some(RawReceiver::Value), Some(owner_self_ty)) => Some(CheckedReceiver {
            owner_self_ty,
            form: MethodReceiver::Value {
                mutable_binding: false,
            },
        }),
        (Some(RawReceiver::Ref(mutability)), Some(owner_self_ty)) => Some(CheckedReceiver {
            owner_self_ty,
            form: MethodReceiver::Ref { mutability },
        }),
        (None, _) => None,
        (Some(_), None) => {
            complete = false;
            None
        }
    };

    let params: Option<Vec<_>> = raw
        .params
        .into_iter()
        .map(|ty| lower_ty(db, &mut stash, &by_index, ty))
        .collect();
    let params = match params {
        Some(params) => stash.alloc_slice(&params),
        None => {
            complete = false;
            stash.alloc_slice(&[])
        }
    };
    let ret = match lower_ty(db, &mut stash, &by_index, raw.ret) {
        Some(ret) => ret,
        None => {
            complete = false;
            stash.alloc(Ty::Never)
        }
    };

    let mut predicates = Vec::new();
    if let Some(owner_predicate) = owner_predicate {
        predicates.push(owner_predicate);
    }
    for predicate in raw.predicates {
        match lower_predicate(db, &mut stash, &by_index, predicate) {
            Some(predicate) => predicates.push(predicate),
            None => complete = false,
        }
    }
    let where_clauses = stash.alloc_slice(&predicates);
    let eligibility = if complete {
        SolverEligibility::Eligible
    } else {
        SolverEligibility::Unsupported
    };
    let generics = stash.alloc_slice(&generics);
    Stashed::new(
        stash,
        Binder::new(
            FnSig {
                owner_self_ty,
                receiver,
                params,
                ret,
                parameter_env: CheckedParameterEnv {
                    where_clauses,
                    solver_eligibility: eligibility,
                },
                method_candidate_eligibility: if receiver.is_some() {
                    eligibility
                } else {
                    SolverEligibility::Unsupported
                },
            },
            generics,
        ),
    )
}

fn lower_generics<'db>(
    db: &'db dyn crate::Db,
    parent: Symbol<'db>,
    raw_generics: Vec<RawGenericParam>,
) -> (Vec<GenericParam<'db>>, FxHashMap<u32, GenericParam<'db>>) {
    let mut generics = Vec::with_capacity(raw_generics.len());
    let mut by_index = FxHashMap::default();
    for raw_param in raw_generics {
        let kind = match raw_param.kind {
            RawGenericParamKind::Type => GenericParamKind::Type,
            RawGenericParamKind::Lifetime => GenericParamKind::Lifetime,
            RawGenericParamKind::Const => GenericParamKind::Const,
        };
        let name = raw_param.name.map(|name| Name::new(db, name));
        let param = GenericParam::Ext(ExtGenericParam::new(
            db,
            kind,
            name,
            parent,
            raw_param.index,
        ));
        generics.push(param);
        by_index.insert(raw_param.index, param);
    }
    (generics, by_index)
}

fn lower_predicate<'db>(
    db: &'db dyn crate::Db,
    stash: &mut Stash,
    generics: &FxHashMap<u32, GenericParam<'db>>,
    raw: crate::tcx::RawTraitPredicate,
) -> Option<WherePredicate<'db>> {
    let self_ty = lower_ty(db, stash, generics, raw.self_ty)?;
    let args: Option<Vec<_>> = raw
        .args
        .into_iter()
        .map(|ty| lower_ty(db, stash, generics, ty))
        .collect();
    let args = stash.alloc_slice(&args?);
    let trait_sym = TraitSymbol::Ext(lower_def(db, raw.trait_def));
    Some(WherePredicate {
        self_ty,
        trait_ref: TraitRef { trait_sym, args },
    })
}

fn lower_def<'db>(db: &'db dyn crate::Db, raw: RawDefId) -> SymExt<'db> {
    SymExt::new(db, raw.crate_num, raw.def_index, raw.kind)
}

fn lower_ty<'db>(
    db: &'db dyn crate::Db,
    stash: &mut Stash,
    generics: &FxHashMap<u32, GenericParam<'db>>,
    raw: RawTy,
) -> Option<Ptr<Ty<'db>>> {
    let ty = match raw {
        RawTy::Bool => Ty::Bool,
        RawTy::Char => Ty::Char,
        RawTy::Int(int_ty) => Ty::Int(int_ty),
        RawTy::Uint(uint_ty) => Ty::Uint(uint_ty),
        RawTy::Float(float_ty) => Ty::Float(float_ty),
        RawTy::Str => Ty::Str,
        RawTy::Adt(def, raw_args) => {
            let args: Option<Vec<_>> = raw_args
                .into_iter()
                .map(|ty| lower_ty(db, stash, generics, ty))
                .collect();
            Ty::Adt(lower_def(db, def).into(), stash.alloc_slice(&args?))
        }
        RawTy::Ref(inner, mutability) => Ty::Ref(
            lower_ty(db, stash, generics, *inner)?,
            mutability,
            crate::ty::Lifetime::Dummy,
        ),
        RawTy::Tuple(raw_elements) => {
            let elements: Option<Vec<_>> = raw_elements
                .into_iter()
                .map(|ty| lower_ty(db, stash, generics, ty))
                .collect();
            Ty::Tuple(stash.alloc_slice(&elements?))
        }
        RawTy::Slice(inner) => Ty::Slice(lower_ty(db, stash, generics, *inner)?),
        RawTy::Array(inner, length) => Ty::Array(
            lower_ty(db, stash, generics, *inner)?,
            Const::Literal(length),
        ),
        RawTy::Param(index) => Ty::Param(*generics.get(&index)?),
        RawTy::Never => Ty::Never,
    };
    Some(stash.alloc(ty))
}
