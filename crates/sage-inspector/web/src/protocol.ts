export type RevisionId = string;
export type SymbolPath = string;

export interface Response<T> {
  revision_id: RevisionId;
  request_id: string;
  run_id: string | null;
  value: T;
}

export interface Badge {
  label: string;
  tone: "neutral" | "accent" | "success" | "warning" | "danger";
}

export interface Presentation {
  eyebrow: string | null;
  badges: Badge[];
}

export interface SymbolReference {
  path: SymbolPath;
  label: string;
  presentation: Presentation;
}

export interface Session {
  protocol_version: "1";
  target: {
    package: string;
    target_kind: "lib" | "bin" | "example" | "test" | "bench";
    target_name: string;
  };
  workspace_root: string;
  capabilities: string[];
  retained_revisions: { first: string | null; last: string | null };
}

export interface SymbolIndex {
  root: SymbolPath;
  symbols: SymbolSummary[];
}

export interface SymbolSummary {
  path: SymbolPath;
  parent: SymbolPath | null;
  label: string;
  display_path: string;
  search_text: string;
  presentation: Presentation;
  children:
    | { status: "complete" }
    | { status: "not-applicable" }
    | { status: "incomplete"; reason: { code: string; message: string } };
}

export interface ProductDescriptor {
  id: string;
  label: string;
  href: string;
}

export interface SelectedSymbol {
  path: SymbolPath;
  label: string;
  display_path: string;
  presentation: Presentation;
  parent: SymbolReference | null;
  products: ProductDescriptor[];
}

export interface ProductPage {
  id: string;
  title: string;
  content: RenderNode;
}

export interface ContinuationValue {
  continuation: string;
  items: ValueNode[];
  next: string | null;
}

export type RenderNode =
  | { kind: "group"; layout: "block" | "row" | "columns"; children: RenderNode[] }
  | { kind: "heading"; level: 1 | 2 | 3; text: string }
  | { kind: "text"; text: string }
  | { kind: "code"; language: string; text: string; highlights: CodeHighlight[] }
  | { kind: "notice"; tone: "neutral" | "info" | "warning" | "error"; title: string | null; text: string }
  | { kind: "navigation"; target: SymbolReference }
  | { kind: "value"; value: ValueNode };

export interface CodeHighlight {
  start: number;
  end: number;
  role: string;
}

export type ValueNode =
  | { kind: "record"; type_name: string; fields: ValueField[] }
  | { kind: "variant"; enum_name: string; variant_name: string; fields: ValueField[] }
  | { kind: "sequence"; type_name: string; items: ValueNode[] }
  | { kind: "scalar"; type_name: string; value: null | boolean | number | string }
  | { kind: "reference"; target: SymbolReference }
  | { kind: "shared"; identity: string; value: ValueNode }
  | { kind: "shared-reference"; identity: string }
  | { kind: "truncated"; summary: string; continuation?: string | null };

export interface ValueField {
  name: string;
  value: ValueNode;
}

export interface RunObservation {
  run_id: string;
  request: unknown;
  root: TraceNode;
}

export interface TraceNode {
  phase: "bootstrap" | "selection" | "analysis" | "reflection" | "view-assembly";
  source: "salsa" | "sage" | "solver" | "external-metadata";
  operation: string;
  key: { kind: "semantic"; value: string } | { kind: "unmapped"; ingredient: string };
  disposition: "executed" | "validated" | "reused" | "cancelled" | "observed";
  child_order: "sequential" | "unordered";
  observations?: number;
  children: TraceNode[];
}

export interface RevisionSummary {
  revision_id: string;
  cause:
    | { kind: "initial" }
    | { kind: "input-edit"; edit_batch: string }
    | { kind: "workspace-reload"; previous_revision_id: string; reason: { code: string; message: string } };
  input_delta_count: number;
  run_count: number;
}

export interface RevisionPage {
  revisions: RevisionSummary[];
  next_cursor: string | null;
}

export interface InputDelta {
  input: { kind: "source-file"; path: string; field: "text" };
  old_hash: string;
  new_hash: string;
  diff: string;
}

export interface RevisionDetail {
  summary: RevisionSummary;
  input_deltas: InputDelta[];
  runs: string[];
}

export interface TraceIdentity {
  source: TraceNode["source"];
  operation: string;
  key: TraceNode["key"];
}

export interface RunComparison {
  from_revision: string;
  to_revision: string;
  symbol: string;
  product: string;
  value_changed: boolean;
  executed_only_before: TraceIdentity[];
  executed_only_after: TraceIdentity[];
  reused_only_before: TraceIdentity[];
  reused_only_after: TraceIdentity[];
}

export interface ApiError {
  code: string;
  message: string;
}

export interface ErrorResponse {
  revision_id: RevisionId;
  request_id: string;
  run_id: string | null;
  error: ApiError;
}
