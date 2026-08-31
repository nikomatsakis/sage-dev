import { cleanup, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { MemoryRouter, useLocation, useNavigate } from "react-router-dom";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { InspectorApp, ValueTree, symbolHref } from "./InspectorApp";

import routeManifest from "../../../../test-fixtures/semantic-inspector/db-drop-guard/api/routes.json?raw";
import revision from "../../../../test-fixtures/semantic-inspector/db-drop-guard/api/responses/revision.json?raw";
import session from "../../../../test-fixtures/semantic-inspector/db-drop-guard/api/responses/session.json?raw";
import symbols from "../../../../test-fixtures/semantic-inspector/db-drop-guard/api/responses/symbols.json?raw";
import localMethod from "../../../../test-fixtures/semantic-inspector/db-drop-guard/api/responses/local-db-method.json?raw";
import signature from "../../../../test-fixtures/semantic-inspector/db-drop-guard/api/responses/local-db-signature.json?raw";
import body from "../../../../test-fixtures/semantic-inspector/db-drop-guard/api/responses/local-db-body.json?raw";
import source from "../../../../test-fixtures/semantic-inspector/db-drop-guard/api/responses/local-db-source.json?raw";
import concrete from "../../../../test-fixtures/semantic-inspector/db-drop-guard/api/responses/local-db-concrete.json?raw";
import identity from "../../../../test-fixtures/semantic-inspector/db-drop-guard/api/responses/local-db-identity.json?raw";
import diagnostics from "../../../../test-fixtures/semantic-inspector/db-drop-guard/api/responses/local-db-diagnostics.json?raw";
import reviewCard from "../../../../test-fixtures/semantic-inspector/db-drop-guard/api/responses/local-db-review-card.json?raw";
import externalMethod from "../../../../test-fixtures/semantic-inspector/db-drop-guard/api/responses/external-clone-method.json?raw";
import externalIdentity from "../../../../test-fixtures/semantic-inspector/db-drop-guard/api/responses/external-clone-identity.json?raw";
import externalSignature from "../../../../test-fixtures/semantic-inspector/db-drop-guard/api/responses/external-clone-signature.json?raw";
import signatureRun from "../../../../test-fixtures/semantic-inspector/db-drop-guard/api/responses/run-signature.json?raw";
import bodyRun from "../../../../test-fixtures/semantic-inspector/db-drop-guard/api/responses/run-body.json?raw";
import symbolIndexRun from "../../../../test-fixtures/semantic-inspector/db-drop-guard/api/responses/run-generic.json?raw";
import sourceRun from "../../../../test-fixtures/semantic-inspector/db-drop-guard/api/responses/run-source.json?raw";
import identityRun from "../../../../test-fixtures/semantic-inspector/db-drop-guard/api/responses/run-identity.json?raw";
import concreteRun from "../../../../test-fixtures/semantic-inspector/db-drop-guard/api/responses/run-concrete.json?raw";
import externalSignatureRun from "../../../../test-fixtures/semantic-inspector/db-drop-guard/api/responses/run-external-signature.json?raw";
import revisions from "../../../../test-fixtures/semantic-inspector/db-drop-guard/api/responses/revisions.json?raw";
import revisionDetail from "../../../../test-fixtures/semantic-inspector/db-drop-guard/api/responses/revision-detail.json?raw";
import continuation from "../../../../test-fixtures/semantic-inspector/db-drop-guard/api/responses/continuation.json?raw";
import comparison from "../../../../test-fixtures/semantic-inspector/db-drop-guard/api/responses/comparison.json?raw";

const local = "local/db-drop-guard/impl-DbDropGuard/db";
const external = "external/core-1/clone/Clone/clone";

class StrictFixtureFetch {
  readonly demand: string[] = [];
  revisionId = "rev_0";
  private readonly statuses = new Map<string, number>();
  private readonly routes = new Map<string, string>([
    ["/api/v1/revision", revision],
    ["/api/v1/session", session],
    ["/api/v1/symbols", symbols],
    [`/api/v1/symbol?path=${encodeURIComponent(local)}`, localMethod],
    [`/api/v1/symbol?path=${encodeURIComponent(external)}`, externalMethod],
    [`/api/v1/product?symbol=${encodeURIComponent(local)}&product=identity`, identity],
    [`/api/v1/product?symbol=${encodeURIComponent(local)}&product=source`, source],
    [`/api/v1/product?symbol=${encodeURIComponent(local)}&product=concrete-ir`, concrete],
    [`/api/v1/product?symbol=${encodeURIComponent(local)}&product=signature`, signature],
    [`/api/v1/product?symbol=${encodeURIComponent(local)}&product=typed-ir`, body],
    [`/api/v1/product?symbol=${encodeURIComponent(local)}&product=diagnostics`, diagnostics],
    [`/api/v1/product?symbol=${encodeURIComponent(local)}&product=invented-review-card`, reviewCard],
    [`/api/v1/product?symbol=${encodeURIComponent(external)}&product=identity`, externalIdentity],
    [`/api/v1/product?symbol=${encodeURIComponent(external)}&product=signature`, externalSignature],
    ["/api/v1/runs/run_1", symbolIndexRun],
    ["/api/v1/runs/run_2", signatureRun],
    ["/api/v1/runs/run_3", bodyRun],
    ["/api/v1/runs/run_4", sourceRun],
    ["/api/v1/runs/run_5", identityRun],
    ["/api/v1/runs/run_6", concreteRun],
    ["/api/v1/runs/run_7", externalSignatureRun],
    ["/api/v1/revisions", revisions],
    ["/api/v1/revisions/rev_0", revisionDetail],
    ["/api/v1/continuations/fixture-continuation", continuation],
    [`/api/v1/revisions/compare?from=rev_0&to=rev_0&symbol=${encodeURIComponent(local)}&product=signature`, comparison],
  ]);

  constructor() {
    const listed = (JSON.parse(routeManifest) as Array<{ request: { path: string } }>).map(
      (route) => route.request.path,
    );
    expect([...this.routes.keys()].sort()).toEqual(listed.sort());
  }

  respond(path: string, body: string, status = 200) {
    if (!this.routes.has(path)) throw new Error(`cannot replace unlisted fixture route: ${path}`);
    this.routes.set(path, body);
    this.statuses.set(path, status);
  }

  fetch = async (input: RequestInfo | URL, init?: RequestInit) => {
    const path = typeof input === "string" ? input : input.toString();
    expect(init?.headers).toEqual({ Accept: "application/json" });
    const body = this.routes.get(path);
    if (body === undefined) throw new Error(`unlisted fixture request: ${path}`);
    this.demand.push(path);
    const parsed = JSON.parse(body) as { revision_id?: string };
    if (parsed.revision_id) parsed.revision_id = this.revisionId;
    return new Response(`${JSON.stringify(parsed, null, 2)}\n`, {
      status: this.statuses.get(path) ?? 200,
      headers: {
        "content-type": "application/json; charset=utf-8",
        "cache-control": "no-store",
      },
    });
  };
}

class FakeEventSource {
  static latest: FakeEventSource | null = null;
  private readonly listeners = new Map<string, EventListener>();
  onopen: (() => void) | null = null;
  constructor() { FakeEventSource.latest = this; }
  addEventListener(name: string, listener: EventListener) { this.listeners.set(name, listener); }
  open() { this.onopen?.(); }
  emit(name: string, revisionId: string) {
    this.listeners.get(name)?.({ data: JSON.stringify({ revision_id: revisionId }) } as MessageEvent as unknown as Event);
  }
  close() {}
}

let fixture: StrictFixtureFetch;

function RouteProbe() {
  const location = useLocation();
  const navigate = useNavigate();
  return <div><output data-testid="route">{location.pathname}{location.search}</output><button onClick={() => navigate(-1)}>Back</button><button onClick={() => navigate(1)}>Forward</button></div>;
}

beforeEach(() => {
  fixture = new StrictFixtureFetch();
  vi.stubGlobal("fetch", fixture.fetch);
  vi.stubGlobal("EventSource", FakeEventSource);
});

afterEach(() => {
  cleanup();
  vi.unstubAllGlobals();
});

describe("Semantic Inspector fixture UI", () => {
  it("fetches the complete directory once and filters it without more demand", async () => {
    const user = userEvent.setup();
    render(<MemoryRouter><InspectorApp /></MemoryRouter>);

    const search = await screen.findByRole("searchbox", { name: "Filter symbols" });
    await waitFor(() => expect(fixture.demand).toEqual([
      "/api/v1/revision",
      "/api/v1/session",
      "/api/v1/symbols",
    ]));
    await user.type(search, "dbdropguard method");
    expect(await screen.findByRole("button", { name: /db.*Local associated function/ })).toBeVisible();
    expect(fixture.demand).toHaveLength(3);
  });

  it("shows explicit incomplete child membership from the server", async () => {
    const response = JSON.parse(symbols) as {
      value: { symbols: Array<{ label: string; children: unknown }> };
    };
    response.value.symbols[0].children = {
      status: "incomplete",
      reason: { code: "macro-expansion-incomplete", message: "one macro did not expand" },
    };
    fixture.respond("/api/v1/symbols", `${JSON.stringify(response, null, 2)}\n`);
    render(<MemoryRouter><InspectorApp /></MemoryRouter>);

    expect(await screen.findByRole("img", { name: "Incomplete children for db_drop_guard" }))
      .toHaveAttribute("title", "macro-expansion-incomplete: one macro did not expand");
  });

  it("keeps search, grown panels, and symbol navigation in browser history", async () => {
    const user = userEvent.setup();
    render(<MemoryRouter initialEntries={["/?q=Db&grow=symbols"]}><InspectorApp /><RouteProbe /></MemoryRouter>);

    const search = await screen.findByRole("searchbox", { name: "Filter symbols" });
    expect(search).toHaveValue("Db");
    expect(screen.getByRole("button", { name: "Restore Workspace symbols" })).toBeVisible();
    await user.clear(search);
    await user.type(search, "dbdropguard method");
    expect(screen.getByTestId("route")).toHaveTextContent("q=dbdropguard+method");
    await user.click(await screen.findByRole("button", { name: /db.*Local associated function/ }));
    await waitFor(() => expect(screen.getByTestId("route")).toHaveTextContent("/symbols/"));
    await user.click(screen.getByRole("button", { name: "Back" }));
    await waitFor(() => expect(screen.getByTestId("route")).toHaveTextContent(/^\/\?/));
    await user.click(screen.getByRole("button", { name: "Forward" }));
    await waitFor(() => expect(screen.getByTestId("route")).toHaveTextContent("/symbols/"));
  });

  it("restores a directly routed product and renders its full value tree", async () => {
    render(
      <MemoryRouter initialEntries={[symbolHref(local, "signature")]}>
        <InspectorApp />
      </MemoryRouter>,
    );

    expect(await screen.findByText("Checked function signature")).toBeVisible();
    expect(screen.getByText("Binder<FnSig>")).toBeVisible();
    expect(screen.getByText("owner_generic_count")).toBeVisible();
    expect(screen.getByText("LocalFnSym::sig")).toBeVisible();
    expect(fixture.demand).toContain(`/api/v1/product?symbol=${encodeURIComponent(local)}&product=signature`);
    expect(fixture.demand).not.toContain(`/api/v1/product?symbol=${encodeURIComponent(local)}&product=typed-ir`);
  });

  it("caches products within a revision and reruns only on request", async () => {
    const user = userEvent.setup();
    render(
      <MemoryRouter initialEntries={[symbolHref(local, "signature")]}>
        <InspectorApp />
      </MemoryRouter>,
    );
    const signaturePath = `/api/v1/product?symbol=${encodeURIComponent(local)}&product=signature`;
    await screen.findByText("Checked function signature");
    expect(fixture.demand.filter((path) => path === signaturePath)).toHaveLength(1);

    await user.click(screen.getByRole("tab", { name: "Source" }));
    await screen.findByText(/fn db/);
    await user.click(screen.getByRole("tab", { name: "Signature" }));
    await screen.findByText("Checked function signature");
    expect(fixture.demand.filter((path) => path === signaturePath)).toHaveLength(1);

    await user.click(screen.getByRole("button", { name: "Rerun" }));
    await waitFor(() => expect(fixture.demand.filter((path) => path === signaturePath)).toHaveLength(2));
  });

  it("renders an invented product id without a product-specific component", async () => {
    render(
      <MemoryRouter initialEntries={[symbolHref(local, "invented-review-card")]}>
        <InspectorApp />
      </MemoryRouter>,
    );

    expect(await screen.findByText("Protocol-driven page")).toBeVisible();
    expect(screen.getByText("This invented product renders without a product-specific frontend case.")).toBeVisible();
  });

  it("navigates from a reflected value to an external symbol with only listed tabs", async () => {
    const user = userEvent.setup();
    render(
      <MemoryRouter initialEntries={[symbolHref(local, "typed-ir")]}>
        <InspectorApp />
      </MemoryRouter>,
    );

    await user.click(await screen.findByRole("button", { name: /core::clone::Clone::clone/ }));
    await waitFor(() => expect(screen.queryByRole("tab", { name: "Typed body" })).not.toBeInTheDocument());
    expect(await screen.findByText("External symbol")).toBeVisible();
    expect(screen.getByRole("tab", { name: "Signature" })).toBeVisible();
  });

  it("shows retained revisions with edits and demanded runs kept separate", async () => {
    const user = userEvent.setup();
    render(<MemoryRouter><InspectorApp /></MemoryRouter>);

    await user.click(await screen.findByRole("button", { name: "Revisions" }));
    expect(await screen.findByRole("heading", { name: "rev_0" })).toBeVisible();
    expect(screen.getByText("Initial workspace state")).toBeVisible();
    expect(screen.getByText("No input changes.")).toBeVisible();
    expect(screen.getByText("run_7")).toBeVisible();
    expect(fixture.demand).toContain("/api/v1/revisions/rev_0");
  });

  it("compares a previously inspected product through the generic revisions view", async () => {
    const user = userEvent.setup();
    render(<MemoryRouter initialEntries={["/revisions/rev_0"]}><InspectorApp /></MemoryRouter>);

    await screen.findByRole("heading", { name: "rev_0" });
    await user.type(screen.getByRole("textbox", { name: "Comparison symbol" }), local);
    await user.type(screen.getByRole("textbox", { name: "Comparison product" }), "signature");
    await user.click(screen.getByRole("button", { name: "Compare" }));
    expect(await screen.findByText("Value unchanged")).toBeVisible();
    expect(fixture.demand).toContain(`/api/v1/revisions/compare?from=rev_0&to=rev_0&symbol=${encodeURIComponent(local)}&product=signature`);
  });

  it("discards response state and replays the durable URL after a revision event", async () => {
    render(<MemoryRouter initialEntries={[symbolHref(local, "signature")]}><InspectorApp /><RouteProbe /></MemoryRouter>);
    await screen.findByText("Checked function signature");
    const initialRevisionRequests = fixture.demand.filter((path) => path === "/api/v1/revision").length;
    fixture.revisionId = "rev_1";
    FakeEventSource.latest?.emit("revision-advanced", "rev_1");

    await waitFor(() => expect(fixture.demand.filter((path) => path === "/api/v1/revision")).toHaveLength(initialRevisionRequests + 1));
    await waitFor(() => expect(fixture.demand.filter((path) => path.includes("product=") && path.includes("signature")).length).toBeGreaterThanOrEqual(2));
    expect(screen.getByTestId("route")).toHaveTextContent(symbolHref(local, "signature"));
  });

  it("checks the current revision whenever the event stream reconnects", async () => {
    render(<MemoryRouter initialEntries={[symbolHref(local, "signature")]}><InspectorApp /></MemoryRouter>);
    await screen.findByText("Checked function signature");
    const initialRevisionRequests = fixture.demand.filter((path) => path === "/api/v1/revision").length;
    fixture.revisionId = "rev_1";
    FakeEventSource.latest?.open();

    await waitFor(() => expect(fixture.demand.filter((path) => path === "/api/v1/revision")).toHaveLength(initialRevisionRequests + 1));
    await waitFor(() => expect(fixture.demand.filter((path) => path.includes("product=") && path.includes("signature")).length).toBeGreaterThanOrEqual(2));
  });

  it("falls back explicitly when replayed URL intent no longer resolves", async () => {
    render(<MemoryRouter initialEntries={[symbolHref(local, "signature")]}><InspectorApp /><RouteProbe /></MemoryRouter>);
    await screen.findByText("Checked function signature");
    fixture.respond(
      `/api/v1/symbol?path=${encodeURIComponent(local)}`,
      JSON.stringify({
        revision_id: "rev_1",
        request_id: "request-missing",
        run_id: null,
        error: { code: "symbol-not-found", message: "symbol was renamed" },
      }),
      404,
    );
    fixture.revisionId = "rev_1";
    FakeEventSource.latest?.emit("revision-advanced", "rev_1");

    await waitFor(() => expect(screen.getByTestId("route")).toHaveTextContent(/^\/$/));
    expect(screen.getByRole("alert")).toHaveTextContent("no longer exists");
  });

  it("expands and collapses the complete execution tree", async () => {
    const user = userEvent.setup();
    const { container } = render(<MemoryRouter initialEntries={[symbolHref(local, "typed-ir")]}><InspectorApp /></MemoryRouter>);
    await screen.findByText("Completed typed body");
    await waitFor(() => expect(container.querySelectorAll(".trace-node").length).toBeGreaterThan(1));
    await user.click(screen.getByRole("button", { name: "Collapse all" }));
    expect(Array.from(container.querySelectorAll<HTMLDetailsElement>(".trace-node")).every((node) => !node.open)).toBe(true);
    await user.click(screen.getByRole("button", { name: "Expand all" }));
    expect(Array.from(container.querySelectorAll<HTMLDetailsElement>(".trace-node")).every((node) => node.open)).toBe(true);
  });

  it("loads a frozen reflected continuation only when requested", async () => {
    const user = userEvent.setup();
    render(<ValueTree node={{ kind: "truncated", summary: "more fields", continuation: "fixture-continuation" }} navigateSymbol={() => {}} />);

    expect(fixture.demand).not.toContain("/api/v1/continuations/fixture-continuation");
    await user.click(screen.getByRole("button", { name: "Continue…" }));
    expect(await screen.findByText("loaded on demand")).toBeVisible();
  });

  it("renders terminal truncation without an unusable continuation control", () => {
    render(<ValueTree node={{ kind: "truncated", summary: "node budget exhausted" }} navigateSymbol={() => {}} />);
    expect(screen.getByText("node budget exhausted")).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Continue…" })).not.toBeInTheDocument();
  });
});
