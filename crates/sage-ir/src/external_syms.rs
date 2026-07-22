//! Typed imports from the owned `TcxDb` metadata boundary.

use rustc_hash::FxHashMap;
use sage_stash::{Ptr, Stash, Stashed};

use crate::generic_param::{ExtGenericParam, GenericParam, GenericParamKind};
use crate::name::Name;
use crate::symbol::{FnSymbol, SymExt, Symbol, TraitSymbol};
use crate::tcx::{
    RawAdtSignature, RawAssociatedItemKind, RawDefId, RawFnSignature, RawGenericDefault,
    RawGenericParam, RawGenericParamKind, RawImplSignature, RawReceiver, RawSelfTypeHead,
    RawTraitSemantics, RawTraitSignature, RawTy,
};
use crate::ty::{
    Binder, CheckedParameterEnv, CheckedReceiver, Const, ExternalAdtSignature,
    ExternalAdtSignatureData, FnSig, GenericDefault, ImplSignature, ImplSignatureData,
    MethodReceiver, SolverEligibility, TraitItemDef, TraitItems, TraitRef, TraitSemantics,
    TraitSignature, TraitSignatureData, Ty, WherePredicate,
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

#[salsa::tracked]
pub fn external_adt_signature<'db>(
    db: &'db dyn crate::Db,
    adt: SymExt<'db>,
) -> Option<Stashed<ExternalAdtSignature<'db>>> {
    let raw = db
        .tcx()
        .adt_signature(adt.crate_num(db), adt.def_index(db))?;
    Some(lower_adt_signature(db, adt, raw))
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, salsa::Update)]
pub enum SimplifiedSelfType<'db> {
    Adt(SymExt<'db>),
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

impl SimplifiedSelfType<'_> {
    fn into_raw(self, db: &dyn crate::Db) -> RawSelfTypeHead {
        match self {
            Self::Adt(adt) => RawSelfTypeHead::Adt(RawDefId {
                crate_num: adt.crate_num(db),
                def_index: adt.def_index(db),
                kind: adt.kind(db),
            }),
            Self::Bool => RawSelfTypeHead::Bool,
            Self::Char => RawSelfTypeHead::Char,
            Self::Int => RawSelfTypeHead::Int,
            Self::Uint => RawSelfTypeHead::Uint,
            Self::Float => RawSelfTypeHead::Float,
            Self::Str => RawSelfTypeHead::Str,
            Self::Ref => RawSelfTypeHead::Ref,
            Self::Tuple => RawSelfTypeHead::Tuple,
            Self::Slice => RawSelfTypeHead::Slice,
            Self::Array => RawSelfTypeHead::Array,
            Self::FnPtr => RawSelfTypeHead::FnPtr,
            Self::Never => RawSelfTypeHead::Never,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, salsa::Update)]
pub struct ExternalImplCandidates<'db> {
    pub impls: Vec<SymExt<'db>>,
    pub complete: bool,
}

/// External explicit impl identities for one fixed trait and optional rigid
/// self head. Header and associated-value metadata remain separate queries.
#[salsa::tracked(returns(ref))]
pub fn external_relevant_impls<'db>(
    db: &'db dyn crate::Db,
    trait_sym: SymExt<'db>,
    self_head: Option<SimplifiedSelfType<'db>>,
) -> ExternalImplCandidates<'db> {
    let Some(raw) = db.tcx().relevant_trait_impls(
        trait_sym.crate_num(db),
        trait_sym.def_index(db),
        self_head.map(|head| head.into_raw(db)),
    ) else {
        return ExternalImplCandidates {
            impls: Vec::new(),
            complete: false,
        };
    };

    let mut complete = raw.complete;
    let mut impls = Vec::with_capacity(raw.impls.len());
    for impl_def in raw.impls {
        if impl_def.kind != crate::symbol::SymExtKind::Impl {
            complete = false;
            continue;
        }
        impls.push(SymExt::new(
            db,
            impl_def.crate_num,
            impl_def.def_index,
            impl_def.kind,
        ));
    }
    ExternalImplCandidates { impls, complete }
}

#[salsa::tracked]
pub fn external_impl_signature<'db>(
    db: &'db dyn crate::Db,
    impl_sym: SymExt<'db>,
) -> Option<Stashed<ImplSignature<'db>>> {
    let raw = db
        .tcx()
        .impl_signature(impl_sym.crate_num(db), impl_sym.def_index(db))?;
    lower_impl_signature(db, impl_sym, raw)
}

pub fn simplify_self_type<'db>(
    db: &'db dyn crate::Db,
    source: &Stash,
    ty: Ptr<Ty<'db>>,
) -> Option<SimplifiedSelfType<'db>> {
    use crate::symbol::{StructSymbol, SymbolData};

    Some(match source[ty] {
        Ty::Adt(symbol, _) => match symbol.data(db) {
            SymbolData::StructSymbol(StructSymbol::Ext(external))
            | SymbolData::EnumSymbol(crate::symbol::EnumSymbol::Ext(external)) => {
                SimplifiedSelfType::Adt(external)
            }
            SymbolData::StructSymbol(StructSymbol::Local(_))
            | SymbolData::EnumSymbol(crate::symbol::EnumSymbol::Local(_))
            | SymbolData::FnSymbol(_)
            | SymbolData::VariantSymbol(_)
            | SymbolData::VariantCtorSymbol(_)
            | SymbolData::TraitSymbol(_)
            | SymbolData::TypeAliasSymbol(_)
            | SymbolData::ConstSymbol(_)
            | SymbolData::StaticSymbol(_)
            | SymbolData::ImplSymbol(_)
            | SymbolData::ModSymbol(_)
            | SymbolData::MacroDefSymbol(_)
            | SymbolData::IntrinsicTypeSymbol(_)
            | SymbolData::MacroInvocationSymbol(_)
            | SymbolData::UseSymbol(_) => return None,
        },
        Ty::Bool => SimplifiedSelfType::Bool,
        Ty::Char => SimplifiedSelfType::Char,
        Ty::Int(_) => SimplifiedSelfType::Int,
        Ty::Uint(_) => SimplifiedSelfType::Uint,
        Ty::Float(_) => SimplifiedSelfType::Float,
        Ty::Str => SimplifiedSelfType::Str,
        Ty::Ref(..) => SimplifiedSelfType::Ref,
        Ty::Tuple(_) => SimplifiedSelfType::Tuple,
        Ty::Slice(_) => SimplifiedSelfType::Slice,
        Ty::Array(..) => SimplifiedSelfType::Array,
        Ty::FnPtr(..) => SimplifiedSelfType::FnPtr,
        Ty::Never => SimplifiedSelfType::Never,
        Ty::Alias(_) | Ty::Param(_) | Ty::InferVar(_) | Ty::Error(_) => return None,
    })
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum ApplyExternalAdtError {
    MetadataUnavailable,
    IncorrectTypeArgumentCount,
}

#[derive(Copy, Clone, Debug)]
pub struct AppliedExternalAdt<'db> {
    pub args: sage_stash::Slice<Ptr<Ty<'db>>>,
    pub parameter_env: CheckedParameterEnv<'db>,
    pub deferred_complete: bool,
}

/// Apply explicit source arguments and declaration defaults in order, then
/// instantiate the declaration's ordinary predicate environment.
pub fn apply_external_adt_signature<'db>(
    db: &'db dyn crate::Db,
    target: &mut Stash,
    adt: SymExt<'db>,
    explicit_args: &[Ptr<Ty<'db>>],
) -> Result<AppliedExternalAdt<'db>, ApplyExternalAdtError> {
    use crate::ty_fold::{SubstTarget, Substitute, TyFolder, fold_parameter_env};

    let signature =
        external_adt_signature(db, adt).ok_or(ApplyExternalAdtError::MetadataUnavailable)?;
    let (source, binder) = signature.open();
    if !binder.value.ordinary_complete {
        return Err(ApplyExternalAdtError::MetadataUnavailable);
    }
    let generics = source[binder.generics].to_vec();
    let defaults = source[binder.value.defaults].to_vec();
    if generics.len() != defaults.len() {
        return Err(ApplyExternalAdtError::MetadataUnavailable);
    }
    let type_parameter_count = generics
        .iter()
        .filter(|generic| generic.kind(db) == GenericParamKind::Type)
        .count();
    if explicit_args.len() > type_parameter_count {
        return Err(ApplyExternalAdtError::IncorrectTypeArgumentCount);
    }

    let mut explicit = explicit_args.iter().copied();
    let mut applied = Vec::with_capacity(type_parameter_count);
    let mut substitution = FxHashMap::default();
    for (generic, default) in generics.into_iter().zip(defaults) {
        if generic.kind(db) != GenericParamKind::Type {
            continue;
        }
        let argument = if let Some(argument) = explicit.next() {
            argument
        } else {
            let default = match default {
                GenericDefault::Type(default) => default,
                GenericDefault::Absent => {
                    return Err(ApplyExternalAdtError::IncorrectTypeArgumentCount);
                }
                GenericDefault::Unsupported => {
                    return Err(ApplyExternalAdtError::MetadataUnavailable);
                }
            };
            let mut folder = Substitute::new(source, target, substitution.clone());
            let default = folder.fold_ty(source[default]);
            target.alloc(default)
        };
        substitution.insert(generic, SubstTarget::Ty(target[argument]));
        applied.push(argument);
    }
    if explicit.next().is_some() {
        return Err(ApplyExternalAdtError::IncorrectTypeArgumentCount);
    }

    let mut folder = Substitute::new(source, target, substitution);
    let parameter_env = fold_parameter_env(&mut folder, binder.value.parameter_env);
    Ok(AppliedExternalAdt {
        args: target.alloc_slice(&applied),
        parameter_env,
        deferred_complete: binder.value.deferred_complete,
    })
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
        RawTraitSemantics::MetaSized => TraitSemantics::MetaSized,
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

fn lower_adt_signature<'db>(
    db: &'db dyn crate::Db,
    adt: SymExt<'db>,
    raw: RawAdtSignature,
) -> Stashed<ExternalAdtSignature<'db>> {
    let parent: Symbol<'db> = adt.into();
    let (generics, by_index) = lower_generics(db, parent, raw.generics);
    let mut stash = Stash::new();
    let mut ordinary_complete = raw.ordinary_complete;
    let mut defaults = Vec::with_capacity(raw.defaults.len());
    for default in raw.defaults {
        match default {
            RawGenericDefault::Type(default) => {
                match lower_ty(db, &mut stash, &by_index, default) {
                    Some(default) => defaults.push(GenericDefault::Type(default)),
                    None => defaults.push(GenericDefault::Unsupported),
                }
            }
            RawGenericDefault::Absent => defaults.push(GenericDefault::Absent),
            RawGenericDefault::Unsupported => defaults.push(GenericDefault::Unsupported),
        }
    }
    if defaults.len() != generics.len() {
        ordinary_complete = false;
    }

    let mut predicates = Vec::new();
    for predicate in raw.predicates {
        match lower_predicate(db, &mut stash, &by_index, predicate) {
            Some(predicate) => predicates.push(predicate),
            None => ordinary_complete = false,
        }
    }
    let where_clauses = stash.alloc_slice(&predicates);
    let defaults = stash.alloc_slice(&defaults);
    let generics = stash.alloc_slice(&generics);
    Stashed::new(
        stash,
        Binder::new(
            ExternalAdtSignatureData {
                defaults,
                parameter_env: CheckedParameterEnv {
                    where_clauses,
                    solver_eligibility: if ordinary_complete {
                        SolverEligibility::Eligible
                    } else {
                        SolverEligibility::Unsupported
                    },
                },
                ordinary_complete,
                deferred_complete: raw.deferred_complete,
            },
            generics,
        ),
    )
}

fn lower_impl_signature<'db>(
    db: &'db dyn crate::Db,
    impl_sym: SymExt<'db>,
    raw: RawImplSignature,
) -> Option<Stashed<ImplSignature<'db>>> {
    let parent: Symbol<'db> = impl_sym.into();
    let (generics, by_index) = lower_generics(db, parent, raw.generics);
    let mut stash = Stash::new();
    let mut complete = raw.complete;
    let head = lower_predicate(db, &mut stash, &by_index, raw.trait_ref)?;
    let mut predicates = Vec::new();
    for predicate in raw.predicates {
        match lower_predicate(db, &mut stash, &by_index, predicate) {
            Some(predicate) => predicates.push(predicate),
            None => complete = false,
        }
    }
    let where_clauses = stash.alloc_slice(&predicates);
    let generics = stash.alloc_slice(&generics);
    Some(Stashed::new(
        stash,
        Binder::new(
            ImplSignatureData {
                trait_ref: Some(head.trait_ref),
                self_ty: head.self_ty,
                where_clauses,
                solver_eligibility: if complete {
                    SolverEligibility::Eligible
                } else {
                    SolverEligibility::Unsupported
                },
            },
            generics,
        ),
    ))
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
