use rustc_hash::FxHashMap;
use sage_stash::Slice;

use crate::check::infer::version::Version;
use crate::generic_param::GenericParamKind;
use crate::local_syms::impls::{LocalImplSym, local_impls};
use crate::symbol::TraitSymbol;
use crate::ty::{SolverEligibility, TraitRef, Ty, WherePredicate};

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

    let TraitSymbol::Local(target_trait) = trait_ref.trait_sym else {
        return (candidates, true);
    };
    if !target_trait
        .sig(db)
        .root()
        .value
        .solver_eligibility
        .is_eligible()
    {
        return (candidates, true);
    }

    for &local_impl in local_impls(db, state.local_crate) {
        let signature = local_impl.sig(db);
        let signature_data = signature.root().value;
        let Some(impl_ref) = signature_data.trait_ref else {
            continue;
        };
        if impl_ref.trait_sym != trait_ref.trait_sym {
            continue;
        }
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

    let TraitSymbol::Local(local_trait) = trait_ref.trait_sym else {
        unreachable!("external trait impls are not eligible candidates")
    };
    let trait_signature = local_trait.sig(db);
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
