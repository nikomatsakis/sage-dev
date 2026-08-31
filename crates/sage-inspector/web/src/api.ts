import type {
  ErrorResponse,
  ContinuationValue,
  ProductPage,
  Response,
  RevisionPage,
  RevisionDetail,
  RunComparison,
  RunObservation,
  SelectedSymbol,
  Session,
  SymbolIndex,
} from "./protocol";

export class RevisionChanged extends Error {
  constructor(public readonly revision: string) {
    super(`inspector revision changed to ${revision}`);
  }
}

export class InspectorError extends Error {
  constructor(public readonly code: string, message: string) {
    super(message);
  }
}

export class ApiClient {
  private expectedRevision: string | null = null;

  installRevision(revision: string | null) {
    this.expectedRevision = revision;
  }

  revision() {
    return this.request<null>("/api/v1/revision", true);
  }

  session() {
    return this.request<Session>("/api/v1/session");
  }

  symbols() {
    return this.request<SymbolIndex>("/api/v1/symbols");
  }

  symbol(path: string) {
    return this.request<SelectedSymbol>(`/api/v1/symbol?path=${encodeURIComponent(path)}`);
  }

  product(href: string) {
    return this.request<ProductPage>(href);
  }

  run(id: string) {
    return this.request<RunObservation>(`/api/v1/runs/${encodeURIComponent(id)}`);
  }

  continuation(handle: string) {
    return this.request<ContinuationValue>(`/api/v1/continuations/${encodeURIComponent(handle)}`);
  }

  revisions() {
    return this.request<RevisionPage>("/api/v1/revisions");
  }


  revisionDetail(revision: string) {
    return this.request<RevisionDetail>(`/api/v1/revisions/${encodeURIComponent(revision)}`);
  }

  compare(from: string, to: string, symbol: string, product: string) {
    const query = new URLSearchParams({ from, to, symbol, product });
    return this.request<RunComparison>(`/api/v1/revisions/compare?${query.toString()}`);
  }

  private async request<T>(path: string, bootstrap = false): Promise<Response<T>> {
    const response = await fetch(path, { headers: { Accept: "application/json" } });
    const payload = (await response.json()) as Response<T> | ErrorResponse;
    if (!bootstrap && this.expectedRevision !== null && payload.revision_id !== this.expectedRevision) {
      throw new RevisionChanged(payload.revision_id);
    }
    if (!response.ok) {
      const failure = payload as ErrorResponse;
      throw new InspectorError(failure.error.code, failure.error.message);
    }
    return payload as Response<T>;
  }
}
