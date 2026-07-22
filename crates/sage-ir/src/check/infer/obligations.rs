use sage_stash::{Slice, Stashed};

use crate::check::solve::{Assumption, CanonicalMapping, Goal, GoalQueryData};
use crate::scope::LocalCrateSymbol;
use crate::span::RelativeSpan;
use crate::ty::{CheckedParameterEnv, InferVarIndex};

/// Why a body-checking obligation was introduced.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub enum ObligationReason {
    Explicit,
    FunctionCall,
    AdtWellFormedness,
}

impl ObligationReason {
    pub fn description(self) -> &'static str {
        match self {
            Self::Explicit => "required trait bound",
            Self::FunctionCall => "bound required by this function use",
            Self::AdtWellFormedness => "bound required by this type use",
        }
    }
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub struct ObligationProvenance {
    pub span: RelativeSpan,
    pub reason: ObligationReason,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub(crate) enum ObligationState {
    Ready,
    Stalled,
    Terminal,
}

/// One retained caller-side proof request.
pub(crate) struct Obligation<'db> {
    pub goal: Goal<'db>,
    pub assumptions: Slice<Assumption<'db>>,
    pub assumptions_complete: bool,
    pub local_crate: LocalCrateSymbol<'db>,
    pub canonical_goal: Stashed<GoalQueryData<'db>>,
    pub mapping: CanonicalMapping<'db>,
    pub provenance: Vec<ObligationProvenance>,
    pub state: ObligationState,
    pub stalled_on: Vec<InferVarIndex>,
    pub last_attempted_revision: Option<u64>,
}

#[derive(Default)]
pub(crate) struct ObligationManager<'db> {
    pub obligations: Vec<Obligation<'db>>,
}

impl<'db> ObligationManager<'db> {
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
