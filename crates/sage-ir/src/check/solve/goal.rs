use rustc_hash::FxHashSet;
use sage_stash::{AllocStashData, Ptr, Slice, Stash, Stashed};

use crate::generic_param::{AlphaEquivParam, GenericParam, GenericParamKind};
use crate::scope::LocalCrateSymbol;
use crate::ty::{AliasTy, Binder, TraitRef, Ty, WherePredicate};

pub const MAX_PROOF_DEPTH: u32 = 64;

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, AllocStashData)]
pub struct GoalQueryData<'db> {
    pub local_crate: LocalCrateSymbol<'db>,
    pub canonical_universe: u32,
    pub canonical_vars: Slice<CanonicalVarInfo<'db>>,
    pub next_response_param: u32,
    pub assumptions_complete: bool,
    pub assumptions: Slice<Assumption<'db>>,
    pub goal: SolverGoal<'db>,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, AllocStashData)]
pub struct CanonicalVarInfo<'db> {
    pub param: AlphaEquivParam<'db>,
    pub kind: GenericParamKind,
    pub role: CanonicalVarRole,
    pub relative_universe: u32,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, AllocStashData)]
pub enum CanonicalVarRole {
    RigidPlaceholder,
    ExistentialInput,
}

/// A top-level solver operation. Structural proof goals remain propositions;
/// value-producing operations are not encoded by adding an output variable to
/// that proposition language.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, AllocStashData)]
pub enum SolverGoal<'db> {
    Prove(Goal<'db>),
    Normalize(AliasTy<'db>),
}

// ANCHOR: example_solver_goal_ir
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, AllocStashData)]
pub enum Goal<'db> {
    Exists(Binder<'db, Ptr<Goal<'db>>>),
    Implies(Slice<Assumption<'db>>, Ptr<Goal<'db>>),
    All(Slice<Goal<'db>>),
    Atom(Atom<'db>),
    Maybe,
}

impl<'db> Goal<'db> {
    pub fn true_(stash: &mut Stash) -> Self {
        Self::All(stash.alloc_slice(&[]))
    }

    pub fn is_trivially_true(self, stash: &Stash) -> bool {
        matches!(self, Goal::All(goals) if stash[goals].is_empty())
    }

    /// Allocate a canonical conjunction, flattening nested `All` nodes and
    /// eliminating trivially true children while preserving source order.
    pub fn all(stash: &mut Stash, goals: impl IntoIterator<Item = Goal<'db>>) -> Self {
        fn push<'db>(stash: &Stash, goal: Goal<'db>, output: &mut Vec<Goal<'db>>) {
            match goal {
                Goal::All(goals) => {
                    for nested in &stash[goals] {
                        push(stash, *nested, output);
                    }
                }
                other => output.push(other),
            }
        }

        let mut flattened = Vec::new();
        for goal in goals {
            push(stash, goal, &mut flattened);
        }
        Goal::All(stash.alloc_slice(&flattened))
    }
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, AllocStashData)]
pub enum Atom<'db> {
    TraitImpl {
        self_ty: Ptr<Ty<'db>>,
        trait_ref: TraitRef<'db>,
    },
    Equals(Ptr<Ty<'db>>, Ptr<Ty<'db>>),
}
// ANCHOR_END: example_solver_goal_ir

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, AllocStashData)]
pub enum Assumption<'db> {
    TraitImpl {
        self_ty: Ptr<Ty<'db>>,
        trait_ref: TraitRef<'db>,
    },
    /// A value-bearing normalization fact. Unlike `TraitImpl`, this supplies
    /// an associated value and can therefore answer `Normalize`.
    NormalizesTo {
        alias: AliasTy<'db>,
        ty: Ptr<Ty<'db>>,
    },
    Implies(Slice<Goal<'db>>, Ptr<WherePredicate<'db>>),
    All(Slice<Assumption<'db>>),
}

impl<'db> Assumption<'db> {
    pub fn flatten(stash: &mut Stash, assumptions: impl IntoIterator<Item = Self>) -> Slice<Self> {
        fn push<'db>(stash: &Stash, item: Assumption<'db>, output: &mut Vec<Assumption<'db>>) {
            match item {
                Assumption::All(items) => {
                    for nested in &stash[items] {
                        push(stash, *nested, output);
                    }
                }
                other => output.push(other),
            }
        }

        let mut flattened = Vec::new();
        for assumption in assumptions {
            push(stash, assumption, &mut flattened);
        }
        stash.alloc_slice(&flattened)
    }
}

/// Diagnostic/provenance category retained with a clause.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, AllocStashData)]
pub enum ClauseSource<'db> {
    Environment,
    LocalImpl(crate::local_syms::impls::LocalImplSym<'db>),
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, AllocStashData)]
pub struct Clause<'db> {
    pub head: WherePredicate<'db>,
    pub body: Slice<Goal<'db>>,
    pub source: ClauseSource<'db>,
}

pub type ClauseBinder<'db> = Binder<'db, Clause<'db>>;

/// Canonical solver query. The stashed data contains every semantic input,
/// including the selected local crate and environment completeness.
#[salsa::interned(debug)]
pub struct InternedGoalQuery<'db> {
    #[returns(ref)]
    pub data: Stashed<GoalQueryData<'db>>,
}

pub type GoalQuery<'db> = InternedGoalQuery<'db>;

pub(crate) fn validate_goal_query<'db>(
    db: &'db dyn crate::Db,
    query: &Stashed<GoalQueryData<'db>>,
) -> Result<(), &'static str> {
    let (stash, data) = query.open();
    let mut visible = FxHashSet::default();
    for info in &stash[data.canonical_vars] {
        if !visible.insert(info.param) {
            return Err("duplicate canonical variable");
        }
        if info.role == CanonicalVarRole::ExistentialInput && info.kind != GenericParamKind::Type {
            return Err("non-type existential input");
        }
        if info.relative_universe > data.canonical_universe {
            return Err("canonical input is newer than the query universe");
        }
        if info.param.kind(db) != info.kind || info.param.index(db) >= data.next_response_param {
            return Err("inconsistent canonical variable metadata");
        }
    }
    validate_assumptions(
        db,
        stash,
        data.assumptions,
        &mut visible,
        data.next_response_param,
    )?;
    validate_solver_goal(db, stash, data.goal, &mut visible, data.next_response_param)
}

fn validate_solver_goal<'db>(
    db: &'db dyn crate::Db,
    stash: &Stash,
    goal: SolverGoal<'db>,
    visible: &mut FxHashSet<AlphaEquivParam<'db>>,
    next_response_param: u32,
) -> Result<(), &'static str> {
    match goal {
        SolverGoal::Prove(goal) => validate_goal(db, stash, goal, visible, next_response_param),
        SolverGoal::Normalize(alias) => validate_alias(stash, alias, visible),
    }
}

fn validate_goal<'db>(
    db: &'db dyn crate::Db,
    stash: &Stash,
    goal: Goal<'db>,
    visible: &mut FxHashSet<AlphaEquivParam<'db>>,
    next_response_param: u32,
) -> Result<(), &'static str> {
    match goal {
        Goal::Exists(binder) => {
            let parameters = stash[binder.generics].to_vec();
            let mut inserted = Vec::new();
            for parameter in parameters {
                let GenericParam::AlphaEquiv(parameter) = parameter else {
                    return Err("noncanonical existential binder");
                };
                if parameter.kind(db) != GenericParamKind::Type
                    || parameter.index(db) >= next_response_param
                    || !visible.insert(parameter)
                {
                    return Err("invalid existential binder parameter");
                }
                inserted.push(parameter);
            }
            validate_goal(db, stash, stash[binder.value], visible, next_response_param)?;
            for parameter in inserted {
                visible.remove(&parameter);
            }
            Ok(())
        }
        Goal::Implies(assumptions, inner) => {
            validate_assumptions(db, stash, assumptions, visible, next_response_param)?;
            validate_goal(db, stash, stash[inner], visible, next_response_param)
        }
        Goal::All(goals) => {
            for goal in &stash[goals] {
                validate_goal(db, stash, *goal, visible, next_response_param)?;
            }
            Ok(())
        }
        Goal::Atom(atom) => validate_atom(stash, atom, visible),
        Goal::Maybe => Ok(()),
    }
}

fn validate_assumptions<'db>(
    db: &'db dyn crate::Db,
    stash: &Stash,
    assumptions: Slice<Assumption<'db>>,
    visible: &mut FxHashSet<AlphaEquivParam<'db>>,
    next_response_param: u32,
) -> Result<(), &'static str> {
    for assumption in &stash[assumptions] {
        match *assumption {
            Assumption::TraitImpl { self_ty, trait_ref } => {
                validate_ty(stash, self_ty, visible)?;
                validate_trait_ref(stash, trait_ref, visible)?;
            }
            Assumption::NormalizesTo { alias, ty } => {
                validate_alias(stash, alias, visible)?;
                validate_ty(stash, ty, visible)?;
            }
            Assumption::Implies(conditions, consequence) => {
                for condition in &stash[conditions] {
                    validate_goal(db, stash, *condition, visible, next_response_param)?;
                }
                let consequence = stash[consequence];
                validate_ty(stash, consequence.self_ty, visible)?;
                validate_trait_ref(stash, consequence.trait_ref, visible)?;
            }
            Assumption::All(items) => {
                validate_assumptions(db, stash, items, visible, next_response_param)?;
            }
        }
    }
    Ok(())
}

fn validate_atom<'db>(
    stash: &Stash,
    atom: Atom<'db>,
    visible: &FxHashSet<AlphaEquivParam<'db>>,
) -> Result<(), &'static str> {
    match atom {
        Atom::TraitImpl { self_ty, trait_ref } => {
            validate_ty(stash, self_ty, visible)?;
            validate_trait_ref(stash, trait_ref, visible)
        }
        Atom::Equals(left, right) => {
            validate_ty(stash, left, visible)?;
            validate_ty(stash, right, visible)
        }
    }
}

fn validate_trait_ref<'db>(
    stash: &Stash,
    trait_ref: TraitRef<'db>,
    visible: &FxHashSet<AlphaEquivParam<'db>>,
) -> Result<(), &'static str> {
    for argument in &stash[trait_ref.args] {
        validate_ty(stash, *argument, visible)?;
    }
    Ok(())
}

fn validate_ty<'db>(
    stash: &Stash,
    ty: Ptr<Ty<'db>>,
    visible: &FxHashSet<AlphaEquivParam<'db>>,
) -> Result<(), &'static str> {
    match stash[ty] {
        Ty::InferVar(_) => return Err("canonical query contains a caller inference variable"),
        Ty::Param(GenericParam::AlphaEquiv(parameter)) if visible.contains(&parameter) => {}
        Ty::Param(_) => return Err("canonical query contains a noncanonical parameter"),
        Ty::Array(_, crate::ty::Const::Param(GenericParam::AlphaEquiv(parameter)))
            if !visible.contains(&parameter) =>
        {
            return Err("canonical query contains an unbound const parameter");
        }
        Ty::Array(_, crate::ty::Const::Param(_)) => {
            return Err("canonical query contains a noncanonical const parameter");
        }
        Ty::Bool
        | Ty::Char
        | Ty::Int(_)
        | Ty::Uint(_)
        | Ty::Float(_)
        | Ty::Str
        | Ty::Adt(_, _)
        | Ty::Alias(_)
        | Ty::Ref(_, _, crate::ty::Lifetime::Dummy)
        | Ty::Tuple(_)
        | Ty::Slice(_)
        | Ty::Array(_, crate::ty::Const::Literal(_) | crate::ty::Const::Other(_))
        | Ty::FnPtr(_, _)
        | Ty::Never
        | Ty::Error(_) => {}
    }
    for child in crate::check::infer::skeleton::decompose(stash, ty).children {
        validate_ty(stash, child, visible)?;
    }
    Ok(())
}

fn validate_alias<'db>(
    stash: &Stash,
    alias: AliasTy<'db>,
    visible: &FxHashSet<AlphaEquivParam<'db>>,
) -> Result<(), &'static str> {
    match alias {
        AliasTy::Named(alias) => {
            for ty in &stash[alias.args] {
                validate_ty(stash, *ty, visible)?;
            }
        }
        AliasTy::Associated(projection) => {
            validate_ty(stash, projection.self_ty, visible)?;
            validate_trait_ref(stash, projection.trait_ref, visible)?;
            for ty in &stash[projection.args] {
                validate_ty(stash, *ty, visible)?;
            }
        }
        AliasTy::Opaque(alias) => {
            for ty in &stash[alias.args] {
                validate_ty(stash, *ty, visible)?;
            }
        }
    }
    Ok(())
}

// ANCHOR: example_goal_query
#[salsa::tracked]
impl<'db> InternedGoalQuery<'db> {
    #[salsa::tracked]
    pub fn prove(self, db: &'db dyn crate::Db) -> Stashed<super::QueryResult<'db>> {
        super::prove_query(db, self.data(db))
    }

    #[salsa::tracked]
    pub fn solve(self, db: &'db dyn crate::Db) -> Stashed<super::QueryResult<'db>> {
        super::solve_query(db, self.data(db))
    }
}
// ANCHOR_END: example_goal_query

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn conjunctions_are_flattened_and_empty_is_true() {
        let mut stash = Stash::new();
        let empty = Goal::true_(&mut stash);
        let maybe = Goal::Maybe;
        let nested = Goal::all(&mut stash, [empty, maybe]);
        let all = Goal::all(&mut stash, [nested, Goal::Maybe]);

        let Goal::All(goals) = all else {
            panic!("expected conjunction");
        };
        assert_eq!(stash[goals], [Goal::Maybe, Goal::Maybe]);
    }
}
