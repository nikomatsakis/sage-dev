use rustc_hash::{FxHashMap, FxHashSet};
use sage_stash::{Ptr, Stash, Stashed};

use crate::generic_param::{AlphaEquivParam, GenericParam};
use crate::ty::Ty;

use super::IrCopier;
use super::{Atom, Goal, QueryResult, QueryResultData, SubstEntry, merge_hints};

// ANCHOR: example_merge_candidates
pub(crate) fn merge_candidate_results<'db>(
    db: &'db dyn crate::Db,
    next_response_param: u32,
    input_universes: &FxHashMap<AlphaEquivParam<'db>, u32>,
    mut candidates: Vec<Stashed<QueryResult<'db>>>,
    saw_incomplete_source: bool,
) -> Stashed<QueryResult<'db>> {
    candidates.sort();
    candidates.dedup();

    if let Some(unconditional) = candidates.iter().find(|candidate| {
        let (stash, result) = candidate.open();
        matches!(
            result.value,
            QueryResultData::Yes { subst, modulo }
                if stash[subst].is_empty() && modulo.is_trivially_true(stash)
        )
    }) {
        return unconditional.clone();
    }

    let yes_candidates: Vec<_> = candidates
        .iter()
        .filter(|candidate| matches!(candidate.root().value, QueryResultData::Yes { .. }))
        .cloned()
        .collect();
    let yes = non_dominated_yes(db, input_universes, yes_candidates);
    let maybe: Vec<_> = candidates
        .iter()
        .filter(|candidate| matches!(candidate.root().value, QueryResultData::Maybe { .. }))
        .cloned()
        .collect();

    if yes.len() == 1 && maybe.is_empty() && !saw_incomplete_source {
        return yes[0].clone();
    }
    if yes.is_empty() && maybe.is_empty() && !saw_incomplete_source {
        return no_result();
    }
    if yes.is_empty() && maybe.len() == 1 && !saw_incomplete_source {
        return maybe[0].clone();
    }

    let possible: Vec<_> = yes.into_iter().chain(maybe).collect();
    merge_hints(
        db,
        next_response_param,
        input_universes,
        &possible,
        saw_incomplete_source,
    )
}
// ANCHOR_END: example_merge_candidates

fn non_dominated_yes<'db>(
    db: &'db dyn crate::Db,
    input_universes: &FxHashMap<AlphaEquivParam<'db>, u32>,
    answers: Vec<Stashed<QueryResult<'db>>>,
) -> Vec<Stashed<QueryResult<'db>>> {
    let mut keep = vec![true; answers.len()];
    for index in 0..answers.len() {
        for other in 0..answers.len() {
            if index == other {
                continue;
            }
            let other_subsumes = subsumes(db, input_universes, &answers[other], &answers[index]);
            let index_subsumes = subsumes(db, input_universes, &answers[index], &answers[other]);
            if other_subsumes && (!index_subsumes || other < index) {
                keep[index] = false;
                break;
            }
        }
    }
    answers
        .into_iter()
        .zip(keep)
        .filter_map(|(answer, keep)| keep.then_some(answer))
        .collect()
}

/// Conservative directional implication: `antecedent => consequent`.
fn subsumes<'db>(
    db: &'db dyn crate::Db,
    input_universes: &FxHashMap<AlphaEquivParam<'db>, u32>,
    consequent: &Stashed<QueryResult<'db>>,
    antecedent: &Stashed<QueryResult<'db>>,
) -> bool {
    let mut stash = Stash::new();
    let mut next_param = input_universes
        .keys()
        .map(|parameter| parameter.index(db) + 1)
        .max()
        .unwrap_or(0);
    let antecedent = copy_answer_apart(db, antecedent, &mut stash, &mut next_param);
    let consequent = copy_answer_apart(db, consequent, &mut stash, &mut next_param);
    let mut parameter_universes = input_universes.clone();
    parameter_universes.extend(antecedent.parameter_universes);
    parameter_universes.extend(consequent.parameter_universes.clone());
    let flexible: FxHashSet<_> = consequent.parameter_universes.keys().copied().collect();
    let mut bindings = FxHashMap::default();

    for required in &consequent.substitution {
        let Some(known) = antecedent
            .substitution
            .iter()
            .find(|known| known.key == required.key)
        else {
            return false;
        };
        if !directional_match_ty(
            &stash,
            required.value,
            known.value,
            &flexible,
            &parameter_universes,
            &mut bindings,
        ) {
            return false;
        }
    }

    let consequent_goals = flattened_goals(&stash, consequent.modulo);
    let antecedent_goals = flattened_goals(&stash, antecedent.modulo);
    for required in consequent_goals {
        let mut matched = false;
        for known in &antecedent_goals {
            let mut trial = bindings.clone();
            if directional_match_goal(
                &stash,
                required,
                *known,
                &flexible,
                &parameter_universes,
                &mut trial,
            ) {
                bindings = trial;
                matched = true;
                break;
            }
        }
        if !matched {
            return false;
        }
    }
    true
}

struct AnswerView<'db> {
    substitution: Vec<SubstEntry<'db>>,
    modulo: Goal<'db>,
    parameter_universes: FxHashMap<AlphaEquivParam<'db>, u32>,
}

fn copy_answer_apart<'db>(
    db: &'db dyn crate::Db,
    answer: &Stashed<QueryResult<'db>>,
    target: &mut Stash,
    next_param: &mut u32,
) -> AnswerView<'db> {
    let (source, result) = answer.open();
    let QueryResultData::Yes { subst, modulo } = result.value else {
        unreachable!("subsumption only compares definite answers")
    };
    let mut mapping = FxHashMap::default();
    let mut parameter_universes = FxHashMap::default();
    for info in &source[result.bound_vars] {
        let renamed = AlphaEquivParam::new(db, info.kind, *next_param);
        *next_param += 1;
        let ty = target.alloc(Ty::Param(GenericParam::AlphaEquiv(renamed)));
        mapping.insert(GenericParam::AlphaEquiv(info.param), ty);
        parameter_universes.insert(renamed, info.relative_universe);
    }
    let mut copier = IrCopier::new(source, target, mapping, Some((db, *next_param)));
    let substitution = copier
        .copy_substitution(subst)
        .into_iter()
        .map(|(key, value)| SubstEntry { key, value })
        .collect();
    let modulo = copier.copy_goal(modulo);
    if let Some(next) = copier.next_fresh_binder() {
        *next_param = next;
    }
    AnswerView {
        substitution,
        modulo,
        parameter_universes,
    }
}

fn directional_match_goal<'db>(
    stash: &Stash,
    required: Goal<'db>,
    known: Goal<'db>,
    flexible: &FxHashSet<AlphaEquivParam<'db>>,
    universes: &FxHashMap<AlphaEquivParam<'db>, u32>,
    bindings: &mut FxHashMap<AlphaEquivParam<'db>, Ptr<Ty<'db>>>,
) -> bool {
    match (required, known) {
        (Goal::Atom(required), Goal::Atom(known)) => match (required, known) {
            (
                Atom::TraitImpl {
                    self_ty: required_self,
                    trait_ref: required_ref,
                },
                Atom::TraitImpl {
                    self_ty: known_self,
                    trait_ref: known_ref,
                },
            ) if required_ref.trait_sym == known_ref.trait_sym
                && stash[required_ref.args].len() == stash[known_ref.args].len() =>
            {
                directional_match_ty(
                    stash,
                    required_self,
                    known_self,
                    flexible,
                    universes,
                    bindings,
                ) && stash[required_ref.args]
                    .iter()
                    .zip(&stash[known_ref.args])
                    .all(|(required, known)| {
                        directional_match_ty(
                            stash, *required, *known, flexible, universes, bindings,
                        )
                    })
            }
            (Atom::TraitImpl { .. }, Atom::TraitImpl { .. }) => false,
            (
                Atom::Equals(required_left, required_right),
                Atom::Equals(known_left, known_right),
            ) => {
                let mut direct = bindings.clone();
                if directional_match_ty(
                    stash,
                    required_left,
                    known_left,
                    flexible,
                    universes,
                    &mut direct,
                ) && directional_match_ty(
                    stash,
                    required_right,
                    known_right,
                    flexible,
                    universes,
                    &mut direct,
                ) {
                    *bindings = direct;
                    true
                } else {
                    directional_match_ty(
                        stash,
                        required_left,
                        known_right,
                        flexible,
                        universes,
                        bindings,
                    ) && directional_match_ty(
                        stash,
                        required_right,
                        known_left,
                        flexible,
                        universes,
                        bindings,
                    )
                }
            }
            (Atom::TraitImpl { .. }, Atom::Equals(_, _))
            | (Atom::Equals(_, _), Atom::TraitImpl { .. }) => false,
        },
        (Goal::Maybe, Goal::Maybe) => true,
        // Binder/implication reasoning is deliberately conservative in the MVP.
        (Goal::Exists(required), Goal::Exists(known)) => required == known,
        (
            Goal::Implies(required_assumptions, required_goal),
            Goal::Implies(known_assumptions, known_goal),
        ) => required_assumptions == known_assumptions && required_goal == known_goal,
        (Goal::All(required), Goal::All(known)) => required == known,
        (
            Goal::Exists(_) | Goal::Implies(_, _) | Goal::All(_) | Goal::Atom(_) | Goal::Maybe,
            Goal::Exists(_) | Goal::Implies(_, _) | Goal::All(_) | Goal::Atom(_) | Goal::Maybe,
        ) => false,
    }
}

fn directional_match_ty<'db>(
    stash: &Stash,
    required: Ptr<Ty<'db>>,
    known: Ptr<Ty<'db>>,
    flexible: &FxHashSet<AlphaEquivParam<'db>>,
    universes: &FxHashMap<AlphaEquivParam<'db>, u32>,
    bindings: &mut FxHashMap<AlphaEquivParam<'db>, Ptr<Ty<'db>>>,
) -> bool {
    if required == known {
        return true;
    }
    if let Ty::Param(GenericParam::AlphaEquiv(parameter)) = stash[required]
        && flexible.contains(&parameter)
    {
        if let Some(previous) = bindings.get(&parameter) {
            return structural_ty_eq(stash, *previous, known);
        }
        let ceiling = universes.get(&parameter).copied().unwrap_or(0);
        if contains_newer_parameter(stash, known, ceiling, universes) {
            return false;
        }
        bindings.insert(parameter, known);
        return true;
    }

    let required = crate::check::infer::skeleton::decompose(stash, required);
    let known = crate::check::infer::skeleton::decompose(stash, known);
    required.skeleton == known.skeleton
        && required.children.len() == known.children.len()
        && required
            .children
            .iter()
            .zip(known.children.iter())
            .all(|(required, known)| {
                directional_match_ty(stash, *required, *known, flexible, universes, bindings)
            })
}

fn structural_ty_eq(stash: &Stash, left: Ptr<Ty<'_>>, right: Ptr<Ty<'_>>) -> bool {
    if left == right {
        return true;
    }
    let left = crate::check::infer::skeleton::decompose(stash, left);
    let right = crate::check::infer::skeleton::decompose(stash, right);
    left.skeleton == right.skeleton
        && left.children.len() == right.children.len()
        && left
            .children
            .iter()
            .zip(right.children.iter())
            .all(|(left, right)| structural_ty_eq(stash, *left, *right))
}

fn contains_newer_parameter<'db>(
    stash: &Stash,
    ty: Ptr<Ty<'db>>,
    ceiling: u32,
    universes: &FxHashMap<AlphaEquivParam<'db>, u32>,
) -> bool {
    if let Ty::Param(GenericParam::AlphaEquiv(parameter)) = stash[ty]
        && universes
            .get(&parameter)
            .is_some_and(|universe| *universe > ceiling)
    {
        return true;
    }
    crate::check::infer::skeleton::decompose(stash, ty)
        .children
        .into_iter()
        .any(|child| contains_newer_parameter(stash, child, ceiling, universes))
}

fn flattened_goals<'db>(stash: &Stash, goal: Goal<'db>) -> Vec<Goal<'db>> {
    let mut output = Vec::new();
    fn push<'db>(stash: &Stash, goal: Goal<'db>, output: &mut Vec<Goal<'db>>) {
        match goal {
            Goal::All(goals) => {
                for goal in &stash[goals] {
                    push(stash, *goal, output);
                }
            }
            other => output.push(other),
        }
    }
    push(stash, goal, &mut output);
    let mut unique = Vec::new();
    for goal in output {
        if !unique.contains(&goal) {
            unique.push(goal);
        }
    }
    unique
}

fn no_result<'db>() -> Stashed<QueryResult<'db>> {
    let mut stash = Stash::new();
    let bound_vars = stash.alloc_slice(&[]);
    Stashed::new(
        stash,
        QueryResult {
            bound_vars,
            value: QueryResultData::No,
        },
    )
}

#[cfg(test)]
mod tests {
    use super::super::SubstEntry;
    use super::*;
    use crate::db::Database;
    use crate::generic_param::{AlphaEquivParam, GenericParamKind};
    use crate::ty::{IntTy, Ty};
    use sage_stash::StashCopy;

    fn yes(trivial: bool) -> Stashed<QueryResult<'static>> {
        let mut stash = Stash::new();
        let bound_vars = stash.alloc_slice(&[]);
        let subst = stash.alloc_slice(&[]);
        let modulo = if trivial {
            Goal::true_(&mut stash)
        } else {
            Goal::Maybe
        };
        Stashed::new(
            stash,
            QueryResult {
                bound_vars,
                value: QueryResultData::Yes { subst, modulo },
            },
        )
    }

    #[test]
    fn unconditional_yes_wins_over_incomplete_source() {
        let db = Database::default();
        let result = merge_candidate_results(&db, 0, &FxHashMap::default(), vec![yes(true)], true);
        assert!(matches!(result.root().value, QueryResultData::Yes { .. }));
    }

    #[test]
    fn duplicate_identical_yes_is_one_answer() {
        let db = Database::default();
        let answer = yes(false);
        let result = merge_candidate_results(
            &db,
            0,
            &FxHashMap::default(),
            vec![answer.clone(), answer],
            false,
        );
        assert!(matches!(result.root().value, QueryResultData::Yes { .. }));
    }

    #[test]
    fn no_candidates_is_no_only_for_complete_source() {
        let db = Database::default();
        assert!(matches!(
            merge_candidate_results(&db, 0, &FxHashMap::default(), Vec::new(), false)
                .root()
                .value,
            QueryResultData::No
        ));
        assert!(matches!(
            merge_candidate_results(&db, 0, &FxHashMap::default(), Vec::new(), true)
                .root()
                .value,
            QueryResultData::Maybe { .. }
        ));
    }

    fn maybe() -> Stashed<QueryResult<'static>> {
        let mut stash = Stash::new();
        let bound_vars = stash.alloc_slice(&[]);
        let hints = stash.alloc_slice(&[]);
        Stashed::new(
            stash,
            QueryResult {
                bound_vars,
                value: QueryResultData::Maybe { hints },
            },
        )
    }

    #[test]
    fn final_answer_rules_are_order_independent() {
        let db = Database::default();
        let no = no_result();
        let maybe = maybe();
        let unconditional = yes(true);
        for candidates in [
            vec![no.clone(), maybe.clone(), unconditional.clone()],
            vec![unconditional.clone(), no.clone(), maybe.clone()],
            vec![maybe.clone(), unconditional.clone(), no.clone()],
        ] {
            let result = merge_candidate_results(&db, 0, &FxHashMap::default(), candidates, false);
            assert_eq!(result, unconditional);
        }

        let all_maybe = merge_candidate_results(
            &db,
            0,
            &FxHashMap::default(),
            vec![maybe.clone(), maybe],
            false,
        );
        assert!(matches!(
            all_maybe.root().value,
            QueryResultData::Maybe { .. }
        ));
        let all_no =
            merge_candidate_results(&db, 0, &FxHashMap::default(), vec![no.clone(), no], false);
        assert!(matches!(all_no.root().value, QueryResultData::No));
    }

    fn conditional_answer<'db>(
        key: AlphaEquivParam<'db>,
        value: Ty<'db>,
    ) -> Stashed<QueryResult<'db>> {
        let mut stash = Stash::new();
        let value = stash.alloc(value);
        let subst = stash.alloc_slice(&[SubstEntry { key, value }]);
        let bound_vars = stash.alloc_slice(&[]);
        let modulo = Goal::true_(&mut stash);
        Stashed::new(
            stash,
            QueryResult {
                bound_vars,
                value: QueryResultData::Yes { subst, modulo },
            },
        )
    }

    #[test]
    fn divergent_bare_values_produce_empty_hard_hint() {
        let db = Database::default();
        let key = AlphaEquivParam::new(&db, GenericParamKind::Type, 0);
        let inputs = FxHashMap::from_iter([(key, 0)]);
        let result = merge_candidate_results(
            &db,
            1,
            &inputs,
            vec![
                conditional_answer(key, Ty::Int(IntTy::I32)),
                conditional_answer(key, Ty::Bool),
            ],
            false,
        );
        let (stash, result) = result.open();
        let QueryResultData::Maybe { hints } = result.value else {
            panic!("incomparable answers must be ambiguous")
        };
        assert!(stash[hints].is_empty());
        assert!(stash[result.bound_vars].is_empty());
    }

    #[test]
    fn anti_unification_retains_shared_constructor_and_component() {
        let db = Database::default();
        let key = AlphaEquivParam::new(&db, GenericParamKind::Type, 0);
        let inputs = FxHashMap::from_iter([(key, 0)]);
        fn tuple_answer<'db>(
            key: AlphaEquivParam<'db>,
            first: Ty<'db>,
        ) -> Stashed<QueryResult<'db>> {
            let mut stash = Stash::new();
            let first = stash.alloc(first);
            let boolean = stash.alloc(Ty::Bool);
            let elements = stash.alloc_slice(&[first, boolean]);
            let value = stash.alloc(Ty::Tuple(elements));
            let subst = stash.alloc_slice(&[SubstEntry { key, value }]);
            let bound_vars = stash.alloc_slice(&[]);
            let modulo = Goal::true_(&mut stash);
            Stashed::new(
                stash,
                QueryResult {
                    bound_vars,
                    value: QueryResultData::Yes { subst, modulo },
                },
            )
        }
        let result = merge_candidate_results(
            &db,
            1,
            &inputs,
            vec![
                tuple_answer(key, Ty::Int(IntTy::I32)),
                tuple_answer(key, Ty::Bool),
            ],
            false,
        );
        let (stash, result) = result.open();
        let QueryResultData::Maybe { hints } = result.value else {
            panic!("incomparable answers must be ambiguous")
        };
        let [hint] = &stash[hints] else {
            panic!("shared tuple shape should remain a hard hint")
        };
        let Ty::Tuple(elements) = stash[hint.value] else {
            panic!("expected tuple hint")
        };
        let elements = &stash[elements];
        assert!(matches!(stash[elements[0]], Ty::Param(_)));
        assert_eq!(stash[elements[1]], Ty::Bool);
        assert_eq!(stash[result.bound_vars].len(), 1);
    }

    #[test]
    fn general_conditional_answer_subsumes_specific_one() {
        let db = Database::default();
        let key = AlphaEquivParam::new(&db, GenericParamKind::Type, 0);
        let inputs = FxHashMap::from_iter([(key, 0)]);
        let mut stash = Stash::new();
        let bound_vars = stash.alloc_slice(&[]);
        let subst = stash.alloc_slice(&[]);
        let general = Stashed::new(
            stash,
            QueryResult {
                bound_vars,
                value: QueryResultData::Yes {
                    subst,
                    modulo: Goal::Maybe,
                },
            },
        );
        let specific = {
            let answer = conditional_answer(key, Ty::Bool);
            let (source, result) = answer.open();
            let QueryResultData::Yes { subst, .. } = result.value else {
                unreachable!()
            };
            let mut target = Stash::new();
            let subst = subst.stash_copy(source, &mut target);
            let bound_vars = target.alloc_slice(&[]);
            Stashed::new(
                target,
                QueryResult {
                    bound_vars,
                    value: QueryResultData::Yes {
                        subst,
                        modulo: Goal::Maybe,
                    },
                },
            )
        };
        let result = merge_candidate_results(&db, 1, &inputs, vec![specific, general], false);
        assert!(matches!(result.root().value, QueryResultData::Yes { .. }));
        let QueryResultData::Yes { subst, .. } = result.root().value else {
            unreachable!()
        };
        assert!(result.stash()[subst].is_empty());
    }

    fn repeated_witness_answer<'db>(
        db: &'db dyn crate::Db,
        key: AlphaEquivParam<'db>,
        witness: AlphaEquivParam<'db>,
        universe: u32,
    ) -> Stashed<QueryResult<'db>> {
        let mut stash = Stash::new();
        let witness_ty = stash.alloc(Ty::Param(GenericParam::AlphaEquiv(witness)));
        let elements = stash.alloc_slice(&[witness_ty, witness_ty]);
        let value = stash.alloc(Ty::Tuple(elements));
        let subst = stash.alloc_slice(&[SubstEntry { key, value }]);
        let bound_vars = stash.alloc_slice(&[super::super::ResponseVarInfo {
            param: witness,
            kind: GenericParamKind::Type,
            relative_universe: universe,
        }]);
        let modulo = Goal::true_(&mut stash);
        let _ = db;
        Stashed::new(
            stash,
            QueryResult {
                bound_vars,
                value: QueryResultData::Yes { subst, modulo },
            },
        )
    }

    #[test]
    fn directional_subsumption_binds_only_repeated_consequent_witnesses() {
        let db = Database::default();
        let key = AlphaEquivParam::new(&db, GenericParamKind::Type, 0);
        let witness = AlphaEquivParam::new(&db, GenericParamKind::Type, 1);
        let inputs = FxHashMap::from_iter([(key, 0)]);
        let general = repeated_witness_answer(&db, key, witness, 0);

        fn tuple_answer<'db>(
            key: AlphaEquivParam<'db>,
            left: Ty<'db>,
            right: Ty<'db>,
        ) -> Stashed<QueryResult<'db>> {
            let mut stash = Stash::new();
            let left = stash.alloc(left);
            let right = stash.alloc(right);
            let elements = stash.alloc_slice(&[left, right]);
            let value = stash.alloc(Ty::Tuple(elements));
            let subst = stash.alloc_slice(&[SubstEntry { key, value }]);
            let bound_vars = stash.alloc_slice(&[]);
            let modulo = Goal::true_(&mut stash);
            Stashed::new(
                stash,
                QueryResult {
                    bound_vars,
                    value: QueryResultData::Yes { subst, modulo },
                },
            )
        }

        let repeated = tuple_answer(key, Ty::Int(IntTy::I32), Ty::Int(IntTy::I32));
        let result =
            merge_candidate_results(&db, 2, &inputs, vec![repeated, general.clone()], false);
        assert_eq!(result, general);

        let non_repeated = tuple_answer(key, Ty::Int(IntTy::I32), Ty::Bool);
        let result = merge_candidate_results(&db, 2, &inputs, vec![non_repeated, general], false);
        assert!(matches!(result.root().value, QueryResultData::Maybe { .. }));
    }

    #[test]
    fn subsumption_witness_cannot_capture_a_newer_rigid_input() {
        let db = Database::default();
        let key = AlphaEquivParam::new(&db, GenericParamKind::Type, 0);
        let witness = AlphaEquivParam::new(&db, GenericParamKind::Type, 1);
        let newer = AlphaEquivParam::new(&db, GenericParamKind::Type, 2);
        let inputs = FxHashMap::from_iter([(key, 0), (newer, 1)]);
        let general = {
            let mut stash = Stash::new();
            let value = stash.alloc(Ty::Param(GenericParam::AlphaEquiv(witness)));
            let subst = stash.alloc_slice(&[SubstEntry { key, value }]);
            let bound_vars = stash.alloc_slice(&[super::super::ResponseVarInfo {
                param: witness,
                kind: GenericParamKind::Type,
                relative_universe: 0,
            }]);
            let modulo = Goal::true_(&mut stash);
            Stashed::new(
                stash,
                QueryResult {
                    bound_vars,
                    value: QueryResultData::Yes { subst, modulo },
                },
            )
        };
        let specific = conditional_answer(key, Ty::Param(GenericParam::AlphaEquiv(newer)));
        let result = merge_candidate_results(&db, 3, &inputs, vec![specific, general], false);
        assert!(matches!(result.root().value, QueryResultData::Maybe { .. }));
    }
}
