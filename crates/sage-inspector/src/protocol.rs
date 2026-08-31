use sage_reflect::{SymbolPresentation, SymbolReference, ValueNode};
use serde::{Deserialize, Serialize};

pub type SymbolPath = String;
pub type ProductId = String;
pub type ContinuationHandle = String;
pub type RunHandle = String;
pub type RevisionId = String;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Response<T> {
    pub revision_id: RevisionId,
    pub request_id: String,
    pub run_id: Option<RunHandle>,
    pub value: T,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ErrorResponse {
    pub revision_id: RevisionId,
    pub request_id: String,
    pub run_id: Option<RunHandle>,
    pub error: ApiError,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, thiserror::Error)]
#[error("{message}")]
pub struct ApiError {
    pub code: String,
    pub message: String,
}

impl ApiError {
    pub fn new(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            code: code.into(),
            message: message.into(),
        }
    }

    pub fn not_found(message: impl Into<String>) -> Self {
        Self::new("not-found", message)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Issue {
    pub code: String,
    pub message: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Session {
    pub protocol_version: String,
    pub target: CargoTarget,
    pub workspace_root: String,
    pub capabilities: Vec<Capability>,
    pub retained_revisions: RetainedRevisionRange,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CargoTarget {
    pub package: String,
    pub target_kind: TargetKind,
    pub target_name: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum TargetKind {
    Lib,
    Bin,
    Example,
    Test,
    Bench,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Capability {
    SymbolIndex,
    Products,
    Runs,
    Events,
    Revisions,
    RevisionComparison,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RetainedRevisionRange {
    pub first: Option<RevisionId>,
    pub last: Option<RevisionId>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SymbolIndex {
    pub root: SymbolPath,
    pub symbols: Vec<SymbolSummary>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SymbolSummary {
    pub path: SymbolPath,
    pub parent: Option<SymbolPath>,
    pub label: String,
    pub display_path: String,
    pub search_text: String,
    pub presentation: SymbolPresentation,
    pub children: ChildCompleteness,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "kebab-case")]
pub enum ChildCompleteness {
    Complete,
    NotApplicable,
    Incomplete { reason: Issue },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SelectedSymbol {
    pub path: SymbolPath,
    pub label: String,
    pub display_path: String,
    pub presentation: SymbolPresentation,
    pub parent: Option<SymbolReference>,
    pub products: Vec<ProductDescriptor>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProductDescriptor {
    pub id: ProductId,
    pub label: String,
    pub href: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ProductPage {
    pub id: ProductId,
    pub title: String,
    pub content: RenderNode,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum RenderNode {
    Group {
        layout: GroupLayout,
        children: Vec<RenderNode>,
    },
    Heading {
        level: u8,
        text: String,
    },
    Text {
        text: String,
    },
    Code {
        language: String,
        text: String,
        highlights: Vec<CodeHighlight>,
    },
    Notice {
        tone: NoticeTone,
        title: Option<String>,
        text: String,
    },
    Navigation {
        target: SymbolReference,
    },
    Value {
        value: ValueNode,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum GroupLayout {
    Block,
    Row,
    Columns,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum NoticeTone {
    Neutral,
    Info,
    Warning,
    Error,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CodeHighlight {
    pub start: u32,
    pub end: u32,
    pub role: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ContinuationValue {
    pub continuation: ContinuationHandle,
    pub items: Vec<ValueNode>,
    pub next: Option<ContinuationHandle>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RunObservation {
    pub run_id: RunHandle,
    pub request: RunRequest,
    pub root: TraceNode,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum RunRequest {
    SymbolIndex,
    Symbol {
        target: SymbolPath,
    },
    Product {
        target: SymbolPath,
        product: ProductId,
    },
    AutomaticRefresh {
        resource: String,
    },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TraceNode {
    pub phase: TracePhase,
    pub source: TraceSource,
    pub operation: String,
    pub key: TraceKey,
    pub disposition: TraceDisposition,
    pub child_order: ChildOrder,
    #[serde(
        default = "one_observation",
        skip_serializing_if = "is_one_observation"
    )]
    pub observations: u64,
    pub children: Vec<TraceNode>,
}

fn one_observation() -> u64 {
    1
}

fn is_one_observation(value: &u64) -> bool {
    *value == 1
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum TracePhase {
    Bootstrap,
    Selection,
    Analysis,
    Reflection,
    ViewAssembly,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum TraceSource {
    Salsa,
    Sage,
    Solver,
    ExternalMetadata,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum TraceDisposition {
    Executed,
    Validated,
    Reused,
    Cancelled,
    Observed,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ChildOrder {
    Sequential,
    Unordered,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum TraceKey {
    Semantic { value: String },
    Unmapped { ingredient: String },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RevisionAdvanced {
    pub revision_id: RevisionId,
    pub edit_batch: String,
    pub changed_inputs: Vec<InputIdentity>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkspaceReloaded {
    pub previous_revision_id: RevisionId,
    pub revision_id: RevisionId,
    pub reason: Issue,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct InputIdentity {
    pub kind: InputKind,
    pub path: String,
    pub field: InputField,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum InputKind {
    SourceFile,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum InputField {
    Text,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RevisionPage {
    pub revisions: Vec<RevisionSummary>,
    pub next_cursor: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RevisionSummary {
    pub revision_id: RevisionId,
    pub cause: RevisionCause,
    pub input_delta_count: u32,
    pub run_count: u32,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum RevisionCause {
    Initial,
    InputEdit {
        edit_batch: String,
    },
    WorkspaceReload {
        previous_revision_id: RevisionId,
        reason: Issue,
    },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RevisionDetail {
    pub summary: RevisionSummary,
    pub input_deltas: Vec<InputDelta>,
    pub runs: Vec<RunHandle>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct InputDelta {
    pub input: InputIdentity,
    pub old_hash: String,
    pub new_hash: String,
    pub diff: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RunComparison {
    pub from_revision: RevisionId,
    pub to_revision: RevisionId,
    pub symbol: SymbolPath,
    pub product: ProductId,
    pub value_changed: bool,
    pub executed_only_before: Vec<TraceIdentity>,
    pub executed_only_after: Vec<TraceIdentity>,
    pub reused_only_before: Vec<TraceIdentity>,
    pub reused_only_after: Vec<TraceIdentity>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TraceIdentity {
    pub source: TraceSource,
    pub operation: String,
    pub key: TraceKey,
    #[serde(
        default = "one_observation",
        skip_serializing_if = "is_one_observation"
    )]
    pub observations: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "event", content = "value", rename_all = "kebab-case")]
pub enum RevisionEvent {
    RevisionAdvanced(RevisionAdvanced),
    WorkspaceReloaded(WorkspaceReloaded),
}
