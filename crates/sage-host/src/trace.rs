// {anchor}`crates.sage_test_harness.compile_tests.query_projection`
use sage_inspector::*;

pub fn build_dynamic_trace(
    events: Vec<sage_ir::db::InspectionEvent>,
    default_phase: TracePhase,
) -> Vec<TraceNode> {
    let mut roots = Vec::new();
    let mut stack: Vec<TraceNode> = Vec::new();
    for event in events {
        match event {
            sage_ir::db::InspectionEvent::PhaseEnter { phase } => stack.push(TraceNode {
                phase: trace_phase(phase).unwrap_or(default_phase),
                source: TraceSource::Sage,
                operation: phase.to_owned(),
                key: TraceKey::Semantic {
                    value: phase.to_owned(),
                },
                disposition: TraceDisposition::Observed,
                child_order: ChildOrder::Sequential,
                observations: 1,
                children: vec![],
            }),
            sage_ir::db::InspectionEvent::PhaseExit { phase } => {
                let Some(mut node) = stack.pop() else {
                    roots.push(unbalanced_trace_event(format!(
                        "phase exit without enter: {phase}"
                    )));
                    continue;
                };
                if node.operation != phase {
                    roots.push(unbalanced_trace_event(format!(
                        "phase exit `{phase}` did not match `{}`",
                        node.operation
                    )));
                }
                normalize_unordered_children(&mut node);
                if let Some(parent) = stack.last_mut() {
                    parent.children.push(node);
                } else {
                    roots.push(node);
                }
            }
            sage_ir::db::InspectionEvent::SpanEnter {
                operation,
                source,
                child_order,
            } => stack.push(TraceNode {
                phase: stack.last().map_or(default_phase, |parent| parent.phase),
                source: match source {
                    sage_ir::db::InspectionSource::Sage => TraceSource::Sage,
                    sage_ir::db::InspectionSource::Solver => TraceSource::Solver,
                },
                operation: operation.to_owned(),
                key: TraceKey::Semantic {
                    value: operation.to_owned(),
                },
                disposition: TraceDisposition::Observed,
                child_order: match child_order {
                    sage_ir::db::InspectionChildOrder::Sequential => ChildOrder::Sequential,
                    sage_ir::db::InspectionChildOrder::Unordered => ChildOrder::Unordered,
                },
                observations: 1,
                children: vec![],
            }),
            sage_ir::db::InspectionEvent::SpanExit { operation } => {
                let Some(mut node) = stack.pop() else {
                    roots.push(unbalanced_trace_event(format!(
                        "semantic span exit without enter: {operation}"
                    )));
                    continue;
                };
                if node.operation != operation {
                    roots.push(unbalanced_trace_event(format!(
                        "semantic span exit `{operation}` did not match `{}`",
                        node.operation
                    )));
                }
                normalize_unordered_children(&mut node);
                attach_trace_node(&mut roots, &mut stack, node);
            }
            sage_ir::db::InspectionEvent::QueryEnter { key } => {
                stack.push(TraceNode {
                    phase: stack.last().map_or(default_phase, |parent| parent.phase),
                    source: TraceSource::Salsa,
                    operation: query_operation(&key),
                    key: TraceKey::Unmapped { ingredient: key },
                    disposition: TraceDisposition::Observed,
                    child_order: ChildOrder::Sequential,
                    observations: 1,
                    children: vec![],
                });
            }
            sage_ir::db::InspectionEvent::QueryExit { key, disposition } => {
                let Some(mut node) = stack.pop() else {
                    roots.push(unbalanced_trace_event(format!("exit without enter: {key}")));
                    continue;
                };
                if !matches!(&node.key, TraceKey::Unmapped { ingredient } if ingredient == &key) {
                    roots.push(unbalanced_trace_event(format!(
                        "query exit `{key}` did not match the active span"
                    )));
                }
                node.disposition = trace_disposition(disposition);
                normalize_unordered_children(&mut node);
                attach_trace_node(&mut roots, &mut stack, node);
            }
            sage_ir::db::InspectionEvent::QueryLeaf {
                key,
                disposition,
                observations,
            } => {
                let node = TraceNode {
                    phase: stack.last().map_or(default_phase, |parent| parent.phase),
                    source: TraceSource::Salsa,
                    operation: query_operation(&key),
                    key: TraceKey::Unmapped { ingredient: key },
                    disposition: trace_disposition(disposition),
                    child_order: ChildOrder::Sequential,
                    observations,
                    children: vec![],
                };
                attach_trace_node(&mut roots, &mut stack, node);
            }
            sage_ir::db::InspectionEvent::ExternalMetadata { operation } => {
                let node = TraceNode {
                    phase: stack.last().map_or(default_phase, |parent| parent.phase),
                    source: TraceSource::ExternalMetadata,
                    operation: operation
                        .split_once('(')
                        .map_or_else(|| operation.clone(), |(name, _)| name.to_owned()),
                    key: TraceKey::Unmapped {
                        ingredient: operation,
                    },
                    disposition: TraceDisposition::Executed,
                    child_order: ChildOrder::Sequential,
                    observations: 1,
                    children: vec![],
                };
                attach_trace_node(&mut roots, &mut stack, node);
            }
        }
    }
    while let Some(mut node) = stack.pop() {
        node.disposition = TraceDisposition::Observed;
        attach_trace_node(&mut roots, &mut stack, node);
    }
    roots
}

fn attach_trace_node(roots: &mut Vec<TraceNode>, stack: &mut [TraceNode], node: TraceNode) {
    if let Some(parent) = stack.last_mut() {
        parent.children.push(node);
    } else {
        roots.push(node);
    }
}

fn trace_disposition(disposition: salsa::QueryDisposition) -> TraceDisposition {
    match disposition {
        salsa::QueryDisposition::Executed => TraceDisposition::Executed,
        salsa::QueryDisposition::Validated => TraceDisposition::Validated,
        salsa::QueryDisposition::Reused => TraceDisposition::Reused,
        salsa::QueryDisposition::Cancelled => TraceDisposition::Cancelled,
    }
}

fn trace_phase(phase: &str) -> Option<TracePhase> {
    match phase {
        "bootstrap" => Some(TracePhase::Bootstrap),
        "selection" => Some(TracePhase::Selection),
        "analysis" => Some(TracePhase::Analysis),
        "reflection" => Some(TracePhase::Reflection),
        "view-assembly" => Some(TracePhase::ViewAssembly),
        _ => None,
    }
}

fn normalize_unordered_children(node: &mut TraceNode) {
    if node.child_order != ChildOrder::Unordered {
        return;
    }
    node.children.sort_by_cached_key(trace_sort_key);
}

// {anchor}`crates.sage_inspector.inspection.deterministic_trace`
fn trace_sort_key(node: &TraceNode) -> String {
    serde_json::to_string(node).expect("trace nodes always serialize")
}

fn query_operation(key: &str) -> String {
    key.split_once('(')
        .map_or(key, |(operation, _)| operation)
        .trim()
        .to_owned()
}

fn unbalanced_trace_event(message: String) -> TraceNode {
    TraceNode {
        phase: TracePhase::Analysis,
        source: TraceSource::Sage,
        operation: "trace-recorder-error".to_owned(),
        key: TraceKey::Unmapped {
            ingredient: message,
        },
        disposition: TraceDisposition::Observed,
        child_order: ChildOrder::Sequential,
        observations: 1,
        children: vec![],
    }
}

/// Stable snapshot form of the Inspector projection. Unknown keys remain
/// explicitly unmapped, but their allocation-dependent debug arguments are not
/// identities. Preserve operation, parentage, multiplicity and disposition.
pub fn snapshot_trace(events: Vec<sage_ir::db::InspectionEvent>) -> Vec<TraceNode> {
    fn normalize(node: &mut TraceNode) {
        if let TraceKey::Unmapped { ingredient } = &mut node.key {
            *ingredient = node.operation.clone();
        }
        for child in &mut node.children {
            normalize(child);
        }
        normalize_unordered_children(node);
    }
    let mut nodes = build_dynamic_trace(events, TracePhase::Analysis);
    for node in &mut nodes {
        normalize(node);
    }
    nodes
}
