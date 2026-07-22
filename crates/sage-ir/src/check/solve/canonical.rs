use rustc_hash::FxHashMap;
use sage_stash::{Ptr, Slice, Stash, Stashed};

use crate::check::infer::egraph::VersionedEGraph;
use crate::check::infer::version::{Universe, Version};
use crate::generic_param::{AlphaEquivParam, GenericParam, GenericParamKind};
use crate::scope::LocalCrateSymbol;
use crate::ty::{
    AliasTy, Binder, Const, NamedAliasTy, OpaqueAliasTy, ProjectionTy, TraitRef, Ty, WherePredicate,
};

use super::goal::{
    Assumption, Atom, CanonicalVarInfo, CanonicalVarRole, Goal, GoalQueryData, SolverGoal,
};

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum CallerCanonicalVar<'db> {
    Rigid(GenericParam<'db>),
    Existential(crate::ty::InferVarIndex),
}

#[derive(Clone, Debug)]
pub struct CanonicalMapping<'db> {
    pub absolute_universe_base: Universe,
    /// Same order as `GoalQueryData::canonical_vars`.
    pub inputs: Vec<CallerCanonicalVar<'db>>,
}

pub struct CanonicalizedGoal<'db> {
    pub data: Stashed<GoalQueryData<'db>>,
    pub mapping: CanonicalMapping<'db>,
}

struct InputInfo<'db> {
    param: AlphaEquivParam<'db>,
    kind: GenericParamKind,
    role: CanonicalVarRole,
    absolute_universe: Universe,
    caller: CallerCanonicalVar<'db>,
}

struct Canonicalizer<'a, 'db> {
    db: &'db dyn crate::Db,
    source: &'a Stash,
    target: Stash,
    egraph: &'a VersionedEGraph<'db>,
    version: Version,
    next_param: u32,
    free_params: FxHashMap<GenericParam<'db>, AlphaEquivParam<'db>>,
    infer_vars: FxHashMap<crate::ty::InferVarIndex, AlphaEquivParam<'db>>,
    binder_params: FxHashMap<GenericParam<'db>, GenericParam<'db>>,
    inputs: Vec<InputInfo<'db>>,
}

/// Canonicalize assumptions and a goal in one deterministic traversal.
///
/// Assumptions are visited first, followed by the requested goal. Nested
/// binders occupy the same alpha-parameter index space but never become free
/// canonical inputs.
#[allow(clippy::too_many_arguments)]
// ANCHOR: example_canonicalize_goal
pub fn canonicalize_goal<'db>(
    db: &'db dyn crate::Db,
    source: &Stash,
    egraph: &VersionedEGraph<'db>,
    version: Version,
    local_crate: LocalCrateSymbol<'db>,
    current_universe: Universe,
    assumptions_complete: bool,
    assumptions: Slice<Assumption<'db>>,
    goal: Goal<'db>,
) -> CanonicalizedGoal<'db> {
    canonicalize_solver_goal(
        db,
        source,
        egraph,
        version,
        local_crate,
        current_universe,
        assumptions_complete,
        assumptions,
        SolverGoal::Prove(goal),
    )
}

#[allow(clippy::too_many_arguments)]
pub fn canonicalize_solver_goal<'db>(
    db: &'db dyn crate::Db,
    source: &Stash,
    egraph: &VersionedEGraph<'db>,
    version: Version,
    local_crate: LocalCrateSymbol<'db>,
    current_universe: Universe,
    assumptions_complete: bool,
    assumptions: Slice<Assumption<'db>>,
    goal: SolverGoal<'db>,
) -> CanonicalizedGoal<'db> {
    let mut canonicalizer = Canonicalizer {
        db,
        source,
        target: Stash::new(),
        egraph,
        version,
        next_param: 0,
        free_params: FxHashMap::default(),
        infer_vars: FxHashMap::default(),
        binder_params: FxHashMap::default(),
        inputs: Vec::new(),
    };

    let assumptions = canonicalizer.fold_assumption_slice(assumptions);
    let goal = canonicalizer.fold_solver_goal(goal);

    let absolute_universe_base = canonicalizer
        .inputs
        .iter()
        .map(|info| info.absolute_universe)
        .chain(std::iter::once(current_universe))
        .min()
        .unwrap_or(current_universe);
    let canonical_universe = current_universe
        .0
        .checked_sub(absolute_universe_base.0)
        .expect("canonical universe base exceeds current universe");

    let canonical_vars: Vec<_> = canonicalizer
        .inputs
        .iter()
        .map(|info| CanonicalVarInfo {
            param: info.param,
            kind: info.kind,
            role: info.role,
            relative_universe: info
                .absolute_universe
                .0
                .checked_sub(absolute_universe_base.0)
                .expect("input universe precedes canonical base"),
        })
        .collect();
    let mapping = CanonicalMapping {
        absolute_universe_base,
        inputs: canonicalizer
            .inputs
            .iter()
            .map(|info| info.caller)
            .collect(),
    };
    let canonical_vars = canonicalizer.target.alloc_slice(&canonical_vars);
    let data = GoalQueryData {
        local_crate,
        canonical_universe,
        canonical_vars,
        next_response_param: canonicalizer.next_param,
        assumptions_complete,
        assumptions,
        goal,
    };

    CanonicalizedGoal {
        data: Stashed::new(canonicalizer.target, data),
        mapping,
    }
}
// ANCHOR_END: example_canonicalize_goal

impl<'db> Canonicalizer<'_, 'db> {
    fn fold_solver_goal(&mut self, goal: SolverGoal<'db>) -> SolverGoal<'db> {
        match goal {
            SolverGoal::Prove(goal) => SolverGoal::Prove(self.fold_goal(goal)),
            SolverGoal::Normalize(alias) => SolverGoal::Normalize(self.fold_alias(alias)),
        }
    }

    fn fold_alias(&mut self, alias: AliasTy<'db>) -> AliasTy<'db> {
        match alias {
            AliasTy::Named(alias) => AliasTy::Named(NamedAliasTy {
                def: alias.def,
                args: self.fold_ty_slice(alias.args),
            }),
            AliasTy::Associated(projection) => AliasTy::Associated(ProjectionTy {
                associated_ty: projection.associated_ty,
                self_ty: self.fold_ty_ptr(projection.self_ty),
                trait_ref: self.fold_trait_ref(projection.trait_ref),
                args: self.fold_ty_slice(projection.args),
            }),
            AliasTy::Opaque(alias) => AliasTy::Opaque(OpaqueAliasTy {
                def: alias.def,
                args: self.fold_ty_slice(alias.args),
            }),
        }
    }

    fn fresh_alpha(&mut self, kind: GenericParamKind) -> AlphaEquivParam<'db> {
        let index = self.next_param;
        self.next_param += 1;
        AlphaEquivParam::new(self.db, kind, index)
    }

    fn fold_goal(&mut self, goal: Goal<'db>) -> Goal<'db> {
        match goal {
            Goal::Exists(binder) => {
                let source_generics = self.source[binder.generics].to_vec();
                let mut saved = Vec::with_capacity(source_generics.len());
                let mut target_generics = Vec::with_capacity(source_generics.len());
                for source_param in source_generics {
                    let kind = source_param.kind(self.db);
                    let target_param = GenericParam::AlphaEquiv(self.fresh_alpha(kind));
                    saved.push((
                        source_param,
                        self.binder_params.insert(source_param, target_param),
                    ));
                    target_generics.push(target_param);
                }
                let value = self.fold_goal(self.source[binder.value]);
                for (source_param, previous) in saved.into_iter().rev() {
                    if let Some(previous) = previous {
                        self.binder_params.insert(source_param, previous);
                    } else {
                        self.binder_params.remove(&source_param);
                    }
                }
                let value = self.target.alloc(value);
                let generics = self.target.alloc_slice(&target_generics);
                Goal::Exists(Binder::new(value, generics))
            }
            Goal::Implies(assumptions, inner) => {
                let assumptions = self.fold_assumption_slice(assumptions);
                let inner_goal = self.fold_goal(self.source[inner]);
                let inner = self.target.alloc(inner_goal);
                Goal::Implies(assumptions, inner)
            }
            Goal::All(goals) => {
                let source_goals = self.source[goals].to_vec();
                let goals: Vec<_> = source_goals
                    .into_iter()
                    .map(|goal| self.fold_goal(goal))
                    .collect();
                Goal::all(&mut self.target, goals)
            }
            Goal::Atom(atom) => Goal::Atom(self.fold_atom(atom)),
            Goal::Maybe => Goal::Maybe,
        }
    }

    fn fold_atom(&mut self, atom: Atom<'db>) -> Atom<'db> {
        match atom {
            Atom::TraitImpl { self_ty, trait_ref } => Atom::TraitImpl {
                self_ty: self.fold_ty_ptr(self_ty),
                trait_ref: self.fold_trait_ref(trait_ref),
            },
            Atom::Equals(left, right) => {
                Atom::Equals(self.fold_ty_ptr(left), self.fold_ty_ptr(right))
            }
        }
    }

    fn fold_assumption_slice(
        &mut self,
        assumptions: Slice<Assumption<'db>>,
    ) -> Slice<Assumption<'db>> {
        let source_assumptions = self.source[assumptions].to_vec();
        let assumptions: Vec<_> = source_assumptions
            .into_iter()
            .map(|assumption| self.fold_assumption(assumption))
            .collect();
        Assumption::flatten(&mut self.target, assumptions)
    }

    fn fold_assumption(&mut self, assumption: Assumption<'db>) -> Assumption<'db> {
        match assumption {
            Assumption::TraitImpl { self_ty, trait_ref } => Assumption::TraitImpl {
                self_ty: self.fold_ty_ptr(self_ty),
                trait_ref: self.fold_trait_ref(trait_ref),
            },
            Assumption::NormalizesTo { alias, ty } => Assumption::NormalizesTo {
                alias: self.fold_alias(alias),
                ty: self.fold_ty_ptr(ty),
            },
            Assumption::Implies(conditions, consequence) => {
                let source_conditions = self.source[conditions].to_vec();
                let conditions: Vec<_> = source_conditions
                    .into_iter()
                    .map(|goal| self.fold_goal(goal))
                    .collect();
                let conditions = self.target.alloc_slice(&conditions);
                let consequence = self.fold_where_predicate(self.source[consequence]);
                let consequence = self.target.alloc(consequence);
                Assumption::Implies(conditions, consequence)
            }
            Assumption::All(assumptions) => {
                Assumption::All(self.fold_assumption_slice(assumptions))
            }
        }
    }

    fn fold_where_predicate(&mut self, predicate: WherePredicate<'db>) -> WherePredicate<'db> {
        WherePredicate {
            self_ty: self.fold_ty_ptr(predicate.self_ty),
            trait_ref: self.fold_trait_ref(predicate.trait_ref),
        }
    }

    fn fold_trait_ref(&mut self, trait_ref: TraitRef<'db>) -> TraitRef<'db> {
        let source_args = self.source[trait_ref.args].to_vec();
        let args: Vec<_> = source_args
            .into_iter()
            .map(|arg| self.fold_ty_ptr(arg))
            .collect();
        TraitRef {
            trait_sym: trait_ref.trait_sym,
            args: self.target.alloc_slice(&args),
        }
    }

    fn fold_ty_ptr(&mut self, ty: Ptr<Ty<'db>>) -> Ptr<Ty<'db>> {
        let ty = self.egraph.find(self.version, ty);
        let folded = match self.source[ty] {
            Ty::Param(param) => Ty::Param(self.fold_generic_param(param)),
            Ty::InferVar(index) => {
                let param = if let Some(param) = self.infer_vars.get(&index) {
                    *param
                } else {
                    let param = self.fresh_alpha(GenericParamKind::Type);
                    self.infer_vars.insert(index, param);
                    self.inputs.push(InputInfo {
                        param,
                        kind: GenericParamKind::Type,
                        role: CanonicalVarRole::ExistentialInput,
                        absolute_universe: self.egraph.current_universe(self.version, index),
                        caller: CallerCanonicalVar::Existential(index),
                    });
                    param
                };
                Ty::Param(GenericParam::AlphaEquiv(param))
            }
            Ty::Adt(symbol, args) => {
                let source_args = self.source[args].to_vec();
                let args: Vec<_> = source_args
                    .into_iter()
                    .map(|arg| self.fold_ty_ptr(arg))
                    .collect();
                Ty::Adt(symbol, self.target.alloc_slice(&args))
            }
            Ty::Alias(alias) => Ty::Alias(match alias {
                AliasTy::Named(alias) => AliasTy::Named(NamedAliasTy {
                    def: alias.def,
                    args: self.fold_ty_slice(alias.args),
                }),
                AliasTy::Associated(projection) => AliasTy::Associated(ProjectionTy {
                    associated_ty: projection.associated_ty,
                    self_ty: self.fold_ty_ptr(projection.self_ty),
                    trait_ref: self.fold_trait_ref(projection.trait_ref),
                    args: self.fold_ty_slice(projection.args),
                }),
                AliasTy::Opaque(alias) => AliasTy::Opaque(OpaqueAliasTy {
                    def: alias.def,
                    args: self.fold_ty_slice(alias.args),
                }),
            }),
            Ty::Ref(inner, mutability, lifetime) => {
                Ty::Ref(self.fold_ty_ptr(inner), mutability, lifetime)
            }
            Ty::Tuple(elements) => {
                let source_elements = self.source[elements].to_vec();
                let elements: Vec<_> = source_elements
                    .into_iter()
                    .map(|element| self.fold_ty_ptr(element))
                    .collect();
                Ty::Tuple(self.target.alloc_slice(&elements))
            }
            Ty::Slice(inner) => Ty::Slice(self.fold_ty_ptr(inner)),
            Ty::Array(inner, constant) => {
                Ty::Array(self.fold_ty_ptr(inner), self.fold_const(constant))
            }
            Ty::FnPtr(parameters, result) => {
                let source_parameters = self.source[parameters].to_vec();
                let parameters: Vec<_> = source_parameters
                    .into_iter()
                    .map(|parameter| self.fold_ty_ptr(parameter))
                    .collect();
                Ty::FnPtr(
                    self.target.alloc_slice(&parameters),
                    self.fold_ty_ptr(result),
                )
            }
            leaf => leaf,
        };
        self.target.alloc(folded)
    }

    fn fold_ty_slice(&mut self, source: Slice<Ptr<Ty<'db>>>) -> Slice<Ptr<Ty<'db>>> {
        let source = self.source[source].to_vec();
        let folded: Vec<_> = source
            .into_iter()
            .map(|element| self.fold_ty_ptr(element))
            .collect();
        self.target.alloc_slice(&folded)
    }

    fn fold_const(&mut self, constant: Const<'db>) -> Const<'db> {
        match constant {
            Const::Param(param) => Const::Param(self.fold_generic_param(param)),
            other => other,
        }
    }

    fn fold_generic_param(&mut self, param: GenericParam<'db>) -> GenericParam<'db> {
        if let Some(mapped) = self.binder_params.get(&param) {
            return *mapped;
        }
        if let Some(mapped) = self.free_params.get(&param) {
            return GenericParam::AlphaEquiv(*mapped);
        }

        let kind = param.kind(self.db);
        let alpha = self.fresh_alpha(kind);
        self.free_params.insert(param, alpha);
        self.inputs.push(InputInfo {
            param: alpha,
            kind,
            role: CanonicalVarRole::RigidPlaceholder,
            absolute_universe: self.egraph.placeholder_universe(param),
            caller: CallerCanonicalVar::Rigid(param),
        });
        GenericParam::AlphaEquiv(alpha)
    }
}
