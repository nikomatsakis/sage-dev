use rustc_hash::{FxHashMap, FxHashSet};
use sage_stash::{Ptr, Stash, Stashed};

use crate::generic_param::{AlphaEquivParam, GenericParam, GenericParamKind};
use crate::ty::Ty;

use super::boundary::IrCopier;
use super::{QueryResult, QueryResultData, ResponseVarInfo, SubstEntry};

struct HintInput<'db> {
    entries: FxHashMap<AlphaEquivParam<'db>, Ptr<Ty<'db>>>,
}

struct AntiUnifier<'a, 'db> {
    db: &'db dyn crate::Db,
    stash: &'a mut Stash,
    next_param: u32,
    key_universe: u32,
    witnesses: Vec<ResponseVarInfo<'db>>,
}

pub(crate) fn merge_hints<'db>(
    db: &'db dyn crate::Db,
    next_response_param: u32,
    input_universes: &FxHashMap<AlphaEquivParam<'db>, u32>,
    candidates: &[Stashed<QueryResult<'db>>],
    include_unconstrained_possible: bool,
) -> Stashed<QueryResult<'db>> {
    let mut intermediate = Stash::new();
    let mut next_renamed = next_response_param;
    let mut hints = Vec::new();

    for candidate in candidates {
        let (source, result) = candidate.open();
        let substitution = match result.value {
            QueryResultData::Yes { subst, .. } => subst,
            QueryResultData::Maybe { hints } => hints,
            QueryResultData::No => continue,
        };
        let mut mapping = FxHashMap::default();
        for info in &source[result.bound_vars] {
            let renamed = AlphaEquivParam::new(db, info.kind, next_renamed);
            next_renamed += 1;
            let ty = intermediate.alloc(Ty::Param(GenericParam::AlphaEquiv(renamed)));
            mapping.insert(GenericParam::AlphaEquiv(info.param), ty);
        }
        let mut copier = IrCopier::new(source, &mut intermediate, mapping, None);
        let entries = copier.copy_substitution(substitution);
        hints.push(HintInput {
            entries: entries.into_iter().collect(),
        });
    }
    if include_unconstrained_possible {
        hints.push(HintInput {
            entries: FxHashMap::default(),
        });
    }
    if hints.is_empty() {
        return empty_maybe();
    }

    let mut common_keys: Vec<_> = hints[0].entries.keys().copied().collect();
    common_keys.retain(|key| hints[1..].iter().all(|hint| hint.entries.contains_key(key)));
    common_keys.sort_by_key(|param| param.index(db));

    let mut merged = Vec::new();
    let mut anti = AntiUnifier {
        db,
        stash: &mut intermediate,
        next_param: next_renamed,
        key_universe: 0,
        witnesses: Vec::new(),
    };
    for key in common_keys {
        let values: Vec<_> = hints.iter().map(|hint| hint.entries[&key]).collect();
        anti.key_universe = input_universes.get(&key).copied().unwrap_or(0);
        let (value, bare_witness) = anti.merge_types(&values);
        if bare_witness
            || contains_inaccessible_input(anti.stash, value, anti.key_universe, input_universes)
        {
            continue;
        }
        merged.push(SubstEntry { key, value });
    }

    let used = collect_used_witnesses(anti.stash, &merged, &anti.witnesses);
    let mut final_stash = Stash::new();
    let mut mapping = FxHashMap::default();
    let mut bound_vars = Vec::new();
    let mut next = next_response_param;
    for witness in &anti.witnesses {
        if !used.contains(&witness.param) {
            continue;
        }
        let param = AlphaEquivParam::new(db, witness.kind, next);
        next += 1;
        let ty = final_stash.alloc(Ty::Param(GenericParam::AlphaEquiv(param)));
        mapping.insert(GenericParam::AlphaEquiv(witness.param), ty);
        bound_vars.push(ResponseVarInfo {
            param,
            kind: witness.kind,
            relative_universe: witness.relative_universe,
        });
    }
    let mut copier = IrCopier::new(&intermediate, &mut final_stash, mapping, None);
    let entries: Vec<_> = merged
        .into_iter()
        .map(|entry| SubstEntry {
            key: entry.key,
            value: copier.copy_ty(entry.value),
        })
        .collect();
    let hints = final_stash.alloc_slice(&entries);
    let bound_vars = final_stash.alloc_slice(&bound_vars);
    Stashed::new(
        final_stash,
        QueryResult {
            bound_vars,
            value: QueryResultData::Maybe { hints },
        },
    )
}

impl<'db> AntiUnifier<'_, 'db> {
    /// Returns `(merged, is_fresh_bare_witness)`.
    fn merge_types(&mut self, values: &[Ptr<Ty<'db>>]) -> (Ptr<Ty<'db>>, bool) {
        debug_assert!(!values.is_empty());
        if values.iter().all(|value| *value == values[0]) {
            return (values[0], false);
        }
        let decomposed: Vec<_> = values
            .iter()
            .map(|value| crate::check::infer::skeleton::decompose(self.stash, *value))
            .collect();
        if decomposed
            .iter()
            .all(|item| item.skeleton == decomposed[0].skeleton)
        {
            let arity = decomposed[0].children.len();
            if arity > 0 && decomposed.iter().all(|item| item.children.len() == arity) {
                let mut children = Vec::with_capacity(arity);
                for index in 0..arity {
                    let child_values: Vec<_> =
                        decomposed.iter().map(|item| item.children[index]).collect();
                    children.push(self.merge_types(&child_values).0);
                }
                return (
                    crate::check::infer::skeleton::recompose(
                        self.stash,
                        decomposed[0].skeleton,
                        &children,
                    ),
                    false,
                );
            }
        }
        let param = AlphaEquivParam::new(self.db, GenericParamKind::Type, self.next_param);
        self.next_param += 1;
        self.witnesses.push(ResponseVarInfo {
            param,
            kind: GenericParamKind::Type,
            relative_universe: self.key_universe,
        });
        (
            self.stash.alloc(Ty::Param(GenericParam::AlphaEquiv(param))),
            true,
        )
    }
}

fn contains_inaccessible_input<'db>(
    stash: &Stash,
    ty: Ptr<Ty<'db>>,
    key_universe: u32,
    input_universes: &FxHashMap<AlphaEquivParam<'db>, u32>,
) -> bool {
    match stash[ty] {
        Ty::Param(GenericParam::AlphaEquiv(param)) => input_universes
            .get(&param)
            .is_some_and(|universe| *universe > key_universe),
        Ty::Bool
        | Ty::Char
        | Ty::Int(_)
        | Ty::Uint(_)
        | Ty::Float(_)
        | Ty::Str
        | Ty::Adt(_, _)
        | Ty::Ref(_, _, _)
        | Ty::Tuple(_)
        | Ty::Slice(_)
        | Ty::Array(_, _)
        | Ty::FnPtr(_, _)
        | Ty::Param(_)
        | Ty::InferVar(_)
        | Ty::Never
        | Ty::Error(_) => crate::check::infer::skeleton::decompose(stash, ty)
            .children
            .into_iter()
            .any(|child| contains_inaccessible_input(stash, child, key_universe, input_universes)),
    }
}

fn collect_used_witnesses<'db>(
    stash: &Stash,
    entries: &[SubstEntry<'db>],
    witnesses: &[ResponseVarInfo<'db>],
) -> FxHashSet<AlphaEquivParam<'db>> {
    let all: FxHashSet<_> = witnesses.iter().map(|info| info.param).collect();
    let mut used = FxHashSet::default();
    for entry in entries {
        collect_params(stash, entry.value, &all, &mut used);
    }
    used
}

fn collect_params<'db>(
    stash: &Stash,
    ty: Ptr<Ty<'db>>,
    candidates: &FxHashSet<AlphaEquivParam<'db>>,
    output: &mut FxHashSet<AlphaEquivParam<'db>>,
) {
    if let Ty::Param(GenericParam::AlphaEquiv(param)) = stash[ty]
        && candidates.contains(&param)
    {
        output.insert(param);
    }
    for child in crate::check::infer::skeleton::decompose(stash, ty).children {
        collect_params(stash, child, candidates, output);
    }
}

fn empty_maybe<'db>() -> Stashed<QueryResult<'db>> {
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
