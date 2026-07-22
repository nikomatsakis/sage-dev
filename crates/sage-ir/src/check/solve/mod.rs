//! Positive, inductive, type-only trait solving.

mod anti_unify;
mod boundary;
mod canonical;
mod clauses;
mod goal;
mod merge;
mod prove;
mod result;

pub(crate) use anti_unify::merge_hints;

pub(crate) use boundary::IrCopier;
pub use boundary::{
    AppliedCertainty, AppliedResponse, ApplyResponseError, InputInstance, QueryProofState,
    apply_query_response, extract_query_result, extract_query_result_with_output,
    instantiate_query,
};
pub use canonical::{
    CallerCanonicalVar, CanonicalMapping, CanonicalizedGoal, canonicalize_goal,
    canonicalize_solver_goal,
};
pub use goal::{
    Assumption, Atom, CanonicalVarInfo, CanonicalVarRole, Clause, ClauseBinder, ClauseSource, Goal,
    GoalQuery, GoalQueryData, MAX_PROOF_DEPTH, SolverGoal,
};
pub(crate) use prove::{prove_query, solve_query};
pub use result::{
    GoalOutput, GoalResult, QueryResult, QueryResultData, ResponseVarInfo, Subst, SubstEntry,
    SubstitutionError, validate_and_alloc_subst,
};
