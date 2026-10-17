use sage_stash::{Slice, Stashed};

use crate::check::solve::{Assumption, CanonicalMapping, Goal, GoalQueryData};
use crate::scope::LocalCrateSymbol;
use crate::span::RelativeSpan;
use crate::ty::{AliasTy, CheckedParameterEnv, InferVarIndex, Ty};
use crate::tytree::ExprSlot;
use sage_stash::Ptr;

/// Why a body-checking obligation was introduced.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub enum ObligationReason {
    Explicit,
    FunctionCall,
    AdtWellFormedness,
    BinaryOperator,
}

impl ObligationReason {
    pub fn description(self) -> &'static str {
        match self {
            Self::Explicit => "required trait bound",
            Self::FunctionCall => "bound required by this function use",
            Self::AdtWellFormedness => "bound required by this type use",
            Self::BinaryOperator => "required by this binary operator",
        }
    }
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub struct ObligationProvenance {
    pub span: RelativeSpan,
    pub reason: ObligationReason,
}

/// A transient expression which becomes an error node if its obligation fails.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ObligationRecovery<'db> {
    pub slot: ExprSlot,
    pub span: RelativeSpan,
    /// Existing errors in these types also invalidate the pending expression
    /// without producing another diagnostic.
    pub watched_types: Vec<Ptr<Ty<'db>>>,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub(crate) enum ObligationState {
    Ready,
    Stalled,
    Terminal,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub(crate) enum ObligationGoal<'db> {
    Prove(Goal<'db>),
    Normalize {
        alias: AliasTy<'db>,
        expected: Ptr<Ty<'db>>,
    },
}

/// One retained caller-side proof request.
pub(crate) struct Obligation<'db> {
    pub goal: ObligationGoal<'db>,
    pub assumptions: Slice<Assumption<'db>>,
    pub assumptions_complete: bool,
    pub local_crate: LocalCrateSymbol<'db>,
    pub canonical_goal: Stashed<GoalQueryData<'db>>,
    pub mapping: CanonicalMapping<'db>,
    pub provenance: Vec<ObligationProvenance>,
    pub recoveries: Vec<ObligationRecovery<'db>>,
    pub state: ObligationState,
    pub stalled_on: Vec<InferVarIndex>,
    pub last_attempted_revision: Option<u64>,
}

impl Obligation<'_> {
    /// Whether two pending proof requests may share scheduling and recovery.
    ///
    /// Recovery scope is part of the local identity even though it is absent
    /// from the canonical solver goal. Otherwise an error in one expression
    /// could recover another expression that happens to require the same
    /// proof.
    // {anchor}`crates.sage_ir.checking.body.operator_calls`
    pub fn same_dedup_key(&self, other: &Self) -> bool {
        matches!(self.goal, ObligationGoal::Prove(_))
            && matches!(other.goal, ObligationGoal::Prove(_))
            && self.recoveries == other.recoveries
            && self.canonical_goal == other.canonical_goal
            && self.mapping == other.mapping
    }
}

#[derive(Default)]
pub(crate) struct ObligationManager<'db> {
    pub obligations: Vec<Obligation<'db>>,
}

// {anchor}`crates.sage_ir.inference.terminal_obligations`
impl<'db> ObligationManager<'db> {
    // {anchor}`crates.sage_ir.inference.body_local_quiescence`
    pub fn has_pending(&self) -> bool {
        self.obligations
            .iter()
            .any(|obligation| obligation.state != ObligationState::Terminal)
    }

    pub fn ready_indices(&self) -> Vec<usize> {
        self.obligations
            .iter()
            .enumerate()
            .filter_map(|(index, obligation)| {
                (obligation.state == ObligationState::Ready).then_some(index)
            })
            .collect()
    }

    pub fn wake(&mut self, variables: &[InferVarIndex]) {
        if variables.is_empty() {
            return;
        }
        for obligation in &mut self.obligations {
            if obligation.state == ObligationState::Stalled
                && obligation
                    .stalled_on
                    .iter()
                    .any(|variable| variables.contains(variable))
            {
                obligation.state = ObligationState::Ready;
            }
        }
    }

    pub fn pending_indices(&self) -> Vec<usize> {
        self.obligations
            .iter()
            .enumerate()
            .filter_map(|(index, obligation)| {
                (obligation.state != ObligationState::Terminal).then_some(index)
            })
            .collect()
    }
}

/// Obligations staged by an inference transaction. Dropping the batch is a
/// rollback; the caller publishes it only after committing the transaction.
#[derive(Default)]
// {anchor}`crates.sage_ir.inference.atomic_checking`
pub struct StagedObligationBatch<'db> {
    pub(crate) environments: Vec<(CheckedParameterEnv<'db>, ObligationProvenance)>,
}

impl<'db> StagedObligationBatch<'db> {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn push_parameter_env(
        &mut self,
        environment: CheckedParameterEnv<'db>,
        span: RelativeSpan,
        reason: ObligationReason,
    ) {
        self.environments
            .push((environment, ObligationProvenance { span, reason }));
    }

    pub fn is_empty(&self) -> bool {
        self.environments.is_empty()
    }
}
