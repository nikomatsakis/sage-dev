use std::collections::HashMap;

use sage_inspector::actor::{InspectionProvider, Provided};
use sage_inspector::*;
use sage_ir::Db;
use sage_ir::derive::DeriveExpansion;
use sage_ir::external_syms::{
    external_adt_signature, external_fn_signature, external_trait_items, external_trait_signature,
};
use sage_ir::generic_param::{AlphaEquivParam, AstGenericParam, ExtGenericParam};
use sage_ir::local_syms::LocalAssociatedOwner;
use sage_ir::local_syms::associated::local_associated_items;
use sage_ir::local_syms::fns::LocalFnSym;
use sage_ir::local_syms::mods::module_expansion_complete_for_symbol_listing;
use sage_ir::name::Name;
use sage_ir::source::SourceFile;
use sage_ir::span::{AbsoluteSpan, ParseSource};
use sage_ir::symbol::{
    ConstSymbol, DefIndex, EnumSymbol, FnSymbol, ImplSymbol, MacroDefSymbol, ModSymbol,
    StaticSymbol, StructSymbol, SymExt, SymExtKind, Symbol, SymbolData, TraitSymbol,
    TypeAliasSymbol, UseSymbol, VariantCtorSymbol, VariantSymbol,
};
use sage_ir::types::TokenTree;
use sage_reflect::{
    Badge, BadgeTone, ReferenceKey, Reflect, ReflectionContext, ReflectionResolver,
    SymbolPresentation, SymbolReference, ValueField, ValueNode,
};
use salsa::plumbing::FromId;

use crate::driver::{SageContext, SageHost};

const RETAINED_REVISION_LIMIT: usize = 64;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SourceWatchRoots {
    pub workspace_root: std::path::PathBuf,
    pub package_root: std::path::PathBuf,
    pub source_root: std::path::PathBuf,
}

// ANCHOR: semantic_inspector_live_host
pub struct LiveInspectionProvider<'host> {
    host: &'host mut SageHost,
    watch_root_observer: Option<std::sync::mpsc::Sender<SourceWatchRoots>>,
    database_generation: u64,
    next_edit_batch: u64,
    next_run: u64,
    next_continuation: u64,
    runs: HashMap<RunHandle, RunObservation>,
    continuations: HashMap<ContinuationHandle, Vec<ValueNode>>,
    revisions: Vec<RetainedRevision>,
    products: HashMap<(RevisionId, SymbolPath, ProductId), RetainedProduct>,
    directory: Option<SymbolDirectory>,
    directory_stale: bool,
}

struct RetainedRevision {
    revision_id: RevisionId,
    cause: RevisionCause,
    input_deltas: Vec<InputDelta>,
    runs: Vec<RunHandle>,
}

#[derive(Clone)]
struct RetainedProduct {
    frozen: FrozenProduct,
    run_id: Option<RunHandle>,
}

#[derive(Clone, Debug, PartialEq)]
struct FrozenProduct {
    page: ProductPage,
    continuation_pages: Vec<Vec<ValueNode>>,
}

impl<'host> LiveInspectionProvider<'host> {
    pub fn new(host: &'host mut SageHost) -> Self {
        let revision_id = format!("db0-rev{}", host.salsa_revision());
        Self {
            host,
            watch_root_observer: None,
            database_generation: 0,
            next_edit_batch: 0,
            next_run: 0,
            next_continuation: 0,
            runs: HashMap::new(),
            continuations: HashMap::new(),
            revisions: vec![RetainedRevision {
                revision_id,
                cause: RevisionCause::Initial,
                input_deltas: vec![],
                runs: vec![],
            }],
            products: HashMap::new(),
            directory: None,
            directory_stale: false,
        }
    }

    pub fn with_watch_root_observer(
        mut self,
        observer: std::sync::mpsc::Sender<SourceWatchRoots>,
    ) -> Self {
        let _ = observer.send(self.source_watch_roots());
        self.watch_root_observer = Some(observer);
        self
    }

    fn source_watch_roots(&self) -> SourceWatchRoots {
        SourceWatchRoots {
            workspace_root: self.host.workspace_root().to_path_buf(),
            package_root: self.host.package_root().to_path_buf(),
            source_root: self.host.source_root().to_path_buf(),
        }
    }

    fn revision_id(&self) -> RevisionId {
        format!(
            "db{}-rev{}",
            self.database_generation,
            self.host.salsa_revision()
        )
    }

    fn provided<T>(&self, value: T) -> Provided<T> {
        Provided::without_run(self.revision_id(), value)
    }

    fn record<T>(
        &mut self,
        request: RunRequest,
        phase: TracePhase,
        operation: &str,
        value: T,
    ) -> Provided<T> {
        let events = self.host.take_inspection_log();
        let _ = self.host.take_query_log();
        self.next_run += 1;
        let run_id = format!("run_{}", self.next_run);
        let children = build_dynamic_trace(events, phase);
        let root_key = match &request {
            RunRequest::SymbolIndex => "local-symbol-index".to_owned(),
            RunRequest::Symbol { target } | RunRequest::Product { target, .. } => target.clone(),
            RunRequest::AutomaticRefresh { resource } => resource.clone(),
        };
        self.runs.insert(
            run_id.clone(),
            RunObservation {
                run_id: run_id.clone(),
                request,
                root: TraceNode {
                    phase,
                    source: TraceSource::Sage,
                    operation: operation.to_owned(),
                    key: TraceKey::Semantic { value: root_key },
                    disposition: TraceDisposition::Observed,
                    child_order: ChildOrder::Sequential,
                    observations: 1,
                    children,
                },
            },
        );
        self.revisions
            .last_mut()
            .expect("the current revision is retained")
            .runs
            .push(run_id.clone());
        Provided {
            revision_id: self.revision_id(),
            run_id: Some(run_id),
            value,
        }
    }

    fn start_recording(&self) {
        let _ = self.host.take_query_log();
        let _ = self.host.take_inspection_log();
    }

    fn push_revision(&mut self, revision: RetainedRevision) {
        self.revisions.push(revision);
        if self.revisions.len() <= RETAINED_REVISION_LIMIT {
            return;
        }
        let expired = self.revisions.remove(0);
        for run in expired.runs {
            self.runs.remove(&run);
        }
        self.products
            .retain(|(revision, _, _), _| revision != &expired.revision_id);
        let prefix = format!("{}_", expired.revision_id);
        self.continuations
            .retain(|continuation, _| !continuation.starts_with(&prefix));
    }

    fn directory(&mut self) -> SymbolDirectory {
        if let Some(directory) = &self.directory {
            return directory.clone();
        }
        let directory = self.host.with_context(build_symbol_directory);
        self.directory = Some(directory.clone());
        directory
    }
}
// ANCHOR_END: semantic_inspector_live_host

impl InspectionProvider for LiveInspectionProvider<'_> {
    fn current_revision(&self) -> RevisionId {
        self.revision_id()
    }

    fn revision(&mut self) -> Result<Provided<()>, ApiError> {
        Ok(self.provided(()))
    }

    fn session(&mut self) -> Result<Provided<Session>, ApiError> {
        let target = self.host.target();
        Ok(self.provided(Session {
            protocol_version: "1".to_owned(),
            target: CargoTarget {
                package: self.host.package_name().to_owned(),
                target_kind: TargetKind::Lib,
                target_name: target.name.clone(),
            },
            workspace_root: self.host.workspace_root().display().to_string(),
            capabilities: vec![
                Capability::SymbolIndex,
                Capability::Products,
                Capability::Runs,
                Capability::Events,
                Capability::Revisions,
                Capability::RevisionComparison,
            ],
            retained_revisions: RetainedRevisionRange {
                first: self
                    .revisions
                    .first()
                    .map(|revision| revision.revision_id.clone()),
                last: Some(self.revision_id()),
            },
        }))
    }

    fn symbols(&mut self) -> Result<Provided<SymbolIndex>, ApiError> {
        self.start_recording();
        let directory = self.host.with_context(build_symbol_directory);
        let value = directory.index.clone();
        self.directory = Some(directory);
        self.directory_stale = false;
        Ok(self.record(
            RunRequest::SymbolIndex,
            TracePhase::Bootstrap,
            "local-symbol-index",
            value,
        ))
    }

    fn symbol(&mut self, path: &str) -> Result<Provided<SelectedSymbol>, ApiError> {
        self.start_recording();
        let path_owned = path.to_owned();
        if self.directory_stale {
            self.directory = None;
        }
        let directory = self.directory();
        self.directory_stale = false;
        let selected = self
            .host
            .with_context(|context| selected_symbol(context, &directory, &path_owned))
            .ok_or_else(|| {
                ApiError::new("symbol-not-found", format!("unknown symbol path `{path}`"))
            })?;
        Ok(self.record(
            RunRequest::Symbol {
                target: path.to_owned(),
            },
            TracePhase::Selection,
            "select-symbol",
            selected,
        ))
    }

    fn product(&mut self, symbol: &str, product: &str) -> Result<Provided<ProductPage>, ApiError> {
        self.start_recording();
        let symbol_owned = symbol.to_owned();
        let product_owned = product.to_owned();
        self.next_continuation += 1;
        let continuation_prefix =
            format!("{}_product_{}", self.revision_id(), self.next_continuation);
        let mut directory = self.directory();
        if self.directory_stale {
            // Paths and labels cached by the eager directory belong to the
            // previous revision. Keep its target summary so a direct product
            // request does not rebuild the tree, but resolve reflected links
            // from current tracked owners.
            directory.references.clear();
        }
        let generated = self
            .host
            .with_context(|context| {
                identity_product(
                    context,
                    &directory,
                    &symbol_owned,
                    &product_owned,
                    &continuation_prefix,
                )
            })
            .ok_or_else(|| {
                ApiError::new(
                    "product-not-found",
                    format!("product `{product}` is not listed for `{symbol}`"),
                )
            })?;
        let frozen = freeze_product(&generated.page, &generated.continuations);
        self.continuations.extend(generated.continuations);
        let page = generated.page;
        let provided = self.record(
            RunRequest::Product {
                target: symbol.to_owned(),
                product: product.to_owned(),
            },
            TracePhase::Analysis,
            "product",
            page,
        );
        self.products.insert(
            (self.revision_id(), symbol.to_owned(), product.to_owned()),
            RetainedProduct {
                frozen,
                run_id: provided.run_id.clone(),
            },
        );
        Ok(provided)
    }

    fn continuation(&mut self, handle: &str) -> Result<Provided<ContinuationValue>, ApiError> {
        let items = self.continuations.get(handle).cloned().ok_or_else(|| {
            ApiError::new(
                "continuation-not-found",
                format!("unknown continuation `{handle}`"),
            )
        })?;
        Ok(self.provided(ContinuationValue {
            continuation: handle.to_owned(),
            items,
            next: None,
        }))
    }

    fn run(&mut self, handle: &str) -> Result<Provided<RunObservation>, ApiError> {
        let run = self
            .runs
            .get(handle)
            .cloned()
            .ok_or_else(|| ApiError::new("run-not-found", format!("unknown run `{handle}`")))?;
        Ok(self.provided(run))
    }

    fn revisions(&mut self, _cursor: Option<&str>) -> Result<Provided<RevisionPage>, ApiError> {
        Ok(self.provided(RevisionPage {
            revisions: self.revisions.iter().rev().map(revision_summary).collect(),
            next_cursor: None,
        }))
    }

    fn revision_detail(&mut self, revision: &str) -> Result<Provided<RevisionDetail>, ApiError> {
        let revision = self
            .revisions
            .iter()
            .find(|candidate| candidate.revision_id == revision)
            .ok_or_else(|| {
                ApiError::new(
                    "revision-not-found",
                    format!("unknown revision `{revision}`"),
                )
            })?;
        Ok(self.provided(RevisionDetail {
            summary: revision_summary(revision),
            input_deltas: revision.input_deltas.clone(),
            runs: revision.runs.clone(),
        }))
    }

    fn compare(
        &mut self,
        from: &str,
        to: &str,
        symbol: &str,
        product: &str,
    ) -> Result<Provided<RunComparison>, ApiError> {
        let before = self
            .products
            .get(&(from.to_owned(), symbol.to_owned(), product.to_owned()))
            .ok_or_else(|| {
                ApiError::new(
                    "comparison-value-not-found",
                    "the product was not inspected in the starting revision",
                )
            })?;
        let after = self
            .products
            .get(&(to.to_owned(), symbol.to_owned(), product.to_owned()))
            .ok_or_else(|| {
                ApiError::new(
                    "comparison-value-not-found",
                    "the product was not inspected in the ending revision",
                )
            })?;
        let before_trace = before
            .run_id
            .as_ref()
            .and_then(|run| self.runs.get(run))
            .map(trace_identities)
            .unwrap_or_default();
        let after_trace = after
            .run_id
            .as_ref()
            .and_then(|run| self.runs.get(run))
            .map(trace_identities)
            .unwrap_or_default();
        Ok(self.provided(RunComparison {
            from_revision: from.to_owned(),
            to_revision: to.to_owned(),
            symbol: symbol.to_owned(),
            product: product.to_owned(),
            value_changed: before.frozen != after.frozen,
            executed_only_before: difference(&before_trace.0, &after_trace.0),
            executed_only_after: difference(&after_trace.0, &before_trace.0),
            reused_only_before: difference(&before_trace.1, &after_trace.1),
            reused_only_after: difference(&after_trace.1, &before_trace.1),
        }))
    }

    fn apply_updates(
        &mut self,
        updates: Vec<FileUpdate>,
    ) -> Result<Provided<RevisionEvent>, ApiError> {
        let updates: Vec<_> = updates
            .into_iter()
            .map(|update| {
                let path = normalize_source_path(self.host.source_root(), &update.path);
                (path, update.text)
            })
            .collect();
        if let Some((path, _)) = updates
            .iter()
            .find(|(path, _)| self.host.source_text(path).is_none())
        {
            return Err(ApiError::new(
                "unrepresented-source-file",
                format!("source file `{path}` is not registered in the current host"),
            ));
        }

        let mut deltas = Vec::new();
        for (path, text) in updates {
            let old_text = self
                .host
                .source_text(&path)
                .expect("source paths were validated before applying the batch");
            if old_text == text {
                continue;
            }
            self.host
                .set_source_text(&path, text.clone())
                .map_err(|message| ApiError::new("input-update-failed", message))?;
            deltas.push(InputDelta {
                input: InputIdentity {
                    kind: InputKind::SourceFile,
                    path,
                    field: InputField::Text,
                },
                old_hash: content_hash(&old_text),
                new_hash: content_hash(&text),
                diff: line_diff(&old_text, &text),
            });
        }
        if deltas.is_empty() {
            return Err(ApiError::new(
                "no-input-changes",
                "the file notification did not change a represented source input",
            ));
        }
        self.directory_stale = true;
        self.next_edit_batch += 1;
        let edit_batch = format!("edit-{}", self.next_edit_batch);
        let revision_id = self.revision_id();
        self.push_revision(RetainedRevision {
            revision_id: revision_id.clone(),
            cause: RevisionCause::InputEdit {
                edit_batch: edit_batch.clone(),
            },
            input_deltas: deltas.clone(),
            runs: vec![],
        });
        self.start_recording();
        let event = RevisionEvent::RevisionAdvanced(RevisionAdvanced {
            revision_id,
            edit_batch,
            changed_inputs: deltas.into_iter().map(|delta| delta.input).collect(),
        });
        Ok(self.provided(event))
    }

    fn reload_workspace(&mut self, reason: Issue) -> Result<Provided<RevisionEvent>, ApiError> {
        let previous_revision_id = self.revision_id();
        self.host.reload().map_err(|message| {
            ApiError::new(
                "workspace-reload-failed",
                format!("the inspection host could not be reconstructed: {message}"),
            )
        })?;
        if let Some(observer) = &self.watch_root_observer {
            let _ = observer.send(self.source_watch_roots());
        }
        self.directory = None;
        self.directory_stale = false;
        self.database_generation += 1;
        let revision_id = self.revision_id();
        self.continuations.clear();
        self.push_revision(RetainedRevision {
            revision_id: revision_id.clone(),
            cause: RevisionCause::WorkspaceReload {
                previous_revision_id: previous_revision_id.clone(),
                reason: reason.clone(),
            },
            input_deltas: vec![],
            runs: vec![],
        });
        self.start_recording();
        Ok(
            self.provided(RevisionEvent::WorkspaceReloaded(WorkspaceReloaded {
                previous_revision_id,
                revision_id,
                reason,
            })),
        )
    }
}

fn build_dynamic_trace(
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

fn revision_summary(revision: &RetainedRevision) -> RevisionSummary {
    RevisionSummary {
        revision_id: revision.revision_id.clone(),
        cause: revision.cause.clone(),
        input_delta_count: revision.input_deltas.len() as u32,
        run_count: revision.runs.len() as u32,
    }
}

fn trace_identities(run: &RunObservation) -> (Vec<TraceIdentity>, Vec<TraceIdentity>) {
    fn visit(node: &TraceNode, executed: &mut Vec<TraceIdentity>, reused: &mut Vec<TraceIdentity>) {
        let identity = || TraceIdentity {
            source: node.source,
            operation: node.operation.clone(),
            key: node.key.clone(),
            observations: node.observations,
        };
        match node.disposition {
            TraceDisposition::Executed => executed.push(identity()),
            TraceDisposition::Reused => reused.push(identity()),
            TraceDisposition::Validated
            | TraceDisposition::Cancelled
            | TraceDisposition::Observed => {}
        }
        for child in &node.children {
            visit(child, executed, reused);
        }
    }

    let mut executed = Vec::new();
    let mut reused = Vec::new();
    visit(&run.root, &mut executed, &mut reused);
    (executed, reused)
}

fn difference(left: &[TraceIdentity], right: &[TraceIdentity]) -> Vec<TraceIdentity> {
    let mut unmatched = right.to_vec();
    left.iter()
        .filter_map(|identity| {
            let mut remaining = identity.observations;
            for candidate in &mut unmatched {
                if remaining == 0 {
                    break;
                }
                if candidate.source == identity.source
                    && candidate.operation == identity.operation
                    && candidate.key == identity.key
                {
                    let cancelled = remaining.min(candidate.observations);
                    remaining -= cancelled;
                    candidate.observations -= cancelled;
                }
            }
            (remaining > 0).then(|| TraceIdentity {
                observations: remaining,
                ..identity.clone()
            })
        })
        .collect()
}

fn freeze_product(
    page: &ProductPage,
    continuations: &HashMap<ContinuationHandle, Vec<ValueNode>>,
) -> FrozenProduct {
    let mut page = page.clone();
    let mut canonical_handles = HashMap::new();
    let mut continuation_pages = Vec::new();
    normalize_render_continuations(
        &mut page.content,
        continuations,
        &mut canonical_handles,
        &mut continuation_pages,
    );
    FrozenProduct {
        page,
        continuation_pages,
    }
}

fn normalize_render_continuations(
    node: &mut RenderNode,
    source: &HashMap<ContinuationHandle, Vec<ValueNode>>,
    canonical_handles: &mut HashMap<ContinuationHandle, usize>,
    pages: &mut Vec<Vec<ValueNode>>,
) {
    match node {
        RenderNode::Group { children, .. } => {
            for child in children {
                normalize_render_continuations(child, source, canonical_handles, pages);
            }
        }
        RenderNode::Value { value } => {
            normalize_value_continuations(value, source, canonical_handles, pages)
        }
        RenderNode::Heading { .. }
        | RenderNode::Text { .. }
        | RenderNode::Code { .. }
        | RenderNode::Notice { .. }
        | RenderNode::Navigation { .. } => {}
    }
}

fn normalize_value_continuations(
    node: &mut ValueNode,
    source: &HashMap<ContinuationHandle, Vec<ValueNode>>,
    canonical_handles: &mut HashMap<ContinuationHandle, usize>,
    pages: &mut Vec<Vec<ValueNode>>,
) {
    match node {
        ValueNode::Record { fields, .. } | ValueNode::Variant { fields, .. } => {
            for field in fields {
                normalize_value_continuations(&mut field.value, source, canonical_handles, pages);
            }
        }
        ValueNode::Sequence { items, .. } => {
            for item in items {
                normalize_value_continuations(item, source, canonical_handles, pages);
            }
        }
        ValueNode::Truncated {
            continuation: Some(handle),
            ..
        } => {
            let original = handle.clone();
            let index = if let Some(index) = canonical_handles.get(&original).copied() {
                index
            } else {
                let index = pages.len();
                canonical_handles.insert(original.clone(), index);
                pages.push(vec![]);
                let mut page = source.get(&original).cloned().unwrap_or_default();
                for value in &mut page {
                    normalize_value_continuations(value, source, canonical_handles, pages);
                }
                pages[index] = page;
                index
            };
            *handle = format!("continuation-{index}");
        }
        ValueNode::Scalar { .. } | ValueNode::Reference { .. } | ValueNode::Cycle { .. } => {}
        ValueNode::Truncated {
            continuation: None, ..
        } => {}
    }
}

fn normalize_source_path(source_root: &std::path::Path, path: &str) -> String {
    let path = std::path::Path::new(path);
    path.strip_prefix(source_root)
        .unwrap_or(path)
        .to_string_lossy()
        .replace(std::path::MAIN_SEPARATOR, "/")
}

fn content_hash(text: &str) -> String {
    let mut hash = 0xcbf2_9ce4_8422_2325_u64;
    for byte in text.bytes() {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    format!("{hash:016x}")
}

fn line_diff(before: &str, after: &str) -> String {
    let before: Vec<_> = before.lines().collect();
    let after: Vec<_> = after.lines().collect();
    let common_prefix = before
        .iter()
        .zip(&after)
        .take_while(|(before, after)| before == after)
        .count();
    let mut output = String::new();
    for line in &before[common_prefix..] {
        output.push_str("- ");
        output.push_str(line);
        output.push('\n');
    }
    for line in &after[common_prefix..] {
        output.push_str("+ ");
        output.push_str(line);
        output.push('\n');
    }
    output
}

#[derive(Clone)]
struct IndexedSymbol<'db> {
    symbol: Option<Symbol<'db>>,
    summary: SymbolSummary,
}

#[derive(Clone)]
struct SymbolDirectory {
    index: SymbolIndex,
    locators: HashMap<SymbolPath, ReferenceKey>,
    references: HashMap<ReferenceKey, SymbolReference>,
}

fn build_symbol_directory(context: &SageContext<'_>) -> SymbolDirectory {
    let entries = indexed_symbols(context);
    let index = SymbolIndex {
        root: entries[0].summary.path.clone(),
        symbols: entries.iter().map(|entry| entry.summary.clone()).collect(),
    };
    let locators = entries
        .iter()
        .filter_map(|entry| Some((entry.summary.path.clone(), entry.symbol?.reflection_key())))
        .collect();
    let references = entries
        .iter()
        .filter_map(|entry| {
            Some((
                entry.symbol?.reflection_key(),
                SymbolReference {
                    path: entry.summary.path.clone(),
                    label: entry.summary.label.clone(),
                    presentation: entry.summary.presentation.clone(),
                },
            ))
        })
        .collect();
    SymbolDirectory {
        index,
        locators,
        references,
    }
}

fn selected_symbol(
    context: &SageContext<'_>,
    directory: &SymbolDirectory,
    path: &str,
) -> Option<SelectedSymbol> {
    if path.starts_with("external/") {
        return external_selected_symbol(context, path);
    }
    let Some(summary) = directory
        .index
        .symbols
        .iter()
        .find(|summary| summary.path == path)
    else {
        return external_selected_symbol(context, path);
    };
    let parent = summary
        .parent
        .as_ref()
        .and_then(|parent| {
            directory
                .index
                .symbols
                .iter()
                .find(|summary| &summary.path == parent)
        })
        .map(|parent| SymbolReference {
            path: parent.path.clone(),
            label: parent.label.clone(),
            presentation: parent.presentation.clone(),
        });
    let products = directory
        .locators
        .get(path)
        .and_then(Symbol::from_reflection_key)
        .map(|symbol| product_catalog(context.db, path, symbol))
        .unwrap_or_default();
    Some(SelectedSymbol {
        path: summary.path.clone(),
        label: summary.label.clone(),
        display_path: summary.display_path.clone(),
        presentation: summary.presentation.clone(),
        parent,
        products,
    })
}

fn identity_product(
    context: &SageContext<'_>,
    directory: &SymbolDirectory,
    symbol_path: &str,
    product: &str,
    continuation_prefix: &str,
) -> Option<GeneratedProduct> {
    if symbol_path.starts_with("external/") {
        return external_product(context, symbol_path, product, continuation_prefix);
    }
    let Some(summary) = directory
        .index
        .symbols
        .iter()
        .find(|summary| summary.path == symbol_path)
    else {
        return external_product(context, symbol_path, product, continuation_prefix);
    };
    let symbol = resolve_local_symbol(context, symbol_path)?;
    match product {
        "identity" => Some(GeneratedProduct::page(ProductPage {
            id: "identity".to_owned(),
            title: "Symbol identity".to_owned(),
            content: RenderNode::Group {
                layout: GroupLayout::Block,
                children: vec![
                    RenderNode::Heading {
                        level: 2,
                        text: "Canonical identity".to_owned(),
                    },
                    RenderNode::Text {
                        text: summary.path.clone(),
                    },
                    RenderNode::Text {
                        text: summary.display_path.clone(),
                    },
                ],
            },
        })),
        "source" => source_product(context.db, symbol).map(GeneratedProduct::page),
        "concrete-ir" => local_fn(symbol, context.db).map(|function| {
            reflected_product(
                context,
                "concrete-ir",
                "Concrete IR",
                function.cst(context.db),
                continuation_prefix,
                &directory.references,
            )
        }),
        "signature" => local_fn(symbol, context.db).map(|function| {
            reflected_product(
                context,
                "signature",
                "Checked function signature",
                function.sig(context.db),
                continuation_prefix,
                &directory.references,
            )
        }),
        "typed-ir" => local_fn(symbol, context.db).map(|function| {
            let body = function.body(context.db).clone();
            reflected_product(
                context,
                "typed-ir",
                "Completed typed body",
                body,
                continuation_prefix,
                &directory.references,
            )
        }),
        "diagnostics" => local_fn(symbol, context.db).map(|function| {
            reflected_product(
                context,
                "diagnostics",
                "Diagnostics",
                function.body(context.db).diagnostics.clone(),
                continuation_prefix,
                &directory.references,
            )
        }),
        _ => None,
    }
}

/// Resolve one canonical local path by walking only its ownership chain. This
/// refreshes the tracked handles after an edit without rebuilding the eager
/// recursive directory or observing membership in unrelated descendant
/// modules.
fn resolve_local_symbol<'db>(context: &'db SageContext<'db>, path: &str) -> Option<Symbol<'db>> {
    let root = format!("local/{}", safe_segment(&context.target.name));
    let descendants = path.strip_prefix(&format!("{root}/"))?;
    let mut children = context.root.expanded_module_items(context.db).to_vec();
    let mut current = None;
    for expected_segment in descendants.split('/') {
        let next = canonical_children(context.db, &children)
            .into_iter()
            .find(|child| child.segment == expected_segment)?
            .symbol;
        current = Some(next);
        children = direct_symbol_children(context.db, next);
    }
    current
}

fn direct_symbol_children<'db>(db: &'db dyn Db, symbol: Symbol<'db>) -> Vec<Symbol<'db>> {
    owned_symbol_children(db, symbol)
        .map(|(children, _)| children)
        .unwrap_or_default()
}

/// Return direct represented ownership children without asking whether a
/// module's expansion is complete. Path replay deliberately needs only the
/// ownership edge; the eager directory layers completeness onto the same
/// child enumeration below.
fn owned_symbol_children<'db>(
    db: &'db dyn Db,
    symbol: Symbol<'db>,
) -> Option<(Vec<Symbol<'db>>, Option<ModSymbol<'db>>)> {
    match symbol.data(db) {
        SymbolData::ModSymbol(module @ ModSymbol::Local(_)) => {
            Some((module.expanded_module_items(db).to_vec(), Some(module)))
        }
        SymbolData::EnumSymbol(EnumSymbol::Local(symbol)) => {
            Some((EnumSymbol::Local(symbol).variants(db).to_vec(), None))
        }
        SymbolData::TraitSymbol(TraitSymbol::Local(symbol)) => Some((
            local_associated_items(db, LocalAssociatedOwner::Trait(symbol))
                .iter()
                .copied()
                .map(Into::into)
                .collect(),
            None,
        )),
        SymbolData::ImplSymbol(ImplSymbol::Local(symbol)) => Some((
            local_associated_items(db, LocalAssociatedOwner::Impl(symbol))
                .iter()
                .copied()
                .map(Into::into)
                .collect(),
            None,
        )),
        SymbolData::FnSymbol(_)
        | SymbolData::StructSymbol(_)
        | SymbolData::VariantSymbol(_)
        | SymbolData::VariantCtorSymbol(_)
        | SymbolData::TypeAliasSymbol(_)
        | SymbolData::ConstSymbol(_)
        | SymbolData::StaticSymbol(_)
        | SymbolData::MacroDefSymbol(_)
        | SymbolData::UseSymbol(_)
        | SymbolData::IntrinsicTypeSymbol(_)
        | SymbolData::MacroInvocationSymbol(_)
        | SymbolData::EnumSymbol(EnumSymbol::Ext(_))
        | SymbolData::TraitSymbol(TraitSymbol::Ext(_))
        | SymbolData::ImplSymbol(ImplSymbol::Ext(_))
        | SymbolData::ModSymbol(ModSymbol::Ext(_)) => None,
    }
}

struct CanonicalChild<'db> {
    symbol: Symbol<'db>,
    label: String,
    kind: String,
    segment: String,
}

fn canonical_children<'db>(db: &'db dyn Db, symbols: &[Symbol<'db>]) -> Vec<CanonicalChild<'db>> {
    let mut occurrences = HashMap::<String, u32>::new();
    symbols
        .iter()
        .copied()
        .filter(|symbol| !is_external(db, *symbol))
        .map(|symbol| {
            let (label, kind, base_segment) = symbol_identity_parts(db, symbol);
            let occurrence = occurrences.entry(base_segment.clone()).or_default();
            let segment = if *occurrence == 0 {
                base_segment
            } else {
                format!("{base_segment}-duplicate-{occurrence}")
            };
            *occurrence += 1;
            CanonicalChild {
                symbol,
                label,
                kind: kind.to_owned(),
                segment,
            }
        })
        .collect()
}

fn find_local_reference(context: &SageContext<'_>, target: Symbol<'_>) -> Option<SymbolReference> {
    let root: Symbol<'_> = context.root.into();
    let root_path = format!("local/{}", safe_segment(&context.target.name));
    if target == root {
        return Some(SymbolReference {
            path: root_path,
            label: context.target.name.clone(),
            presentation: presentation("Local crate", vec![]),
        });
    }

    let mut reverse_chain = vec![target];
    let mut current = target;
    while current != root {
        current = local_symbol_owner(context.db, current)?;
        reverse_chain.push(current);
        if reverse_chain.len() > 256 {
            return None;
        }
    }
    reverse_chain.reverse();

    let mut path = root_path;
    let mut parent = root;
    let mut final_child = None;
    for symbol in reverse_chain.into_iter().skip(1) {
        let child = canonical_children(context.db, &direct_symbol_children(context.db, parent))
            .into_iter()
            .find(|child| child.symbol == symbol)?;
        path.push('/');
        path.push_str(&child.segment);
        parent = symbol;
        final_child = Some(child);
    }
    let child = final_child?;
    Some(SymbolReference {
        path,
        label: child.label,
        presentation: local_symbol_presentation(context.db, child.symbol, &child.kind),
    })
}

fn local_symbol_owner<'db>(db: &'db dyn Db, symbol: Symbol<'db>) -> Option<Symbol<'db>> {
    fn scope_owner<'db>(db: &'db dyn Db, scope: sage_ir::scope::ScopeSymbol<'db>) -> Symbol<'db> {
        ModSymbol::Local(scope.module(db)).into()
    }

    fn associated_owner(owner: LocalAssociatedOwner<'_>) -> Symbol<'_> {
        match owner {
            LocalAssociatedOwner::Trait(owner) => TraitSymbol::Local(owner).into(),
            LocalAssociatedOwner::Impl(owner) => ImplSymbol::Local(owner).into(),
        }
    }

    match symbol.data(db) {
        SymbolData::FnSymbol(FnSymbol::Local(symbol)) => symbol
            .owner(db)
            .map(associated_owner)
            .or_else(|| Some(scope_owner(db, symbol.scope(db)))),
        SymbolData::TypeAliasSymbol(TypeAliasSymbol::Local(symbol)) => symbol
            .owner(db)
            .map(associated_owner)
            .or_else(|| Some(scope_owner(db, symbol.scope(db)))),
        SymbolData::ConstSymbol(ConstSymbol::Local(symbol)) => symbol
            .owner(db)
            .map(associated_owner)
            .or_else(|| Some(scope_owner(db, symbol.scope(db)))),
        SymbolData::StructSymbol(StructSymbol::Local(symbol)) => {
            Some(scope_owner(db, symbol.scope(db)))
        }
        SymbolData::EnumSymbol(EnumSymbol::Local(symbol)) => {
            Some(scope_owner(db, symbol.scope(db)))
        }
        SymbolData::TraitSymbol(TraitSymbol::Local(symbol)) => {
            Some(scope_owner(db, symbol.scope(db)))
        }
        SymbolData::StaticSymbol(StaticSymbol::Local(symbol)) => {
            Some(scope_owner(db, symbol.scope(db)))
        }
        SymbolData::ImplSymbol(ImplSymbol::Local(symbol)) => {
            Some(scope_owner(db, symbol.scope(db)))
        }
        SymbolData::MacroDefSymbol(MacroDefSymbol::Local(symbol)) => {
            Some(scope_owner(db, symbol.scope(db)))
        }
        SymbolData::UseSymbol(UseSymbol::Local(symbol)) => Some(scope_owner(db, symbol.scope(db))),
        SymbolData::MacroInvocationSymbol(symbol) => Some(scope_owner(db, symbol.scope(db))),
        SymbolData::ModSymbol(ModSymbol::Local(symbol)) => {
            symbol.parent(db).map(|parent| scope_owner(db, parent))
        }
        SymbolData::VariantSymbol(VariantSymbol::Local(symbol)) => {
            Some(EnumSymbol::Local(symbol.parent_enum(db)).into())
        }
        SymbolData::VariantCtorSymbol(VariantCtorSymbol::Local(symbol)) => {
            Some(EnumSymbol::Local(symbol.variant(db).parent_enum(db)).into())
        }
        SymbolData::FnSymbol(FnSymbol::Ext(_))
        | SymbolData::StructSymbol(StructSymbol::Ext(_))
        | SymbolData::EnumSymbol(EnumSymbol::Ext(_))
        | SymbolData::VariantSymbol(VariantSymbol::Ext(_))
        | SymbolData::VariantCtorSymbol(VariantCtorSymbol::Ext(_))
        | SymbolData::TraitSymbol(TraitSymbol::Ext(_))
        | SymbolData::TypeAliasSymbol(TypeAliasSymbol::Ext(_))
        | SymbolData::ConstSymbol(ConstSymbol::Ext(_))
        | SymbolData::StaticSymbol(StaticSymbol::Ext(_))
        | SymbolData::ImplSymbol(ImplSymbol::Ext(_))
        | SymbolData::ModSymbol(ModSymbol::Ext(_))
        | SymbolData::MacroDefSymbol(MacroDefSymbol::Ext(_))
        | SymbolData::UseSymbol(UseSymbol::Ext(_))
        | SymbolData::IntrinsicTypeSymbol(_) => None,
    }
}

fn local_symbol_presentation(db: &dyn Db, symbol: Symbol<'_>, kind: &str) -> SymbolPresentation {
    let mut badges = vec![];
    if let Some(span) = symbol_span(db, symbol) {
        let (label, tone) = match span.source {
            ParseSource::SourceFile(_) => ("Source-written", BadgeTone::Neutral),
            ParseSource::BangMacro(..) | ParseSource::Derive(..) => {
                ("Generated", BadgeTone::Accent)
            }
        };
        badges.push(Badge {
            label: label.to_owned(),
            tone,
        });
    }
    presentation(&format!("Local {kind}"), badges)
}

fn product_catalog(db: &dyn Db, path: &str, symbol: Symbol<'_>) -> Vec<ProductDescriptor> {
    let mut products = vec![product_descriptor(path, "identity", "Identity")];
    if symbol_span(db, symbol).is_some() {
        products.push(product_descriptor(path, "source", "Source"));
    }
    if local_fn(symbol, db).is_some() {
        products.extend([
            product_descriptor(path, "concrete-ir", "Concrete IR"),
            product_descriptor(path, "signature", "Signature"),
            product_descriptor(path, "diagnostics", "Diagnostics"),
            product_descriptor(path, "typed-ir", "Typed IR"),
        ]);
    }
    products
}

fn product_descriptor(path: &str, id: &str, label: &str) -> ProductDescriptor {
    ProductDescriptor {
        id: id.to_owned(),
        label: label.to_owned(),
        href: format!(
            "/api/v1/product?symbol={}&product={id}",
            percent_encode(path)
        ),
    }
}

fn local_fn<'db>(symbol: Symbol<'db>, db: &'db dyn Db) -> Option<LocalFnSym<'db>> {
    match symbol.data(db) {
        SymbolData::FnSymbol(FnSymbol::Local(function)) => Some(function),
        SymbolData::FnSymbol(FnSymbol::Ext(_))
        | SymbolData::StructSymbol(_)
        | SymbolData::EnumSymbol(_)
        | SymbolData::VariantSymbol(_)
        | SymbolData::VariantCtorSymbol(_)
        | SymbolData::TraitSymbol(_)
        | SymbolData::TypeAliasSymbol(_)
        | SymbolData::ConstSymbol(_)
        | SymbolData::StaticSymbol(_)
        | SymbolData::ImplSymbol(_)
        | SymbolData::ModSymbol(_)
        | SymbolData::MacroDefSymbol(_)
        | SymbolData::UseSymbol(_)
        | SymbolData::IntrinsicTypeSymbol(_)
        | SymbolData::MacroInvocationSymbol(_) => None,
    }
}

fn source_product(db: &dyn Db, symbol: Symbol<'_>) -> Option<ProductPage> {
    let span = symbol_span(db, symbol)?;
    let ParseSource::SourceFile(source_file) = span.source else {
        return Some(ProductPage {
            id: "source".to_owned(),
            title: "Generated source".to_owned(),
            content: RenderNode::Notice {
                tone: NoticeTone::Info,
                title: Some("Generated symbol".to_owned()),
                text: "This symbol was produced by macro expansion; inspect its origin from the identity tree.".to_owned(),
            },
        });
    };
    let text = source_file.text(db);
    let start = span.start as usize;
    let end = span.end as usize;
    let snippet = text.get(start..end).unwrap_or_default().to_owned();
    Some(ProductPage {
        id: "source".to_owned(),
        title: "Source".to_owned(),
        content: RenderNode::Code {
            language: "rust".to_owned(),
            text: snippet,
            highlights: vec![],
        },
    })
}

struct GeneratedProduct {
    page: ProductPage,
    continuations: HashMap<ContinuationHandle, Vec<ValueNode>>,
}

impl GeneratedProduct {
    fn page(page: ProductPage) -> Self {
        Self {
            page,
            continuations: HashMap::new(),
        }
    }
}

fn reflected_product<'db, T>(
    context: &'db SageContext<'db>,
    id: &str,
    title: &str,
    value: T,
    continuation_prefix: &str,
    references: &HashMap<ReferenceKey, SymbolReference>,
) -> GeneratedProduct
where
    T: Reflect<'db>,
{
    let (value, continuations) = {
        let _phase = InspectionPhase::new(context.db, "reflection");
        reflect_with_references(context, value, continuation_prefix, references)
    };
    assemble_reflected_page(context.db, id, title, value, continuations)
}

fn reflected_external_product<'db, T>(
    context: &'db SageContext<'db>,
    id: &str,
    title: &str,
    value: T,
    continuation_prefix: &str,
) -> GeneratedProduct
where
    T: Reflect<'db>,
{
    let (value, continuations) = {
        let _phase = InspectionPhase::new(context.db, "reflection");
        reflect_with_references(context, value, continuation_prefix, &HashMap::new())
    };
    assemble_reflected_page(context.db, id, title, value, continuations)
}

fn reflect_with_references<'db, T>(
    context: &'db SageContext<'db>,
    value: T,
    continuation_prefix: &str,
    references: &HashMap<ReferenceKey, SymbolReference>,
) -> (ValueNode, HashMap<ContinuationHandle, Vec<ValueNode>>)
where
    T: Reflect<'db>,
{
    let resolver = InspectorReflectionResolver::new(context, references);
    let mut reflection =
        ReflectionContext::with_continuation_prefix(96, 25_000, continuation_prefix)
            .with_resolver(&resolver);
    let raw = value.reflect(&mut reflection, None);
    let value = reflection.finish(raw);
    (value, reflection.continuations())
}

fn assemble_reflected_page(
    db: &dyn Db,
    id: &str,
    title: &str,
    value: ValueNode,
    continuations: HashMap<ContinuationHandle, Vec<ValueNode>>,
) -> GeneratedProduct {
    let _phase = InspectionPhase::new(db, "view-assembly");
    GeneratedProduct {
        page: ProductPage {
            id: id.to_owned(),
            title: title.to_owned(),
            content: RenderNode::Value { value },
        },
        continuations,
    }
}

struct InspectionPhase<'db> {
    db: &'db dyn Db,
    phase: &'static str,
}

impl<'db> InspectionPhase<'db> {
    fn new(db: &'db dyn Db, phase: &'static str) -> Self {
        db.log_inspection_phase(phase, true);
        Self { db, phase }
    }
}

impl Drop for InspectionPhase<'_> {
    fn drop(&mut self) {
        self.db.log_inspection_phase(self.phase, false);
    }
}

#[derive(Copy, Clone)]
struct ResolvedExternal<'db> {
    symbol: SymExt<'db>,
    parent: Option<SymExt<'db>>,
}

fn external_selected_symbol(context: &SageContext<'_>, path: &str) -> Option<SelectedSymbol> {
    let resolved = resolve_external_path(context.db, path)?;
    let reference = canonical_external_reference(context.db, resolved.symbol)?;
    let parent = resolved
        .parent
        .and_then(|parent| canonical_external_reference(context.db, parent));
    let products = external_product_catalog(context.db, path, resolved.symbol);
    Some(SelectedSymbol {
        path: reference.path,
        label: reference.label.clone(),
        display_path: context
            .db
            .tcx()
            .def_path(
                resolved.symbol.crate_num(context.db),
                resolved.symbol.def_index(context.db),
            )
            .unwrap_or(reference.label),
        presentation: reference.presentation,
        parent,
        products,
    })
}

fn external_product_catalog(db: &dyn Db, path: &str, symbol: SymExt<'_>) -> Vec<ProductDescriptor> {
    let mut products = vec![product_descriptor(path, "identity", "Identity")];
    match symbol.kind(db) {
        SymExtKind::Fn | SymExtKind::Trait | SymExtKind::Struct | SymExtKind::Enum => {
            products.push(product_descriptor(path, "signature", "Signature"));
        }
        SymExtKind::TupleStructCtor
        | SymExtKind::Variant
        | SymExtKind::VariantCtor
        | SymExtKind::Impl
        | SymExtKind::Mod
        | SymExtKind::TypeAlias
        | SymExtKind::Const
        | SymExtKind::Static
        | SymExtKind::MacroDef
        | SymExtKind::Use
        | SymExtKind::Other => {}
    }
    match symbol.kind(db) {
        SymExtKind::Mod | SymExtKind::Enum | SymExtKind::Trait => {
            products.push(product_descriptor(path, "items", "Items"));
        }
        SymExtKind::Fn
        | SymExtKind::Struct
        | SymExtKind::TupleStructCtor
        | SymExtKind::Variant
        | SymExtKind::VariantCtor
        | SymExtKind::Impl
        | SymExtKind::TypeAlias
        | SymExtKind::Const
        | SymExtKind::Static
        | SymExtKind::MacroDef
        | SymExtKind::Use
        | SymExtKind::Other => {}
    }
    products
}

fn external_product(
    context: &SageContext<'_>,
    path: &str,
    product: &str,
    continuation_prefix: &str,
) -> Option<GeneratedProduct> {
    let external = resolve_external_path(context.db, path)?.symbol;
    match product {
        "identity" => {
            let reference = canonical_external_reference(context.db, external)?;
            Some(GeneratedProduct::page(ProductPage {
                id: "identity".to_owned(),
                title: "External symbol identity".to_owned(),
                content: RenderNode::Value {
                    value: ValueNode::record(
                        "SymExt",
                        vec![
                            ValueField::new(
                                "path",
                                ValueNode::scalar("CanonicalSymbolPath", reference.path),
                            ),
                            ValueField::new(
                                "kind",
                                ValueNode::scalar(
                                    "SymExtKind",
                                    format!("{:?}", external.kind(context.db)),
                                ),
                            ),
                        ],
                    ),
                },
            }))
        }
        "signature" => match external.kind(context.db) {
            SymExtKind::Fn => external_fn_signature(context.db, external).map(|signature| {
                reflected_external_product(
                    context,
                    "signature",
                    "External function signature",
                    signature,
                    continuation_prefix,
                )
            }),
            SymExtKind::Trait => external_trait_signature(context.db, external).map(|signature| {
                reflected_external_product(
                    context,
                    "signature",
                    "External trait signature",
                    signature,
                    continuation_prefix,
                )
            }),
            SymExtKind::Struct | SymExtKind::Enum => external_adt_signature(context.db, external)
                .map(|signature| {
                    reflected_external_product(
                        context,
                        "signature",
                        "External type signature",
                        signature,
                        continuation_prefix,
                    )
                }),
            SymExtKind::TupleStructCtor
            | SymExtKind::Variant
            | SymExtKind::VariantCtor
            | SymExtKind::Impl
            | SymExtKind::Mod
            | SymExtKind::TypeAlias
            | SymExtKind::Const
            | SymExtKind::Static
            | SymExtKind::MacroDef
            | SymExtKind::Use
            | SymExtKind::Other => None,
        },
        "items" => {
            let symbols: Vec<Symbol<'_>> = match external.kind(context.db) {
                SymExtKind::Mod | SymExtKind::Enum => {
                    external.expanded_module_items(context.db).to_vec()
                }
                SymExtKind::Trait => {
                    let items = external_trait_items(context.db, external)?;
                    let (stash, binder) = items.open();
                    stash[binder.value]
                        .iter()
                        .map(|item| match item {
                            sage_ir::ty::TraitItemDef::Function(symbol) => (*symbol).into(),
                            sage_ir::ty::TraitItemDef::Type(symbol) => (*symbol).into(),
                            sage_ir::ty::TraitItemDef::Const(symbol) => (*symbol).into(),
                        })
                        .collect()
                }
                SymExtKind::Fn
                | SymExtKind::Struct
                | SymExtKind::TupleStructCtor
                | SymExtKind::Variant
                | SymExtKind::VariantCtor
                | SymExtKind::Impl
                | SymExtKind::TypeAlias
                | SymExtKind::Const
                | SymExtKind::Static
                | SymExtKind::MacroDef
                | SymExtKind::Use
                | SymExtKind::Other => return None,
            };
            Some(reflected_external_product(
                context,
                "items",
                "External children",
                symbols,
                continuation_prefix,
            ))
        }
        _ => None,
    }
}

fn resolve_external_path<'db>(db: &'db dyn Db, path: &str) -> Option<ResolvedExternal<'db>> {
    let remainder = path.strip_prefix("external/")?;
    let (root_segment, descendants) = remainder
        .split_once('/')
        .map_or((remainder, None), |(root, rest)| (root, Some(rest)));
    let (crate_name, stable_id) = root_segment.rsplit_once('-')?;
    let expected_stable_id = u64::from_str_radix(stable_id, 16).ok()?;
    let crate_num = db
        .tcx()
        .extern_crate_with_disambiguator(crate_name, expected_stable_id)?;
    let root = SymExt::new(db, crate_num, DefIndex(0), SymExtKind::Mod);
    let root_reference = canonical_external_reference(db, root)?;
    if root_reference.path != format!("external/{root_segment}") {
        return None;
    }
    let Some(descendants) = descendants else {
        return Some(ResolvedExternal {
            symbol: root,
            parent: None,
        });
    };
    let mut current = root;
    let mut parent = None;
    let mut prefix = format!("external/{root_segment}");
    for segment in descendants.split('/') {
        prefix.push('/');
        prefix.push_str(segment);
        let next = external_children(db, current).into_iter().find(|child| {
            canonical_external_reference(db, *child)
                .is_some_and(|reference| reference.path == prefix)
        })?;
        parent = Some(current);
        current = next;
    }
    Some(ResolvedExternal {
        symbol: current,
        parent,
    })
}

fn external_children<'db>(db: &'db dyn Db, symbol: SymExt<'db>) -> Vec<SymExt<'db>> {
    match symbol.kind(db) {
        SymExtKind::Mod | SymExtKind::Enum => db
            .tcx()
            .module_children(symbol.crate_num(db), symbol.def_index(db))
            .into_iter()
            .filter(|child| child.kind != SymExtKind::Other)
            .map(|child| SymExt::new(db, child.crate_num, child.def_index, child.kind))
            .collect(),
        SymExtKind::Trait => db
            .tcx()
            .associated_items(symbol.crate_num(db), symbol.def_index(db))
            .into_iter()
            .flat_map(|items| items.items)
            .map(|item| SymExt::new(db, item.def.crate_num, item.def.def_index, item.def.kind))
            .collect(),
        SymExtKind::Fn
        | SymExtKind::Struct
        | SymExtKind::TupleStructCtor
        | SymExtKind::Variant
        | SymExtKind::VariantCtor
        | SymExtKind::Impl
        | SymExtKind::TypeAlias
        | SymExtKind::Const
        | SymExtKind::Static
        | SymExtKind::MacroDef
        | SymExtKind::Use
        | SymExtKind::Other => vec![],
    }
}

fn canonical_external_reference(db: &dyn Db, external: SymExt<'_>) -> Option<SymbolReference> {
    let def_path = db
        .tcx()
        .canonical_def_path(external.crate_num(db), external.def_index(db))?;
    let mut path = format!(
        "external/{}-{:016x}",
        safe_segment(&def_path.krate),
        def_path.crate_disambiguator
    );
    for segment in &def_path.segments {
        path.push('/');
        path.push_str(
            external_segment(segment.kind, &segment.name, segment.disambiguator).as_str(),
        );
    }
    let label = def_path
        .segments
        .last()
        .map(|segment| segment.name.clone())
        .unwrap_or(def_path.krate);
    Some(SymbolReference {
        path,
        label,
        presentation: presentation("External symbol", vec![]),
    })
}

struct InspectorReflectionResolver<'a, 'db> {
    context: &'a SageContext<'db>,
    local_symbols: &'a HashMap<ReferenceKey, SymbolReference>,
}

impl<'a, 'db> InspectorReflectionResolver<'a, 'db> {
    fn new(
        context: &'a SageContext<'db>,
        local_symbols: &'a HashMap<ReferenceKey, SymbolReference>,
    ) -> Self {
        Self {
            context,
            local_symbols,
        }
    }

    fn db(&self) -> &'db dyn Db {
        self.context.db
    }

    fn external_reference(&self, id: u64) -> Option<SymbolReference> {
        let external = SymExt::from_id(salsa::Id::from_bits(id));
        canonical_external_reference(self.db(), external)
    }

    fn reference_for_symbol(&self, symbol: Symbol<'db>) -> Option<SymbolReference> {
        let key = symbol.reflection_key();
        self.symbol_reference(&key)
    }

    fn name_node(&self, name: Option<Name<'db>>) -> ValueNode {
        match name {
            Some(name) => ValueNode::variant(
                "Option",
                "Some",
                vec![ValueField::new(
                    "0",
                    ValueNode::scalar("Name", name.text(self.db()).clone()),
                )],
            ),
            None => ValueNode::variant("Option", "None", vec![]),
        }
    }

    fn symbol_node(&self, symbol: Symbol<'db>) -> ValueNode {
        self.reference_for_symbol(symbol)
            .map(|target| ValueNode::Reference { target })
            .unwrap_or_else(|| ValueNode::scalar("Symbol", format!("{symbol:?}")))
    }

    fn generic_param_node(
        &self,
        variant: &str,
        kind: sage_ir::generic_param::GenericParamKind,
        name: Option<Name<'db>>,
        parent: Option<Symbol<'db>>,
        index: u32,
    ) -> ValueNode {
        let mut fields = vec![
            ValueField::new(
                "kind",
                ValueNode::scalar("GenericParamKind", format!("{kind:?}")),
            ),
            ValueField::new("name", self.name_node(name)),
        ];
        if let Some(parent) = parent {
            fields.push(ValueField::new("parent", self.symbol_node(parent)));
        }
        fields.push(ValueField::new("index", ValueNode::scalar("u32", index)));
        ValueNode::variant("GenericParam", variant, fields)
    }
}

impl ReflectionResolver for InspectorReflectionResolver<'_, '_> {
    fn symbol_reference(&self, key: &ReferenceKey) -> Option<SymbolReference> {
        if key.family == "external-symbol" {
            self.external_reference(key.id)
        } else {
            self.local_symbols.get(key).cloned().or_else(|| {
                Symbol::from_reflection_key(key)
                    .and_then(|symbol| find_local_reference(self.context, symbol))
            })
        }
    }

    fn reflected_value(&self, key: &ReferenceKey) -> Option<ValueNode> {
        let id = salsa::Id::from_bits(key.id);
        match key.family {
            "name" => {
                let name = Name::from_id(id);
                Some(ValueNode::scalar("Name", name.text(self.db()).clone()))
            }
            "source-file" => {
                let source_file = SourceFile::from_id(id);
                Some(ValueNode::record(
                    "SourceFile",
                    vec![ValueField::new(
                        "path",
                        ValueNode::scalar("String", source_file.path(self.db()).clone()),
                    )],
                ))
            }
            "token-tree" => {
                let token_tree = TokenTree::from_id(id);
                let span = token_tree.span(self.db());
                Some(ValueNode::record(
                    "TokenTree",
                    vec![
                        ValueField::new(
                            "text",
                            ValueNode::scalar("String", token_tree.text(self.db()).clone()),
                        ),
                        ValueField::new("span", relative_span_node(span)),
                    ],
                ))
            }
            "derive-expansion" => {
                let expansion = DeriveExpansion::from_id(id);
                Some(ValueNode::record(
                    "DeriveExpansion",
                    vec![
                        ValueField::new(
                            "derive_name",
                            ValueNode::scalar(
                                "Name",
                                expansion.derive_name(self.db()).text(self.db()).clone(),
                            ),
                        ),
                        ValueField::new(
                            "source_item",
                            self.symbol_node(expansion.source_item(self.db()).into()),
                        ),
                        ValueField::new(
                            "attribute_index",
                            ValueNode::scalar("u32", expansion.attribute_index(self.db())),
                        ),
                        ValueField::new(
                            "derive_index",
                            ValueNode::scalar("u32", expansion.derive_index(self.db())),
                        ),
                        ValueField::new(
                            "macro_def",
                            self.symbol_node(expansion.macro_def(self.db()).into()),
                        ),
                    ],
                ))
            }
            "generic-param-ast" => {
                let param = AstGenericParam::from_id(id);
                Some(self.generic_param_node(
                    "Ast",
                    param.kind(self.db()),
                    param.name(self.db()),
                    Some(param.parent(self.db())),
                    param.index(self.db()),
                ))
            }
            "generic-param-external" => {
                let param = ExtGenericParam::from_id(id);
                Some(self.generic_param_node(
                    "Ext",
                    param.kind(self.db()),
                    param.name(self.db()),
                    Some(param.parent(self.db())),
                    param.index(self.db()),
                ))
            }
            "generic-param-alpha" => {
                let param = AlphaEquivParam::from_id(id);
                Some(self.generic_param_node(
                    "AlphaEquiv",
                    param.kind(self.db()),
                    None,
                    None,
                    param.index(self.db()),
                ))
            }
            _ => None,
        }
    }
}

fn relative_span_node(span: sage_ir::span::RelativeSpan) -> ValueNode {
    ValueNode::record(
        "RelativeSpan",
        vec![
            ValueField::new("start", ValueNode::scalar("u32", span.start)),
            ValueField::new("end", ValueNode::scalar("u32", span.end)),
        ],
    )
}

fn external_segment(kind: SymExtKind, name: &str, disambiguator: u32) -> String {
    let kind = match kind {
        SymExtKind::Fn => "fn",
        SymExtKind::Struct => "struct",
        SymExtKind::TupleStructCtor => "tuple-struct-constructor",
        SymExtKind::Enum => "enum",
        SymExtKind::Variant => "variant",
        SymExtKind::VariantCtor => "variant-constructor",
        SymExtKind::Trait => "trait",
        SymExtKind::Impl => "impl",
        SymExtKind::Mod => "module",
        SymExtKind::TypeAlias => "type-alias",
        SymExtKind::Const => "const",
        SymExtKind::Static => "static",
        SymExtKind::MacroDef => "macro",
        SymExtKind::Use => "use",
        SymExtKind::Other => "other",
    };
    format!("{kind}-{}-{disambiguator}", safe_segment(name))
}

fn indexed_symbols<'db>(context: &'db SageContext<'db>) -> Vec<IndexedSymbol<'db>> {
    let crate_name = safe_segment(&context.target.name);
    let root_path = format!("local/{crate_name}");
    let root_display = context.target.name.clone();
    let mut output = vec![IndexedSymbol {
        symbol: None,
        summary: SymbolSummary {
            path: root_path.clone(),
            parent: None,
            label: context.target.name.clone(),
            display_path: root_display.clone(),
            search_text: format!("{} crate", context.target.name),
            presentation: presentation("Local crate", vec![]),
            children: module_child_completeness(context.db, context.root),
        },
    }];
    walk_symbols(
        context.db,
        context.root.expanded_module_items(context.db),
        &root_path,
        &root_display,
        &mut output,
    );
    output
}

fn walk_symbols<'db>(
    db: &'db dyn Db,
    symbols: &[Symbol<'db>],
    parent_path: &str,
    parent_display: &str,
    output: &mut Vec<IndexedSymbol<'db>>,
) {
    for CanonicalChild {
        symbol,
        label,
        kind,
        segment,
    } in canonical_children(db, symbols)
    {
        let path = format!("{parent_path}/{segment}");
        let display_path = format!("{parent_display}::{label}");
        let children = symbol_children(db, symbol);
        let child_completeness = children
            .as_ref()
            .map(|(_, completeness)| completeness.clone())
            .unwrap_or(ChildCompleteness::NotApplicable);
        output.push(IndexedSymbol {
            symbol: Some(symbol),
            summary: SymbolSummary {
                path: path.clone(),
                parent: Some(parent_path.to_owned()),
                label: label.clone(),
                display_path: display_path.clone(),
                search_text: format!("{label} {display_path} {kind}"),
                presentation: local_symbol_presentation(db, symbol, &kind),
                children: child_completeness,
            },
        });
        if let Some((children, _)) = children {
            walk_symbols(db, &children, &path, &display_path, output);
        }
    }
}

fn symbol_children<'db>(
    db: &'db dyn Db,
    symbol: Symbol<'db>,
) -> Option<(Vec<Symbol<'db>>, ChildCompleteness)> {
    owned_symbol_children(db, symbol).map(|(children, module)| {
        let completeness = module.map_or(ChildCompleteness::Complete, |module| {
            module_child_completeness(db, module)
        });
        (children, completeness)
    })
}

fn module_child_completeness(db: &dyn Db, module: ModSymbol<'_>) -> ChildCompleteness {
    let ModSymbol::Local(module) = module else {
        return ChildCompleteness::NotApplicable;
    };
    if module_expansion_complete_for_symbol_listing(db, module) {
        ChildCompleteness::Complete
    } else {
        ChildCompleteness::Incomplete {
            reason: Issue {
                code: "macro-expansion-incomplete".to_owned(),
                message: "one or more macro-generated children are not represented".to_owned(),
            },
        }
    }
}

fn symbol_identity_parts(db: &dyn Db, symbol: Symbol<'_>) -> (String, &'static str, String) {
    let (kind, prefix) = match symbol.data(db) {
        SymbolData::FnSymbol(_) => ("function", "value-fn"),
        SymbolData::StructSymbol(_) => ("struct", "type-struct"),
        SymbolData::EnumSymbol(_) => ("enum", "type-enum"),
        SymbolData::VariantSymbol(_) => ("variant", "type-variant"),
        SymbolData::VariantCtorSymbol(_) => ("variant constructor", "value-variant-ctor"),
        SymbolData::TraitSymbol(_) => ("trait", "type-trait"),
        SymbolData::TypeAliasSymbol(_) => ("type alias", "type-alias"),
        SymbolData::ConstSymbol(_) => ("constant", "value-const"),
        SymbolData::StaticSymbol(_) => ("static", "value-static"),
        SymbolData::ImplSymbol(_) => ("impl", "impl"),
        SymbolData::ModSymbol(_) => ("module", "type-module"),
        SymbolData::MacroDefSymbol(_) => ("macro", "macro-def"),
        SymbolData::UseSymbol(_) => ("use", "use"),
        SymbolData::IntrinsicTypeSymbol(_) => ("intrinsic type", "type-intrinsic"),
        SymbolData::MacroInvocationSymbol(_) => ("macro invocation", "macro-invocation"),
    };
    let label = symbol
        .name(db)
        .map(|(name, _)| name.text(db).clone())
        .unwrap_or_else(|| format!("{kind} item"));
    let segment = if label == format!("{kind} item") {
        format!("{prefix}-{:016x}", stable_symbol_hash(db, symbol))
    } else {
        format!("{prefix}-{}", safe_segment(&label))
    };
    let display_label = if kind == "impl" {
        "impl item".to_owned()
    } else {
        label
    };
    (display_label, kind, segment)
}

fn stable_symbol_hash(db: &dyn Db, symbol: Symbol<'_>) -> u64 {
    use sage_stash::{InternHasher, StashHash};
    use std::hash::{Hash, Hasher};

    let mut hasher = InternHasher::new();
    match symbol.data(db) {
        SymbolData::ImplSymbol(ImplSymbol::Local(symbol)) => {
            let (stash, cst) = symbol.cst(db).open_deref();
            cst.generics.stash_hash(stash, &mut hasher);
            cst.is_unsafe.hash(&mut hasher);
            cst.is_negative.hash(&mut hasher);
            cst.is_const.hash(&mut hasher);
            cst.is_default.hash(&mut hasher);
            cst.self_ty.stash_hash(stash, &mut hasher);
            cst.trait_path.stash_hash(stash, &mut hasher);
            cst.where_clauses.stash_hash(stash, &mut hasher);
        }
        SymbolData::MacroInvocationSymbol(symbol) => symbol.hash(&mut hasher),
        SymbolData::UseSymbol(UseSymbol::Local(symbol)) => symbol.hash(&mut hasher),
        SymbolData::FnSymbol(_)
        | SymbolData::StructSymbol(_)
        | SymbolData::EnumSymbol(_)
        | SymbolData::VariantSymbol(_)
        | SymbolData::VariantCtorSymbol(_)
        | SymbolData::TraitSymbol(_)
        | SymbolData::TypeAliasSymbol(_)
        | SymbolData::ConstSymbol(_)
        | SymbolData::StaticSymbol(_)
        | SymbolData::ImplSymbol(ImplSymbol::Ext(_))
        | SymbolData::ModSymbol(_)
        | SymbolData::MacroDefSymbol(_)
        | SymbolData::UseSymbol(UseSymbol::Ext(_))
        | SymbolData::IntrinsicTypeSymbol(_) => symbol.hash(&mut hasher),
    }
    hasher.finish()
}

fn symbol_span<'db>(db: &'db dyn Db, symbol: Symbol<'db>) -> Option<AbsoluteSpan<'db>> {
    match symbol.data(db) {
        SymbolData::FnSymbol(FnSymbol::Local(symbol)) => Some(symbol.span(db)),
        SymbolData::StructSymbol(StructSymbol::Local(symbol)) => Some(symbol.span(db)),
        SymbolData::EnumSymbol(EnumSymbol::Local(symbol)) => Some(symbol.span(db)),
        SymbolData::VariantSymbol(VariantSymbol::Local(symbol)) => Some(symbol.span(db)),
        SymbolData::VariantCtorSymbol(VariantCtorSymbol::Local(symbol)) => {
            Some(symbol.variant(db).span(db))
        }
        SymbolData::TraitSymbol(TraitSymbol::Local(symbol)) => Some(symbol.span(db)),
        SymbolData::TypeAliasSymbol(TypeAliasSymbol::Local(symbol)) => Some(symbol.span(db)),
        SymbolData::ConstSymbol(ConstSymbol::Local(symbol)) => Some(symbol.span(db)),
        SymbolData::StaticSymbol(StaticSymbol::Local(symbol)) => Some(symbol.span(db)),
        SymbolData::ImplSymbol(ImplSymbol::Local(symbol)) => Some(symbol.span(db)),
        SymbolData::ModSymbol(ModSymbol::Local(symbol)) => Some(symbol.span(db)),
        SymbolData::MacroDefSymbol(MacroDefSymbol::Local(symbol)) => Some(symbol.span(db)),
        SymbolData::UseSymbol(UseSymbol::Local(symbol)) => Some(symbol.span(db)),
        SymbolData::MacroInvocationSymbol(symbol) => Some(symbol.span(db)),
        SymbolData::IntrinsicTypeSymbol(_)
        | SymbolData::FnSymbol(FnSymbol::Ext(_))
        | SymbolData::StructSymbol(StructSymbol::Ext(_))
        | SymbolData::EnumSymbol(EnumSymbol::Ext(_))
        | SymbolData::VariantSymbol(VariantSymbol::Ext(_))
        | SymbolData::VariantCtorSymbol(VariantCtorSymbol::Ext(_))
        | SymbolData::TraitSymbol(TraitSymbol::Ext(_))
        | SymbolData::TypeAliasSymbol(TypeAliasSymbol::Ext(_))
        | SymbolData::ConstSymbol(ConstSymbol::Ext(_))
        | SymbolData::StaticSymbol(StaticSymbol::Ext(_))
        | SymbolData::ImplSymbol(ImplSymbol::Ext(_))
        | SymbolData::ModSymbol(ModSymbol::Ext(_))
        | SymbolData::MacroDefSymbol(MacroDefSymbol::Ext(_))
        | SymbolData::UseSymbol(UseSymbol::Ext(_)) => None,
    }
}

fn is_external(db: &dyn Db, symbol: Symbol<'_>) -> bool {
    matches!(
        symbol.data(db),
        SymbolData::FnSymbol(FnSymbol::Ext(_))
            | SymbolData::StructSymbol(StructSymbol::Ext(_))
            | SymbolData::EnumSymbol(EnumSymbol::Ext(_))
            | SymbolData::VariantSymbol(VariantSymbol::Ext(_))
            | SymbolData::VariantCtorSymbol(VariantCtorSymbol::Ext(_))
            | SymbolData::TraitSymbol(TraitSymbol::Ext(_))
            | SymbolData::TypeAliasSymbol(TypeAliasSymbol::Ext(_))
            | SymbolData::ConstSymbol(ConstSymbol::Ext(_))
            | SymbolData::StaticSymbol(StaticSymbol::Ext(_))
            | SymbolData::ImplSymbol(ImplSymbol::Ext(_))
            | SymbolData::ModSymbol(ModSymbol::Ext(_))
            | SymbolData::MacroDefSymbol(MacroDefSymbol::Ext(_))
            | SymbolData::UseSymbol(UseSymbol::Ext(_))
    )
}

fn presentation(eyebrow: &str, badges: Vec<Badge>) -> SymbolPresentation {
    SymbolPresentation {
        eyebrow: Some(eyebrow.to_owned()),
        badges,
    }
}

fn safe_segment(value: &str) -> String {
    percent_encode(value)
}

fn percent_encode(value: &str) -> String {
    value
        .bytes()
        .flat_map(|byte| {
            if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'~') {
                vec![byte as char]
            } else {
                format!("%{byte:02X}").chars().collect()
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn continuation_product(value: &str) -> FrozenProduct {
        let handle = "runtime-specific-handle".to_owned();
        let page = ProductPage {
            id: "value".to_owned(),
            title: "Value".to_owned(),
            content: RenderNode::Value {
                value: ValueNode::Truncated {
                    summary: "more".to_owned(),
                    continuation: Some(handle.clone()),
                },
            },
        };
        freeze_product(
            &page,
            &HashMap::from([(handle, vec![ValueNode::scalar("String", value.to_owned())])]),
        )
    }

    #[test]
    fn retained_product_comparison_includes_frozen_continuations() {
        assert_ne!(
            continuation_product("before"),
            continuation_product("after")
        );
        assert_eq!(continuation_product("same"), continuation_product("same"));
    }

    #[test]
    fn path_segments_preserve_distinct_unicode_identifiers() {
        assert_ne!(safe_segment("α"), safe_segment("β"));
        assert_eq!(safe_segment("α"), "%CE%B1");
    }

    #[test]
    fn producer_authored_trace_order_preserves_sequential_and_sorts_unordered_groups() {
        use sage_ir::db::{InspectionChildOrder, InspectionEvent, InspectionSource};

        let events = vec![
            InspectionEvent::SpanEnter {
                operation: "solver-query",
                source: InspectionSource::Solver,
                child_order: InspectionChildOrder::Sequential,
            },
            InspectionEvent::QueryLeaf {
                key: "zeta()".to_owned(),
                disposition: salsa::QueryDisposition::Executed,
                observations: 1,
            },
            InspectionEvent::QueryLeaf {
                key: "alpha()".to_owned(),
                disposition: salsa::QueryDisposition::Executed,
                observations: 1,
            },
            InspectionEvent::SpanExit {
                operation: "solver-query",
            },
            InspectionEvent::SpanEnter {
                operation: "trait-candidates",
                source: InspectionSource::Solver,
                child_order: InspectionChildOrder::Unordered,
            },
            InspectionEvent::QueryLeaf {
                key: "zeta()".to_owned(),
                disposition: salsa::QueryDisposition::Executed,
                observations: 1,
            },
            InspectionEvent::QueryLeaf {
                key: "alpha()".to_owned(),
                disposition: salsa::QueryDisposition::Executed,
                observations: 1,
            },
            InspectionEvent::SpanExit {
                operation: "trait-candidates",
            },
            InspectionEvent::SpanEnter {
                operation: "normalization-candidates",
                source: InspectionSource::Solver,
                child_order: InspectionChildOrder::Unordered,
            },
            InspectionEvent::QueryLeaf {
                key: "second_impl()".to_owned(),
                disposition: salsa::QueryDisposition::Executed,
                observations: 1,
            },
            InspectionEvent::QueryLeaf {
                key: "first_impl()".to_owned(),
                disposition: salsa::QueryDisposition::Executed,
                observations: 1,
            },
            InspectionEvent::SpanExit {
                operation: "normalization-candidates",
            },
        ];

        let trace = build_dynamic_trace(events, TracePhase::Analysis);
        assert_eq!(trace.len(), 3);
        assert_eq!(trace[0].child_order, ChildOrder::Sequential);
        assert_eq!(
            trace[0]
                .children
                .iter()
                .map(|node| node.operation.as_str())
                .collect::<Vec<_>>(),
            ["zeta", "alpha"]
        );
        assert_eq!(trace[1].child_order, ChildOrder::Unordered);
        assert_eq!(
            trace[1]
                .children
                .iter()
                .map(|node| node.operation.as_str())
                .collect::<Vec<_>>(),
            ["alpha", "zeta"]
        );
        assert_eq!(trace[2].child_order, ChildOrder::Unordered);
        assert_eq!(
            trace[2]
                .children
                .iter()
                .map(|node| node.operation.as_str())
                .collect::<Vec<_>>(),
            ["first_impl", "second_impl"]
        );
    }
}
