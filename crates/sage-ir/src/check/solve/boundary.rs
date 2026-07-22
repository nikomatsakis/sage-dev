use rustc_hash::{FxHashMap, FxHashSet};
use sage_stash::{Ptr, Slice, Stash, Stashed};

use crate::check::infer::egraph::{CommitEffects, VersionedEGraph};
use crate::check::infer::unify::{UnifyError, unify_in_probe};
use crate::check::infer::version::{Universe, Version};
use crate::generic_param::{AlphaEquivParam, GenericParam, GenericParamKind};
use crate::scope::LocalCrateSymbol;
use crate::ty::{Binder, Const, TraitRef, Ty, WherePredicate};

use super::canonical::{CallerCanonicalVar, CanonicalMapping};
use super::goal::{Assumption, Atom, CanonicalVarRole, Goal, GoalQueryData};
use super::result::{GoalResult, QueryResult, QueryResultData, ResponseVarInfo, SubstEntry};

#[derive(Copy, Clone, Debug)]
pub struct InputInstance<'db> {
    pub param: AlphaEquivParam<'db>,
    pub role: CanonicalVarRole,
    pub ty: Ptr<Ty<'db>>,
}

/// One isolated execution of a canonical query.
pub struct QueryProofState<'db> {
    pub stash: Stash,
    pub egraph: VersionedEGraph<'db>,
    pub version: Version,
    pub inputs: Vec<InputInstance<'db>>,
    pub assumptions: Slice<Assumption<'db>>,
    pub assumptions_complete: bool,
    pub local_crate: LocalCrateSymbol<'db>,
    pub goal: Goal<'db>,
    pub canonical_universe: Universe,
    pub next_response_param: u32,
}

/// Import a canonical query into a fresh explicit-version proof egraph.
pub fn instantiate_query<'db>(query: &Stashed<GoalQueryData<'db>>) -> QueryProofState<'db> {
    let (source, data) = query.open();
    let mut stash = Stash::new();
    let mut egraph = VersionedEGraph::new();
    let version = Version::ROOT;
    let canonical_vars = source[data.canonical_vars].to_vec();
    let mut type_mapping = FxHashMap::default();
    let mut inputs = Vec::with_capacity(canonical_vars.len());

    for info in canonical_vars {
        let universe = Universe(info.relative_universe);
        let generic = GenericParam::AlphaEquiv(info.param);
        let ty = match info.role {
            CanonicalVarRole::RigidPlaceholder => {
                egraph.register_placeholder(generic, universe);
                stash.alloc(Ty::Param(generic))
            }
            CanonicalVarRole::ExistentialInput => {
                assert_eq!(info.kind, GenericParamKind::Type);
                let index = egraph.alloc_var(version, universe);
                stash.alloc(Ty::InferVar(index))
            }
        };
        type_mapping.insert(generic, ty);
        inputs.push(InputInstance {
            param: info.param,
            role: info.role,
            ty,
        });
    }

    let mut copier = IrCopier::new(source, &mut stash, type_mapping, None);
    let assumptions = copier.copy_assumption_slice(data.assumptions);
    let goal = copier.copy_goal(data.goal);

    QueryProofState {
        stash,
        egraph,
        version,
        inputs,
        assumptions,
        assumptions_complete: data.assumptions_complete,
        local_crate: data.local_crate,
        goal,
        canonical_universe: Universe(data.canonical_universe),
        next_response_param: data.next_response_param,
    }
}

/// Extract a proof state into a self-contained canonical response.
pub fn extract_query_result<'db>(
    db: &'db dyn crate::Db,
    state: &QueryProofState<'db>,
    result: GoalResult<'db>,
) -> Stashed<QueryResult<'db>> {
    extract_query_result_at(db, state, state.version, result)
}

pub(crate) fn extract_query_result_at<'db>(
    db: &'db dyn crate::Db,
    state: &QueryProofState<'db>,
    version: Version,
    result: GoalResult<'db>,
) -> Stashed<QueryResult<'db>> {
    let mut target = Stash::new();
    let mut extractor = ResponseExtractor::new(db, state, version, &mut target);

    let value = match result {
        GoalResult::No => QueryResultData::No,
        GoalResult::Maybe => {
            let hints = extractor.extract_substitution();
            QueryResultData::Maybe { hints }
        }
        GoalResult::Yes { modulo } => {
            let subst = extractor.extract_substitution();
            let modulo = extractor.copy_goal(modulo);
            QueryResultData::Yes { subst, modulo }
        }
    };
    let response_vars = std::mem::take(&mut extractor.response_vars);
    drop(extractor);
    let bound_vars = target.alloc_slice(&response_vars);
    let result = QueryResult { bound_vars, value };
    assert!(
        response_contains_no_infer_vars(&target, result),
        "canonical solver response leaked a proof-local inference variable"
    );
    Stashed::new(target, result)
}

fn response_contains_no_infer_vars(stash: &Stash, result: QueryResult<'_>) -> bool {
    let (substitution, residual) = match result.value {
        QueryResultData::Yes { subst, modulo } => (subst, Some(modulo)),
        QueryResultData::Maybe { hints } => (hints, None),
        QueryResultData::No => return true,
    };
    stash[substitution]
        .iter()
        .all(|entry| ty_contains_no_infer_vars(stash, entry.value))
        && residual.is_none_or(|goal| goal_contains_no_infer_vars(stash, goal))
}

fn goal_contains_no_infer_vars(stash: &Stash, goal: Goal<'_>) -> bool {
    match goal {
        Goal::Exists(binder) => goal_contains_no_infer_vars(stash, stash[binder.value]),
        Goal::Implies(assumptions, inner) => {
            stash[assumptions]
                .iter()
                .all(|assumption| assumption_contains_no_infer_vars(stash, *assumption))
                && goal_contains_no_infer_vars(stash, stash[inner])
        }
        Goal::All(goals) => stash[goals]
            .iter()
            .all(|goal| goal_contains_no_infer_vars(stash, *goal)),
        Goal::Atom(atom) => match atom {
            Atom::TraitImpl { self_ty, trait_ref } => {
                ty_contains_no_infer_vars(stash, self_ty)
                    && stash[trait_ref.args]
                        .iter()
                        .all(|argument| ty_contains_no_infer_vars(stash, *argument))
            }
            Atom::Equals(left, right) => {
                ty_contains_no_infer_vars(stash, left) && ty_contains_no_infer_vars(stash, right)
            }
        },
        Goal::Maybe => true,
    }
}

fn assumption_contains_no_infer_vars(stash: &Stash, assumption: Assumption<'_>) -> bool {
    match assumption {
        Assumption::TraitImpl { self_ty, trait_ref } => {
            ty_contains_no_infer_vars(stash, self_ty)
                && stash[trait_ref.args]
                    .iter()
                    .all(|argument| ty_contains_no_infer_vars(stash, *argument))
        }
        Assumption::Implies(conditions, consequence) => {
            stash[conditions]
                .iter()
                .all(|goal| goal_contains_no_infer_vars(stash, *goal))
                && {
                    let predicate = stash[consequence];
                    ty_contains_no_infer_vars(stash, predicate.self_ty)
                        && stash[predicate.trait_ref.args]
                            .iter()
                            .all(|argument| ty_contains_no_infer_vars(stash, *argument))
                }
        }
        Assumption::All(assumptions) => stash[assumptions]
            .iter()
            .all(|item| assumption_contains_no_infer_vars(stash, *item)),
    }
}

fn ty_contains_no_infer_vars(stash: &Stash, ty: Ptr<Ty<'_>>) -> bool {
    !matches!(stash[ty], Ty::InferVar(_))
        && crate::check::infer::skeleton::decompose(stash, ty)
            .children
            .into_iter()
            .all(|child| ty_contains_no_infer_vars(stash, child))
}

#[derive(Debug)]
pub enum ApplyResponseError<'db> {
    MappingArity,
    InvalidResponseKey(AlphaEquivParam<'db>),
    DuplicateResponseKey(AlphaEquivParam<'db>),
    InvalidResponseBinder(AlphaEquivParam<'db>),
    UniverseOverflow,
    Unification(UnifyError<'db>),
}

pub struct AppliedResponse<'db> {
    pub effects: CommitEffects,
    pub certainty: AppliedCertainty<'db>,
}

pub enum AppliedCertainty<'db> {
    Yes { modulo: Goal<'db> },
    Maybe,
    No,
}

/// Instantiate and transactionally apply a response to its original caller.
#[allow(clippy::too_many_arguments)]
pub fn apply_query_response<'db>(
    db: &'db dyn crate::Db,
    caller_stash: &mut Stash,
    caller_egraph: &mut VersionedEGraph<'db>,
    caller_version: Version,
    query: &Stashed<GoalQueryData<'db>>,
    mapping: &CanonicalMapping<'db>,
    response: &Stashed<QueryResult<'db>>,
) -> Result<AppliedResponse<'db>, ApplyResponseError<'db>> {
    let (query_stash, query_data) = query.open();
    let canonical_vars = &query_stash[query_data.canonical_vars];
    if canonical_vars.len() != mapping.inputs.len() {
        return Err(ApplyResponseError::MappingArity);
    }

    let (response_stash, response_data) = response.open();
    let response_substitution = match response_data.value {
        QueryResultData::No => {
            return Ok(AppliedResponse {
                effects: CommitEffects {
                    wakes: Vec::new(),
                    semantic_revision: caller_egraph.semantic_revision(caller_version),
                },
                certainty: AppliedCertainty::No,
            });
        }
        QueryResultData::Maybe { hints } => hints,
        QueryResultData::Yes { subst, .. } => subst,
    };
    let mut seen_keys = FxHashSet::default();
    for entry in &response_stash[response_substitution] {
        if !seen_keys.insert(entry.key) {
            return Err(ApplyResponseError::DuplicateResponseKey(entry.key));
        }
        let Some(position) = canonical_vars
            .iter()
            .position(|info| info.param == entry.key)
        else {
            return Err(ApplyResponseError::InvalidResponseKey(entry.key));
        };
        if canonical_vars[position].role != CanonicalVarRole::ExistentialInput
            || !matches!(mapping.inputs[position], CallerCanonicalVar::Existential(_))
        {
            return Err(ApplyResponseError::InvalidResponseKey(entry.key));
        }
    }
    let canonical_parameters: FxHashSet<_> = canonical_vars.iter().map(|info| info.param).collect();
    let mut response_parameters = FxHashSet::default();
    for info in &response_stash[response_data.bound_vars] {
        if info.kind != GenericParamKind::Type
            || canonical_parameters.contains(&info.param)
            || !response_parameters.insert(info.param)
        {
            return Err(ApplyResponseError::InvalidResponseBinder(info.param));
        }
    }

    // ANCHOR: example_apply_response_transaction
    let child = caller_egraph.branch_from(caller_version);
    let application = (|| {
        let mut type_mapping = FxHashMap::default();
        for (info, caller) in canonical_vars.iter().zip(&mapping.inputs) {
            let ty = match (*caller, info.role) {
                (CallerCanonicalVar::Rigid(param), CanonicalVarRole::RigidPlaceholder) => {
                    let universe = mapping
                        .absolute_universe_base
                        .0
                        .checked_add(info.relative_universe)
                        .ok_or(ApplyResponseError::UniverseOverflow)?;
                    caller_egraph.register_placeholder(param, Universe(universe));
                    caller_stash.alloc(Ty::Param(param))
                }
                (CallerCanonicalVar::Existential(index), CanonicalVarRole::ExistentialInput) => {
                    caller_egraph.get_var_info(child, index);
                    caller_stash.alloc(Ty::InferVar(index))
                }
                (CallerCanonicalVar::Rigid(_), CanonicalVarRole::ExistentialInput)
                | (CallerCanonicalVar::Existential(_), CanonicalVarRole::RigidPlaceholder) => {
                    return Err(ApplyResponseError::MappingArity);
                }
            };
            type_mapping.insert(GenericParam::AlphaEquiv(info.param), ty);
        }

        let mut next_binder_param = query_data.next_response_param;
        for info in &response_stash[response_data.bound_vars] {
            next_binder_param = next_binder_param.max(info.param.index(db) + 1);
            let universe = Universe(
                mapping
                    .absolute_universe_base
                    .0
                    .checked_add(info.relative_universe)
                    .ok_or(ApplyResponseError::UniverseOverflow)?,
            );
            let index = caller_egraph.alloc_var(child, universe);
            let ty = caller_stash.alloc(Ty::InferVar(index));
            type_mapping.insert(GenericParam::AlphaEquiv(info.param), ty);
        }

        let mut copier = IrCopier::new(
            response_stash,
            caller_stash,
            type_mapping,
            Some((db, next_binder_param)),
        );
        let (certainty, substitutions) = match response_data.value {
            QueryResultData::No => unreachable!(),
            QueryResultData::Maybe { hints } => {
                let substitutions = copier.copy_substitution(hints);
                (AppliedCertainty::Maybe, substitutions)
            }
            QueryResultData::Yes { subst, modulo } => {
                let substitutions = copier.copy_substitution(subst);
                let modulo = copier.copy_goal(modulo);
                (AppliedCertainty::Yes { modulo }, substitutions)
            }
        };

        for (key, value) in substitutions {
            let input = canonical_vars
                .iter()
                .position(|info| info.param == key)
                .expect("response keys were prevalidated");
            let CallerCanonicalVar::Existential(index) = mapping.inputs[input] else {
                unreachable!("response keys were prevalidated")
            };
            let input_ty = caller_stash.alloc(Ty::InferVar(index));
            unify_in_probe(caller_egraph, caller_stash, child, input_ty, value)
                .map_err(ApplyResponseError::Unification)?;
        }
        caller_egraph.rebuild(child, caller_stash);
        Ok(certainty)
    })();

    match application {
        Ok(certainty) => {
            let effects = caller_egraph.collapse_into(child, caller_version);
            Ok(AppliedResponse { effects, certainty })
        }
        Err(error) => {
            caller_egraph.discard(child);
            Err(error)
        }
    }
    // ANCHOR_END: example_apply_response_transaction
}

struct ResponseExtractor<'state, 'target, 'db> {
    db: &'db dyn crate::Db,
    state: &'state QueryProofState<'db>,
    version: Version,
    target: &'target mut Stash,
    input_indices: FxHashMap<crate::ty::InferVarIndex, AlphaEquivParam<'db>>,
    class_inputs: FxHashMap<Ptr<Ty<'db>>, AlphaEquivParam<'db>>,
    response_params: FxHashMap<crate::ty::InferVarIndex, AlphaEquivParam<'db>>,
    response_vars: Vec<ResponseVarInfo<'db>>,
    binder_params: FxHashMap<GenericParam<'db>, GenericParam<'db>>,
    next_response_param: u32,
}

impl<'state, 'target, 'db> ResponseExtractor<'state, 'target, 'db> {
    fn new(
        db: &'db dyn crate::Db,
        state: &'state QueryProofState<'db>,
        version: Version,
        target: &'target mut Stash,
    ) -> Self {
        let mut input_indices = FxHashMap::default();
        let mut class_inputs = FxHashMap::default();
        for input in &state.inputs {
            if input.role != CanonicalVarRole::ExistentialInput {
                continue;
            }
            let Ty::InferVar(index) = state.stash[input.ty] else {
                unreachable!()
            };
            input_indices.insert(index, input.param);
            let root = state.egraph.find(version, input.ty);
            class_inputs.entry(root).or_insert(input.param);
        }
        Self {
            db,
            state,
            version,
            target,
            input_indices,
            class_inputs,
            response_params: FxHashMap::default(),
            response_vars: Vec::new(),
            binder_params: FxHashMap::default(),
            next_response_param: state.next_response_param,
        }
    }

    fn extract_substitution(&mut self) -> Slice<SubstEntry<'db>> {
        let mut entries = Vec::new();
        for input in &self.state.inputs {
            if input.role != CanonicalVarRole::ExistentialInput {
                continue;
            }
            let value = self.copy_ty(input.ty);
            if self.target[value] != Ty::Param(GenericParam::AlphaEquiv(input.param)) {
                entries.push(SubstEntry {
                    key: input.param,
                    value,
                });
            }
        }
        self.target.alloc_slice(&entries)
    }

    fn copy_goal(&mut self, goal: Goal<'db>) -> Goal<'db> {
        match goal {
            Goal::Exists(binder) => {
                let source_generics = self.state.stash[binder.generics].to_vec();
                let mut saved = Vec::new();
                let mut target_generics = Vec::new();
                for source in source_generics {
                    let kind = source.kind(self.db);
                    let target = GenericParam::AlphaEquiv(AlphaEquivParam::new(
                        self.db,
                        kind,
                        self.next_response_param,
                    ));
                    self.next_response_param += 1;
                    saved.push((source, self.binder_params.insert(source, target)));
                    target_generics.push(target);
                }
                let value = self.copy_goal(self.state.stash[binder.value]);
                for (source, previous) in saved.into_iter().rev() {
                    if let Some(previous) = previous {
                        self.binder_params.insert(source, previous);
                    } else {
                        self.binder_params.remove(&source);
                    }
                }
                Goal::Exists(Binder::new(
                    self.target.alloc(value),
                    self.target.alloc_slice(&target_generics),
                ))
            }
            Goal::Implies(assumptions, inner) => {
                let assumptions = self.copy_assumption_slice(assumptions);
                let inner_value = self.copy_goal(self.state.stash[inner]);
                Goal::Implies(assumptions, self.target.alloc(inner_value))
            }
            Goal::All(goals) => {
                let source = self.state.stash[goals].to_vec();
                let copied: Vec<_> = source
                    .into_iter()
                    .map(|goal| self.copy_goal(goal))
                    .collect();
                Goal::all(self.target, copied)
            }
            Goal::Atom(atom) => Goal::Atom(self.copy_atom(atom)),
            Goal::Maybe => Goal::Maybe,
        }
    }

    fn copy_assumption_slice(
        &mut self,
        assumptions: Slice<Assumption<'db>>,
    ) -> Slice<Assumption<'db>> {
        let source = self.state.stash[assumptions].to_vec();
        let copied: Vec<_> = source
            .into_iter()
            .map(|assumption| self.copy_assumption(assumption))
            .collect();
        Assumption::flatten(self.target, copied)
    }

    fn copy_assumption(&mut self, assumption: Assumption<'db>) -> Assumption<'db> {
        match assumption {
            Assumption::TraitImpl { self_ty, trait_ref } => Assumption::TraitImpl {
                self_ty: self.copy_ty(self_ty),
                trait_ref: self.copy_trait_ref(trait_ref),
            },
            Assumption::Implies(conditions, consequence) => {
                let source = self.state.stash[conditions].to_vec();
                let conditions: Vec<_> = source
                    .into_iter()
                    .map(|goal| self.copy_goal(goal))
                    .collect();
                let conditions = self.target.alloc_slice(&conditions);
                let consequence_value = self.copy_predicate(self.state.stash[consequence]);
                Assumption::Implies(conditions, self.target.alloc(consequence_value))
            }
            Assumption::All(items) => Assumption::All(self.copy_assumption_slice(items)),
        }
    }

    fn copy_atom(&mut self, atom: Atom<'db>) -> Atom<'db> {
        match atom {
            Atom::TraitImpl { self_ty, trait_ref } => Atom::TraitImpl {
                self_ty: self.copy_ty(self_ty),
                trait_ref: self.copy_trait_ref(trait_ref),
            },
            Atom::Equals(left, right) => Atom::Equals(self.copy_ty(left), self.copy_ty(right)),
        }
    }

    fn copy_predicate(&mut self, predicate: WherePredicate<'db>) -> WherePredicate<'db> {
        WherePredicate {
            self_ty: self.copy_ty(predicate.self_ty),
            trait_ref: self.copy_trait_ref(predicate.trait_ref),
        }
    }

    fn copy_trait_ref(&mut self, trait_ref: TraitRef<'db>) -> TraitRef<'db> {
        let source = self.state.stash[trait_ref.args].to_vec();
        let args: Vec<_> = source.into_iter().map(|arg| self.copy_ty(arg)).collect();
        TraitRef {
            trait_sym: trait_ref.trait_sym,
            args: self.target.alloc_slice(&args),
        }
    }

    fn copy_ty(&mut self, ty: Ptr<Ty<'db>>) -> Ptr<Ty<'db>> {
        if let Ty::InferVar(index) = self.state.stash[ty]
            && let Some(param) = self.input_indices.get(&index)
        {
            let root = self.state.egraph.find(self.version, ty);
            if matches!(self.state.stash[root], Ty::InferVar(_))
                && self.class_inputs.get(&root) == Some(param)
            {
                return self
                    .target
                    .alloc(Ty::Param(GenericParam::AlphaEquiv(*param)));
            }
        }

        let root = self.state.egraph.find(self.version, ty);
        if let Ty::InferVar(_) = self.state.stash[root]
            && let Some(param) = self.class_inputs.get(&root)
        {
            return self
                .target
                .alloc(Ty::Param(GenericParam::AlphaEquiv(*param)));
        }
        let copied = match self.state.stash[root] {
            Ty::Param(param) => Ty::Param(self.binder_params.get(&param).copied().unwrap_or(param)),
            Ty::InferVar(index) => {
                let param = if let Some(param) = self.response_params.get(&index) {
                    *param
                } else {
                    let param = AlphaEquivParam::new(
                        self.db,
                        GenericParamKind::Type,
                        self.next_response_param,
                    );
                    self.next_response_param += 1;
                    self.response_params.insert(index, param);
                    self.response_vars.push(ResponseVarInfo {
                        param,
                        kind: GenericParamKind::Type,
                        relative_universe: self
                            .state
                            .egraph
                            .current_universe(self.version, index)
                            .0,
                    });
                    param
                };
                Ty::Param(GenericParam::AlphaEquiv(param))
            }
            Ty::Adt(symbol, args) => {
                let source = self.state.stash[args].to_vec();
                let args: Vec<_> = source.into_iter().map(|arg| self.copy_ty(arg)).collect();
                Ty::Adt(symbol, self.target.alloc_slice(&args))
            }
            Ty::Ref(inner, mutability, lifetime) => {
                Ty::Ref(self.copy_ty(inner), mutability, lifetime)
            }
            Ty::Tuple(elements) => {
                let source = self.state.stash[elements].to_vec();
                let elements: Vec<_> = source
                    .into_iter()
                    .map(|element| self.copy_ty(element))
                    .collect();
                Ty::Tuple(self.target.alloc_slice(&elements))
            }
            Ty::Slice(inner) => Ty::Slice(self.copy_ty(inner)),
            Ty::Array(inner, constant) => Ty::Array(self.copy_ty(inner), self.copy_const(constant)),
            Ty::FnPtr(parameters, result) => {
                let source = self.state.stash[parameters].to_vec();
                let parameters: Vec<_> = source
                    .into_iter()
                    .map(|parameter| self.copy_ty(parameter))
                    .collect();
                let parameters = self.target.alloc_slice(&parameters);
                let result = self.copy_ty(result);
                Ty::FnPtr(parameters, result)
            }
            leaf => leaf,
        };
        self.target.alloc(copied)
    }

    fn copy_const(&self, constant: Const<'db>) -> Const<'db> {
        match constant {
            Const::Param(param) => {
                Const::Param(self.binder_params.get(&param).copied().unwrap_or(param))
            }
            other => other,
        }
    }
}

/// Cross-stash IR copier with a free type-parameter substitution and optional
/// capture-avoiding binder freshening.
pub(crate) struct IrCopier<'source, 'target, 'db> {
    source: &'source Stash,
    target: &'target mut Stash,
    type_mapping: FxHashMap<GenericParam<'db>, Ptr<Ty<'db>>>,
    binder_mapping: FxHashMap<GenericParam<'db>, GenericParam<'db>>,
    fresh_binders: Option<(&'db dyn crate::Db, u32)>,
}

impl<'source, 'target, 'db> IrCopier<'source, 'target, 'db> {
    pub(crate) fn new(
        source: &'source Stash,
        target: &'target mut Stash,
        type_mapping: FxHashMap<GenericParam<'db>, Ptr<Ty<'db>>>,
        fresh_binders: Option<(&'db dyn crate::Db, u32)>,
    ) -> Self {
        Self {
            source,
            target,
            type_mapping,
            binder_mapping: FxHashMap::default(),
            fresh_binders,
        }
    }

    pub(crate) fn next_fresh_binder(&self) -> Option<u32> {
        self.fresh_binders.map(|(_, next)| next)
    }

    pub(crate) fn copy_substitution(
        &mut self,
        substitution: Slice<SubstEntry<'db>>,
    ) -> Vec<(AlphaEquivParam<'db>, Ptr<Ty<'db>>)> {
        self.source[substitution]
            .to_vec()
            .into_iter()
            .map(|entry| (entry.key, self.copy_ty(entry.value)))
            .collect()
    }

    pub(crate) fn copy_goal(&mut self, goal: Goal<'db>) -> Goal<'db> {
        match goal {
            Goal::Exists(binder) => {
                let source_generics = self.source[binder.generics].to_vec();
                let mut saved = Vec::new();
                let mut target_generics = Vec::new();
                for source in source_generics {
                    let target = if let Some((db, next)) = &mut self.fresh_binders {
                        let target = GenericParam::AlphaEquiv(AlphaEquivParam::new(
                            *db,
                            source.kind(*db),
                            *next,
                        ));
                        *next += 1;
                        target
                    } else {
                        source
                    };
                    saved.push((source, self.binder_mapping.insert(source, target)));
                    target_generics.push(target);
                }
                let value = self.copy_goal(self.source[binder.value]);
                for (source, previous) in saved.into_iter().rev() {
                    if let Some(previous) = previous {
                        self.binder_mapping.insert(source, previous);
                    } else {
                        self.binder_mapping.remove(&source);
                    }
                }
                Goal::Exists(Binder::new(
                    self.target.alloc(value),
                    self.target.alloc_slice(&target_generics),
                ))
            }
            Goal::Implies(assumptions, inner) => {
                let assumptions = self.copy_assumption_slice(assumptions);
                let inner_value = self.copy_goal(self.source[inner]);
                Goal::Implies(assumptions, self.target.alloc(inner_value))
            }
            Goal::All(goals) => {
                let source = self.source[goals].to_vec();
                let copied: Vec<_> = source
                    .into_iter()
                    .map(|goal| self.copy_goal(goal))
                    .collect();
                Goal::all(self.target, copied)
            }
            Goal::Atom(atom) => Goal::Atom(self.copy_atom(atom)),
            Goal::Maybe => Goal::Maybe,
        }
    }

    fn copy_assumption_slice(
        &mut self,
        assumptions: Slice<Assumption<'db>>,
    ) -> Slice<Assumption<'db>> {
        let source = self.source[assumptions].to_vec();
        let copied: Vec<_> = source
            .into_iter()
            .map(|item| self.copy_assumption(item))
            .collect();
        Assumption::flatten(self.target, copied)
    }

    fn copy_assumption(&mut self, assumption: Assumption<'db>) -> Assumption<'db> {
        match assumption {
            Assumption::TraitImpl { self_ty, trait_ref } => Assumption::TraitImpl {
                self_ty: self.copy_ty(self_ty),
                trait_ref: self.copy_trait_ref(trait_ref),
            },
            Assumption::Implies(conditions, consequence) => {
                let source = self.source[conditions].to_vec();
                let conditions: Vec<_> = source
                    .into_iter()
                    .map(|goal| self.copy_goal(goal))
                    .collect();
                let conditions = self.target.alloc_slice(&conditions);
                let consequence_value = self.copy_predicate(self.source[consequence]);
                Assumption::Implies(conditions, self.target.alloc(consequence_value))
            }
            Assumption::All(items) => Assumption::All(self.copy_assumption_slice(items)),
        }
    }

    fn copy_atom(&mut self, atom: Atom<'db>) -> Atom<'db> {
        match atom {
            Atom::TraitImpl { self_ty, trait_ref } => Atom::TraitImpl {
                self_ty: self.copy_ty(self_ty),
                trait_ref: self.copy_trait_ref(trait_ref),
            },
            Atom::Equals(left, right) => Atom::Equals(self.copy_ty(left), self.copy_ty(right)),
        }
    }

    pub(crate) fn copy_predicate(&mut self, predicate: WherePredicate<'db>) -> WherePredicate<'db> {
        WherePredicate {
            self_ty: self.copy_ty(predicate.self_ty),
            trait_ref: self.copy_trait_ref(predicate.trait_ref),
        }
    }

    pub(crate) fn copy_trait_ref(&mut self, trait_ref: TraitRef<'db>) -> TraitRef<'db> {
        let source = self.source[trait_ref.args].to_vec();
        let args: Vec<_> = source.into_iter().map(|arg| self.copy_ty(arg)).collect();
        TraitRef {
            trait_sym: trait_ref.trait_sym,
            args: self.target.alloc_slice(&args),
        }
    }

    pub(crate) fn copy_ty(&mut self, ty: Ptr<Ty<'db>>) -> Ptr<Ty<'db>> {
        if let Ty::Param(param) = self.source[ty] {
            if let Some(mapped) = self.type_mapping.get(&param) {
                return *mapped;
            }
        }
        let copied = match self.source[ty] {
            Ty::Param(param) => {
                Ty::Param(self.binder_mapping.get(&param).copied().unwrap_or(param))
            }
            Ty::Adt(symbol, args) => {
                let source = self.source[args].to_vec();
                let args: Vec<_> = source.into_iter().map(|arg| self.copy_ty(arg)).collect();
                Ty::Adt(symbol, self.target.alloc_slice(&args))
            }
            Ty::Ref(inner, mutability, lifetime) => {
                Ty::Ref(self.copy_ty(inner), mutability, lifetime)
            }
            Ty::Tuple(elements) => {
                let source = self.source[elements].to_vec();
                let elements: Vec<_> = source
                    .into_iter()
                    .map(|element| self.copy_ty(element))
                    .collect();
                Ty::Tuple(self.target.alloc_slice(&elements))
            }
            Ty::Slice(inner) => Ty::Slice(self.copy_ty(inner)),
            Ty::Array(inner, constant) => Ty::Array(self.copy_ty(inner), self.copy_const(constant)),
            Ty::FnPtr(parameters, result) => {
                let source = self.source[parameters].to_vec();
                let parameters: Vec<_> = source
                    .into_iter()
                    .map(|parameter| self.copy_ty(parameter))
                    .collect();
                let parameters = self.target.alloc_slice(&parameters);
                let result = self.copy_ty(result);
                Ty::FnPtr(parameters, result)
            }
            leaf => leaf,
        };
        self.target.alloc(copied)
    }

    fn copy_const(&self, constant: Const<'db>) -> Const<'db> {
        match constant {
            Const::Param(param) => {
                Const::Param(self.binder_mapping.get(&param).copied().unwrap_or(param))
            }
            other => other,
        }
    }
}
