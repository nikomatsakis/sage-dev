//! Runtime checking over explicit architectural operation boundaries.
//!
//! Callers supply balanced events; inert Rust attributes alone do not produce
//! events. Callback failures are retained as data, so enforcement need not panic
//! inside a query engine's lifecycle hooks.
//! Prohibitions cover observed requests and internal executions. Reusing a memo
//! does not replay its historical dependency graph through this checker.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use crate::{Diagnostic, Document, MethodKind, Span};

/// A permission or prohibition's location in the loaded model.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
pub struct ContractRule {
    pub file: String,
    pub span: Span,
    pub operation: String,
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
pub struct CallPermission {
    /// Includes conservative virtual dispatch, resolved by the model validator.
    pub targets: BTreeSet<String>,
    pub same_receiver: bool,
    pub rule: ContractRule,
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
pub struct OperationContract {
    pub id: String,
    pub kind: MethodKind,
    pub is_abstract: bool,
    pub invokes: Vec<CallPermission>,
    /// Includes prohibitions inherited by overriding implementations.
    pub prohibits: Vec<CallPermission>,
    /// An abstract dispatch frame may enter a known override on the same receiver.
    pub dispatches: BTreeSet<String>,
    pub unchecked_effects: BTreeSet<UncheckedEffect>,
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, serde::Serialize)]
pub enum UncheckedEffect {
    Reads,
    Creates,
}

/// Immutable, owned contracts: later AST or source edits cannot alter a session.
#[derive(Clone, Debug, serde::Serialize)]
pub struct ContractModel {
    operations: BTreeMap<String, OperationContract>,
}

impl ContractModel {
    pub fn new(documents: &[Document]) -> Result<Self, Vec<Diagnostic>> {
        crate::validate::resolve_contracts(documents).map(|operations| Self { operations })
    }

    pub fn operations(&self) -> &BTreeMap<String, OperationContract> {
        &self.operations
    }
}

/// IDs are monotonic within a session, including failed and recursive entries.
pub type InvocationId = u64;

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
pub enum EntryKind {
    Request,
    /// Query-engine reexecution establishes context, not an observed source call.
    Validation,
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
pub struct Invocation {
    pub id: InvocationId,
    pub parent: Option<InvocationId>,
    pub operation: String,
    /// Opaque semantic identity; adapters must namespace IDs across identity families.
    pub receiver: Option<String>,
    pub kind: EntryKind,
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
pub enum ContractEvent {
    Enter(Invocation),
    Exit { invocation: InvocationId },
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
pub enum ViolationKind {
    UnknownOperation,
    UnexpectedRoot,
    UndeclaredCall,
    ReceiverMismatch,
    ForbiddenTransitive,
    UnbalancedExit,
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
pub struct ContractViolation {
    pub kind: ViolationKind,
    pub invocation: InvocationId,
    pub message: String,
    /// Includes the offending invocation, so receiver distinctions survive reporting.
    pub path: Vec<Invocation>,
    pub rule: Option<ContractRule>,
}

/// Coverage of observed calls is separate from observing all implementation effects.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
pub struct ContractCoverage {
    pub checked_requests: usize,
    pub validation_contexts: usize,
    pub gaps: Vec<String>,
    pub unchecked_effects: BTreeSet<UncheckedEffect>,
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
pub struct ContractReport {
    pub trace: Vec<ContractEvent>,
    pub violations: Vec<ContractViolation>,
    pub coverage: ContractCoverage,
    /// All supplied operation boundaries were balanced, known, and permitted.
    /// This does not assert that uninstrumented calls, reads, or creates were observed.
    pub calls_conform: bool,
}

impl ContractReport {
    /// Fail outside callbacks, after retaining all violations in this report.
    pub fn assert_calls_conform(&self) {
        assert!(
            self.calls_conform,
            "architectural call checking failed: violations={:?}; gaps={:?}",
            self.violations, self.coverage.gaps
        );
    }
}

/// One sequential architectural execution; independent threads/tasks use separate
/// sessions or an adapter which explicitly propagates their active context.
pub struct ContractSession {
    model: Arc<ContractModel>,
    roots: BTreeSet<String>,
    active: Vec<Invocation>,
    trace: Vec<ContractEvent>,
    violations: Vec<ContractViolation>,
    coverage: ContractCoverage,
    next_id: InvocationId,
}

impl ContractSession {
    // {anchor}`meta.contracts.validated_session`
    pub fn new(model: Arc<ContractModel>, roots: impl IntoIterator<Item = String>) -> Self {
        let roots: BTreeSet<_> = roots.into_iter().collect();
        let gaps = roots
            .iter()
            .filter(|root| !model.operations.contains_key(*root))
            .map(|root| format!("configured root operation does not exist: {root}"))
            .collect();
        Self {
            model,
            roots,
            active: Vec::new(),
            trace: Vec::new(),
            violations: Vec::new(),
            coverage: ContractCoverage {
                checked_requests: 0,
                validation_contexts: 0,
                gaps,
                unchecked_effects: BTreeSet::new(),
            },
            next_id: 1,
        }
    }

    /// Record a source request, whether its result will be computed or reused.
    pub fn enter(&mut self, operation: &str, receiver: Option<&str>) -> InvocationId {
        self.enter_with_kind(operation, receiver, EntryKind::Request)
    }

    /// Establish the body context of an internal reexecution. It is not an
    /// architectural call by the suspended parent, but the observed execution
    /// must respect active ancestors' prohibitions. Calls from this body pass
    /// through `enter` and are checked against the executing operation.
    pub fn enter_validation(&mut self, operation: &str, receiver: Option<&str>) -> InvocationId {
        self.enter_with_kind(operation, receiver, EntryKind::Validation)
    }

    fn enter_with_kind(
        &mut self,
        operation: &str,
        receiver: Option<&str>,
        kind: EntryKind,
    ) -> InvocationId {
        let invocation = Invocation {
            id: self.next_id,
            parent: self.active.last().map(|caller| caller.id),
            operation: operation.to_owned(),
            receiver: receiver.map(str::to_owned),
            kind,
        };
        self.next_id += 1;
        match self.model.operations.get(operation) {
            Some(contract) => self
                .coverage
                .unchecked_effects
                .extend(contract.unchecked_effects.iter().cloned()),
            None => self.violation(
                ViolationKind::UnknownOperation,
                &invocation,
                format!("unknown architectural operation {operation}"),
                None,
            ),
        }
        match invocation.kind {
            EntryKind::Request => {
                self.coverage.checked_requests += 1;
                self.check_request(&invocation);
            }
            EntryKind::Validation => self.coverage.validation_contexts += 1,
        }
        self.check_prohibitions(&invocation);
        self.trace.push(ContractEvent::Enter(invocation.clone()));
        let id = invocation.id;
        self.active.push(invocation);
        id
    }

    // {anchor}`meta.language.calls.declared`
    fn check_request(&mut self, invocation: &Invocation) {
        if let Some(caller) = self.active.last().cloned() {
            if let Some(contract) = self.model.operations.get(&caller.operation) {
                // {anchor}`meta.language.envelopes.permissions`
                let permissions: Vec<_> = contract
                    .invokes
                    .iter()
                    .filter(|permission| permission.targets.contains(&invocation.operation))
                    .cloned()
                    .collect();
                let dispatch = contract.dispatches.contains(&invocation.operation);
                let unrestricted = permissions
                    .iter()
                    .any(|permission| !permission.same_receiver);
                if !unrestricted {
                    if !permissions.is_empty() || dispatch {
                        match (&caller.receiver, &invocation.receiver) {
                            (Some(a), Some(b)) if a == b => (),
                            (Some(_), Some(_)) => self.violation(
                                ViolationKind::ReceiverMismatch,
                                invocation,
                                format!(
                                    "{} may invoke {} only on the same receiver",
                                    caller.operation, invocation.operation
                                ),
                                permissions
                                    .first()
                                    .map(|permission| permission.rule.clone()),
                            ),
                            _ => self.record_gap(format!(
                                "receiver identity required for {} -> {} (invocation {})",
                                caller.operation, invocation.operation, invocation.id
                            )),
                        }
                    } else {
                        self.violation(
                            ViolationKind::UndeclaredCall,
                            invocation,
                            format!(
                                "undeclared call: {} -> {}",
                                caller.operation, invocation.operation
                            ),
                            None,
                        );
                    }
                }
            }
        } else if !self.roots.contains(&invocation.operation) {
            self.violation(
                ViolationKind::UnexpectedRoot,
                invocation,
                format!(
                    "operation is not an allowed session root: {}",
                    invocation.operation
                ),
                None,
            );
        }
    }

    // {anchor}`meta.language.calls.forbidden`
    fn check_prohibitions(&mut self, invocation: &Invocation) {
        // Restrictions apply to execution reached through incremental validation
        // as well as source requests, regardless of direct-call permissions.
        let prohibitions: Vec<_> = self
            .active
            .iter()
            .flat_map(|ancestor| {
                self.model
                    .operations
                    .get(&ancestor.operation)
                    .into_iter()
                    .flat_map(move |contract| {
                        contract
                            .prohibits
                            .iter()
                            .filter(|rule| rule.targets.contains(&invocation.operation))
                            .map(move |rule| (ancestor.clone(), rule.clone()))
                    })
            })
            .collect();
        for (ancestor, prohibition) in prohibitions {
            if prohibition.same_receiver {
                match (&ancestor.receiver, &invocation.receiver) {
                    (Some(a), Some(b)) if a != b => continue,
                    (Some(_), Some(_)) => (),
                    _ => {
                        self.record_gap(format!(
                            "receiver identity required to check prohibition from {} on {} (invocation {})",
                            ancestor.operation, invocation.operation, invocation.id
                        ));
                        continue;
                    }
                }
            }
            self.violation(
                ViolationKind::ForbiddenTransitive,
                invocation,
                format!(
                    "{} forbids transitive {} of {}",
                    prohibition.rule.operation,
                    match invocation.kind {
                        EntryKind::Request => "request",
                        EntryKind::Validation => "execution",
                    },
                    invocation.operation
                ),
                Some(prohibition.rule),
            );
        }
    }

    fn violation(
        &mut self,
        kind: ViolationKind,
        invocation: &Invocation,
        message: String,
        rule: Option<ContractRule>,
    ) {
        let mut path = self.active.clone();
        path.push(invocation.clone());
        self.violations.push(ContractViolation {
            kind,
            invocation: invocation.id,
            message,
            path,
            rule,
        });
    }

    /// An out-of-order exit is retained as a violation. It never clears an
    /// unrelated frame or makes a malformed trace appear balanced.
    pub fn exit(&mut self, invocation: InvocationId) {
        if self
            .active
            .last()
            .is_some_and(|frame| frame.id == invocation)
        {
            self.active.pop();
            self.trace.push(ContractEvent::Exit { invocation });
        } else {
            self.violations.push(ContractViolation {
                kind: ViolationKind::UnbalancedExit,
                invocation,
                message: format!("exit for invocation {invocation} does not match active frame"),
                path: self.active.clone(),
                rule: None,
            });
        }
    }

    /// Adapters must record every unmapped/unsupported observed boundary here.
    pub fn record_gap(&mut self, message: impl Into<String>) {
        self.coverage.gaps.push(message.into());
    }

    pub fn trace(&self) -> &[ContractEvent] {
        &self.trace
    }

    pub fn violations(&self) -> &[ContractViolation] {
        &self.violations
    }

    // {anchor}`meta.contracts.observation_coverage`
    pub fn finish(mut self) -> ContractReport {
        if !self.active.is_empty() {
            self.record_gap(format!(
                "unclosed architectural invocations: {}",
                self.active
                    .iter()
                    .map(|frame| format!("{} ({})", frame.operation, frame.id))
                    .collect::<Vec<_>>()
                    .join(", ")
            ));
        }
        ContractReport {
            calls_conform: self.violations.is_empty() && self.coverage.gaps.is_empty(),
            trace: self.trace,
            violations: self.violations,
            coverage: self.coverage,
        }
    }
}
