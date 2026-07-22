use rustc_hash::{FxHashMap, FxHashSet};
use sage_stash::{Slice, Stash, StashCopy};

use crate::check::infer::version::Version;
use crate::generic_param::GenericParamKind;
use crate::local_syms::impls::{LocalImplSym, local_impl_candidates};
use crate::symbol::{StructSymbol, Symbol, SymbolData, TraitSymbol};
use crate::ty::{SolverEligibility, TraitRef, TraitSemantics, Ty, WherePredicate};
use crate::ty_fold::{SubstTarget, Substitute, TyFolder};

use super::boundary::{IrCopier, QueryProofState};
use super::{Assumption, Atom, Goal};

#[derive(Copy, Clone, Debug)]
pub(crate) enum Candidate<'db> {
    Environment {
        head: WherePredicate<'db>,
        body: Option<Slice<Goal<'db>>>,
    },
    LocalImpl(LocalImplSym<'db>),
}

pub(crate) struct InstantiatedCandidate<'db> {
    pub head: WherePredicate<'db>,
    pub body: Slice<Goal<'db>>,
}

// ANCHOR: example_assemble_candidates
pub(crate) fn assemble_candidates<'db>(
    db: &'db dyn crate::Db,
    state: &QueryProofState<'db>,
    version: Version,
    environment: Slice<Assumption<'db>>,
    atom: Atom<'db>,
) -> (Vec<Candidate<'db>>, bool) {
    let Atom::TraitImpl { self_ty, trait_ref } = atom else {
        return (Vec::new(), false);
    };
    let mut candidates = Vec::new();
    let mut incomplete = !state.assumptions_complete;

    for assumption in &state.stash[environment] {
        match *assumption {
            Assumption::TraitImpl {
                self_ty,
                trait_ref: assumption_ref,
            } if matching_trait_ref(&state.stash, assumption_ref, trait_ref) => {
                candidates.push(Candidate::Environment {
                    head: WherePredicate {
                        self_ty,
                        trait_ref: assumption_ref,
                    },
                    body: None,
                });
            }
            Assumption::Implies(conditions, consequence) => {
                let consequence = state.stash[consequence];
                if matching_trait_ref(&state.stash, consequence.trait_ref, trait_ref) {
                    candidates.push(Candidate::Environment {
                        head: consequence,
                        body: Some(conditions),
                    });
                }
            }
            Assumption::All(_) | Assumption::TraitImpl { .. } => {}
        }
    }

    let self_root = state.egraph.find(version, self_ty);
    if matches!(state.stash[self_root], Ty::InferVar(_)) {
        return (candidates, true);
    }

    let Some(target_signature) = trait_ref.trait_sym.sig(db) else {
        return (candidates, true);
    };
    let target_data = target_signature.root().value;
    if !target_data.solver_eligibility.is_eligible() {
        return (candidates, true);
    }
    if target_data.semantics == TraitSemantics::Sized {
        match sized_certainty(db, &state.stash, self_root, &mut FxHashSet::default()) {
            SizedCertainty::Yes => candidates.push(Candidate::Environment {
                head: WherePredicate { self_ty, trait_ref },
                body: None,
            }),
            SizedCertainty::No => {}
            SizedCertainty::Maybe => incomplete = true,
        }
        return (candidates, incomplete);
    }

    let local_source = local_impl_candidates(db, state.local_crate, trait_ref.trait_sym);
    incomplete |= !local_source.complete;
    // Upstream crates may implement an upstream trait for an upstream type.
    // Until relevant external impl enumeration exists, an external trait's
    // local candidate set cannot justify a definitive negative result. An
    // unconditional local or environment Yes can still absorb this source.
    incomplete |= matches!(trait_ref.trait_sym, TraitSymbol::Ext(_));
    for &local_impl in &local_source.impls {
        let signature = local_impl.sig(db);
        let signature_data = signature.root().value;
        if signature_data.solver_eligibility == SolverEligibility::Eligible {
            candidates.push(Candidate::LocalImpl(local_impl));
        } else {
            incomplete = true;
        }
    }
    (candidates, incomplete)
}
// ANCHOR_END: example_assemble_candidates

pub(crate) fn instantiate_candidate<'db>(
    db: &'db dyn crate::Db,
    state: &mut QueryProofState<'db>,
    version: Version,
    candidate: Candidate<'db>,
) -> InstantiatedCandidate<'db> {
    match candidate {
        Candidate::Environment { head, body } => InstantiatedCandidate {
            head,
            body: body.unwrap_or_else(|| state.stash.alloc_slice(&[])),
        },
        Candidate::LocalImpl(local_impl) => instantiate_local_impl(db, state, version, local_impl),
    }
}

// ANCHOR: example_instantiate_impl
fn instantiate_local_impl<'db>(
    db: &'db dyn crate::Db,
    state: &mut QueryProofState<'db>,
    version: Version,
    local_impl: LocalImplSym<'db>,
) -> InstantiatedCandidate<'db> {
    let signature = local_impl.sig(db);
    let (source, binder) = signature.open();
    let mut mapping = FxHashMap::default();
    for generic in &source[binder.generics] {
        match generic.kind(db) {
            GenericParamKind::Type => {
                let index = state.egraph.alloc_var(version, state.canonical_universe);
                let ty = state.stash.alloc(Ty::InferVar(index));
                mapping.insert(*generic, ty);
            }
            GenericParamKind::Lifetime => {}
            GenericParamKind::Const => {
                unreachable!("const-generic impls are not eligible candidates")
            }
        }
    }
    let mut copier = IrCopier::new(source, &mut state.stash, mapping, None);
    let self_ty = copier.copy_ty(binder.value.self_ty);
    let trait_ref = copier.copy_trait_ref(
        binder
            .value
            .trait_ref
            .expect("trait candidate must have a trait reference"),
    );
    let impl_predicates = source[binder.value.where_clauses].to_vec();
    let mut body: Vec<_> = impl_predicates
        .into_iter()
        .map(|predicate| predicate_goal(&mut copier, predicate))
        .collect();
    drop(copier);

    let trait_signature = trait_ref
        .trait_sym
        .sig(db)
        .expect("eligible trait candidates must have a checked signature");
    let (trait_source, trait_binder) = trait_signature.open();
    let trait_generics = &trait_source[trait_binder.generics];
    let mut trait_mapping = FxHashMap::default();
    trait_mapping.insert(trait_binder.value.self_param, self_ty);
    for (generic, argument) in trait_generics[1..]
        .iter()
        .filter(|generic| generic.kind(db) == GenericParamKind::Type)
        .zip(state.stash[trait_ref.args].iter())
    {
        trait_mapping.insert(*generic, *argument);
    }
    let mut trait_copier = IrCopier::new(trait_source, &mut state.stash, trait_mapping, None);
    for predicate in &trait_source[trait_binder.value.where_clauses] {
        body.push(predicate_goal(&mut trait_copier, *predicate));
    }
    let body = state.stash.alloc_slice(&body);

    InstantiatedCandidate {
        head: WherePredicate { self_ty, trait_ref },
        body,
    }
}
// ANCHOR_END: example_instantiate_impl

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
enum SizedCertainty {
    Yes,
    No,
    Maybe,
}

fn sized_certainty<'db>(
    db: &'db dyn crate::Db,
    source: &Stash,
    ty: sage_stash::Ptr<Ty<'db>>,
    visiting: &mut FxHashSet<Symbol<'db>>,
) -> SizedCertainty {
    match source[ty] {
        Ty::Bool
        | Ty::Char
        | Ty::Int(_)
        | Ty::Uint(_)
        | Ty::Float(_)
        | Ty::Ref(_, _, _)
        | Ty::FnPtr(_, _)
        | Ty::Never => SizedCertainty::Yes,
        Ty::Str | Ty::Slice(_) => SizedCertainty::No,
        Ty::Array(element, _) => sized_certainty(db, source, element, visiting),
        Ty::Tuple(elements) => source[elements]
            .iter()
            .map(|element| sized_certainty(db, source, *element, visiting))
            .fold(SizedCertainty::Yes, combine_sized),
        Ty::Adt(symbol, arguments) => match symbol.data(db) {
            SymbolData::EnumSymbol(_) => SizedCertainty::Yes,
            SymbolData::StructSymbol(StructSymbol::Ext(external)) => {
                if crate::external_syms::external_adt_is_always_sized(db, external) == Some(true) {
                    SizedCertainty::Yes
                } else {
                    SizedCertainty::Maybe
                }
            }
            SymbolData::StructSymbol(StructSymbol::Local(local)) => {
                if !visiting.insert(symbol) {
                    return SizedCertainty::Maybe;
                }
                let result = local_struct_sized(db, source, arguments, local, visiting);
                visiting.remove(&symbol);
                result
            }
            SymbolData::FnSymbol(_)
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
            | SymbolData::UseSymbol(_) => SizedCertainty::Maybe,
        },
        Ty::Alias(_) => SizedCertainty::Maybe,
        Ty::Param(_) | Ty::InferVar(_) | Ty::Error(_) => SizedCertainty::Maybe,
    }
}

fn local_struct_sized<'db>(
    db: &'db dyn crate::Db,
    argument_source: &Stash,
    arguments: Slice<sage_stash::Ptr<Ty<'db>>>,
    local: crate::local_syms::structs::LocalStructSym<'db>,
    visiting: &mut FxHashSet<Symbol<'db>>,
) -> SizedCertainty {
    let fields = local.fields(db);
    let (field_source, fields) = fields.open();
    let Some(last_field) = field_source[fields.fields].last() else {
        return SizedCertainty::Yes;
    };

    let signature = local.sig(db);
    let signature_source = signature.stash();
    let type_parameters: Vec<_> = signature_source[signature.root().generics]
        .iter()
        .filter(|parameter| parameter.kind(db) == GenericParamKind::Type)
        .copied()
        .collect();
    if type_parameters.len() != argument_source[arguments].len() {
        return SizedCertainty::Maybe;
    }

    let mut target = Stash::new();
    let mut mapping = FxHashMap::default();
    for (parameter, argument) in type_parameters
        .into_iter()
        .zip(argument_source[arguments].iter())
    {
        let copied = argument_source[*argument].stash_copy(argument_source, &mut target);
        mapping.insert(parameter, SubstTarget::Ty(copied));
    }
    let mut substitute = Substitute::new(field_source, &mut target, mapping);
    let tail = substitute.fold_ty(field_source[last_field.ty]);
    let tail = target.alloc(tail);
    sized_certainty(db, &target, tail, visiting)
}

fn combine_sized(left: SizedCertainty, right: SizedCertainty) -> SizedCertainty {
    match (left, right) {
        (SizedCertainty::No, _) | (_, SizedCertainty::No) => SizedCertainty::No,
        (SizedCertainty::Maybe, _) | (_, SizedCertainty::Maybe) => SizedCertainty::Maybe,
        (SizedCertainty::Yes, SizedCertainty::Yes) => SizedCertainty::Yes,
    }
}

fn predicate_goal<'db>(
    copier: &mut IrCopier<'_, '_, 'db>,
    predicate: WherePredicate<'db>,
) -> Goal<'db> {
    let predicate = copier.copy_predicate(predicate);
    Goal::Atom(Atom::TraitImpl {
        self_ty: predicate.self_ty,
        trait_ref: predicate.trait_ref,
    })
}

fn matching_trait_ref(stash: &sage_stash::Stash, left: TraitRef<'_>, right: TraitRef<'_>) -> bool {
    left.trait_sym == right.trait_sym && stash[left.args].len() == stash[right.args].len()
}
