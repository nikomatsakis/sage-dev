import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { useLocation, useNavigate } from "react-router-dom";
import { ApiClient, InspectorError, RevisionChanged } from "./api";
import type {
  ProductDescriptor,
  ProductPage,
  RenderNode,
  RevisionDetail,
  RevisionPage,
  RevisionSummary,
  RunComparison,
  RunObservation,
  SelectedSymbol,
  Session,
  SymbolIndex,
  SymbolReference,
  SymbolSummary,
  TraceNode,
  ValueNode,
} from "./protocol";

const api = new ApiClient();

type ViewIntent =
  | { kind: "directory" }
  | { kind: "symbol"; path: string; product: string | null }
  | { kind: "revisions"; revision: string | null };

function parseIntent(pathname: string): ViewIntent {
  if (pathname === "/revisions") return { kind: "revisions", revision: null };
  if (pathname.startsWith("/revisions/")) {
    return { kind: "revisions", revision: decodeURIComponent(pathname.slice("/revisions/".length)) };
  }
  if (!pathname.startsWith("/symbols/")) return { kind: "directory" };
  const parts = pathname.slice("/symbols/".length).split("/");
  return {
    kind: "symbol",
    path: decodeURIComponent(parts[0] ?? ""),
    product: parts[1] ? decodeURIComponent(parts[1]) : null,
  };
}

export function symbolHref(path: string, product?: string | null) {
  const base = `/symbols/${encodeURIComponent(path)}`;
  return product ? `${base}/${encodeURIComponent(product)}` : base;
}

export function InspectorApp() {
  const location = useLocation();
  const navigate = useNavigate();
  const intent = useMemo(() => parseIntent(location.pathname), [location.pathname]);
  const viewQuery = useMemo(() => new URLSearchParams(location.search), [location.search]);
  const searchText = viewQuery.get("q") ?? "";
  const grownPanel = viewQuery.get("grow");
  const [revision, setRevision] = useState<string | null>(null);
  const [session, setSession] = useState<Session | null>(null);
  const [index, setIndex] = useState<SymbolIndex | null>(null);
  const [selected, setSelected] = useState<SelectedSymbol | null>(null);
  const [product, setProduct] = useState<ProductPage | null>(null);
  const [run, setRun] = useState<RunObservation | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [bootstrapKey, setBootstrapKey] = useState(0);
  const [rerunVersion, setRerunVersion] = useState(0);
  const rerunRequested = useRef(false);
  const productCache = useRef(new Map<string, { product: ProductPage; run: RunObservation | null }>());

  const resetForRevision = useCallback((nextRevision?: string) => {
    api.installRevision(nextRevision ?? null);
    setRevision(nextRevision ?? null);
    setSession(null);
    setIndex(null);
    setSelected(null);
    setProduct(null);
    setRun(null);
    productCache.current.clear();
    setBootstrapKey((key) => key + 1);
  }, []);

  const handleFailure = useCallback(
    (failure: unknown) => {
      if (failure instanceof RevisionChanged) {
        resetForRevision(failure.revision);
      } else {
        setError(failure instanceof Error ? failure.message : String(failure));
      }
    },
    [resetForRevision],
  );

  useEffect(() => {
    let cancelled = false;
    async function bootstrap() {
      setError(null);
      try {
        const current = await api.revision();
        if (cancelled) return;
        api.installRevision(current.revision_id);
        setRevision(current.revision_id);
        const [sessionResponse, symbolsResponse] = await Promise.all([api.session(), api.symbols()]);
        if (cancelled) return;
        setSession(sessionResponse.value);
        setIndex(symbolsResponse.value);
      } catch (failure) {
        if (!cancelled) handleFailure(failure);
      }
    }
    void bootstrap();
    return () => {
      cancelled = true;
    };
  }, [bootstrapKey, handleFailure]);

  useEffect(() => {
    if (!session?.capabilities.includes("events")) return;
    let cancelled = false;
    const events = new EventSource("/api/v1/events");
    const advance = (event: MessageEvent) => {
      const payload = JSON.parse(event.data) as { revision_id: string };
      if (payload.revision_id !== revision) resetForRevision(payload.revision_id);
    };
    events.onopen = () => {
      void api.revision().then((current) => {
        if (!cancelled && current.revision_id !== revision) {
          resetForRevision(current.revision_id);
        }
      }).catch((failure: unknown) => {
        if (!cancelled) handleFailure(failure);
      });
    };
    events.addEventListener("revision-advanced", advance as EventListener);
    events.addEventListener("workspace-reloaded", advance as EventListener);
    return () => {
      cancelled = true;
      events.close();
    };
  }, [handleFailure, revision, resetForRevision, session]);

  useEffect(() => {
    if (intent.kind !== "symbol" || !index || !revision) {
      setSelected(null);
      setProduct(null);
      setRun(null);
      return;
    }
    const symbolIntent = intent;
    let cancelled = false;
    async function select() {
      try {
        setError(null);
        const response = await api.symbol(symbolIntent.path);
        if (cancelled) return;
        setSelected(response.value);
        setRun(null);
        const descriptor = symbolIntent.product
          ? response.value.products.find((candidate) => candidate.id === symbolIntent.product)
          : response.value.products[0];
        if (!descriptor) {
          setProduct(null);
          if (symbolIntent.product) {
            setError(`Product '${symbolIntent.product}' is no longer available for this symbol.`);
            navigate(symbolHref(symbolIntent.path), { replace: true });
          }
          return;
        }
        if (!symbolIntent.product) {
          navigate(symbolHref(symbolIntent.path, descriptor.id), { replace: true });
          return;
        }
        const cacheKey = `${revision}\u0000${symbolIntent.path}\u0000${descriptor.id}`;
        const forceRerun = rerunRequested.current;
        rerunRequested.current = false;
        const cached = forceRerun ? undefined : productCache.current.get(cacheKey);
        if (cached) {
          setProduct(cached.product);
          setRun(cached.run);
          return;
        }
        const productResponse = await api.product(descriptor.href);
        if (cancelled) return;
        setProduct(productResponse.value);
        let observedRun: RunObservation | null = null;
        if (productResponse.run_id) {
          const runResponse = await api.run(productResponse.run_id);
          if (cancelled) return;
          observedRun = runResponse.value;
        }
        setRun(observedRun);
        productCache.current.set(cacheKey, { product: productResponse.value, run: observedRun });
      } catch (failure) {
        if (!cancelled && failure instanceof InspectorError && failure.code === "symbol-not-found") {
          setError(`The symbol at '${symbolIntent.path}' no longer exists in this revision.`);
          navigate("/", { replace: true });
        } else if (!cancelled) {
          handleFailure(failure);
        }
      }
    }
    void select();
    return () => {
      cancelled = true;
    };
  }, [handleFailure, index, intent, navigate, rerunVersion, revision]);

  const rerun = useCallback(() => {
    if (intent.kind !== "symbol" || !intent.product || !revision) return;
    productCache.current.delete(`${revision}\u0000${intent.path}\u0000${intent.product}`);
    rerunRequested.current = true;
    setRerunVersion((version) => version + 1);
  }, [intent, revision]);

  const navigateSymbol = useCallback(
    (path: string, productId?: string | null) => navigate({ pathname: symbolHref(path, productId), search: location.search }),
    [location.search, navigate],
  );

  const setViewParameter = useCallback((name: string, value: string | null, replace = false) => {
    const query = new URLSearchParams(location.search);
    if (value) query.set(name, value); else query.delete(name);
    navigate({ pathname: location.pathname, search: query.toString() }, { replace });
  }, [location.pathname, location.search, navigate]);

  if (!session || !index || !revision) {
    return <div className="loading">{error ?? "Loading Semantic Inspector…"}</div>;
  }

  return (
    <div className="app-shell">
      <header className="topbar">
        <div className="brand"><span className="mark">S</span><div><strong>Semantic Inspector</strong><small>live Sage database</small></div></div>
        <code>cargo sage inspect — {session.target.package}:{session.target.target_name}</code>
        <div className="session-state"><span className="online-dot" />{revision}</div>
      </header>
      <nav className="view-nav">
        <button className={intent.kind !== "revisions" ? "active" : ""} onClick={() => navigate({ pathname: "/", search: location.search })}>Symbols</button>
        {session.capabilities.includes("revisions") && (
          <button className={intent.kind === "revisions" ? "active" : ""} onClick={() => navigate({ pathname: "/revisions", search: location.search })}>Revisions</button>
        )}
      </nav>
      {error && <div role="alert" className="error-banner">{error}</div>}
      {intent.kind === "revisions" ? (
        <RevisionsView revision={intent.revision} navigate={navigate} />
      ) : (
        <div className="workspace">
          <GrowablePanel id="symbols-panel" className="symbols-panel" title="Workspace symbols" grown={grownPanel === "symbols"} onToggle={() => setViewParameter("grow", grownPanel === "symbols" ? null : "symbols")}>
            <SymbolDirectory index={index} selected={selected?.path ?? null} navigateSymbol={navigateSymbol} query={searchText} onQuery={(value) => setViewParameter("q", value || null, true)} />
          </GrowablePanel>
          <main className="detail-panel">
            {selected ? (
              <SymbolDetail
                selected={selected}
                activeProduct={intent.kind === "symbol" ? intent.product : null}
                product={product}
                navigateSymbol={navigateSymbol}
                grown={grownPanel === "product"}
                onGrow={() => setViewParameter("grow", grownPanel === "product" ? null : "product")}
              />
            ) : (
              <div className="empty-state"><h1>Choose a symbol</h1><p>Search or expand the local symbol tree. Semantic products load only after selection.</p></div>
            )}
          </main>
          <ResizableTrace run={run} navigateSymbol={navigateSymbol} grown={grownPanel === "trace"} onGrow={() => setViewParameter("grow", grownPanel === "trace" ? null : "trace")} onRerun={selected && product ? rerun : null} />
        </div>
      )}
    </div>
  );
}

function GrowablePanel({ id, className, title, children, grown, onToggle }: { id: string; className: string; title: string; children: React.ReactNode; grown: boolean; onToggle: () => void }) {
  return (
    <section id={id} className={`${className} panel ${grown ? "grown" : ""}`}>
      <div className="panel-heading"><span>{title}</span><button aria-label={`${grown ? "Restore" : "Grow"} ${title}`} onClick={onToggle}>{grown ? "↙" : "↗"}</button></div>
      {children}
    </section>
  );
}

function SymbolDirectory({ index, selected, navigateSymbol, query, onQuery }: { index: SymbolIndex; selected: string | null; navigateSymbol: (path: string) => void; query: string; onQuery: (query: string) => void }) {
  const [expanded, setExpanded] = useState(() => new Set([index.root]));
  const children = useMemo(() => {
    const map = new Map<string | null, SymbolSummary[]>();
    for (const symbol of index.symbols) {
      const siblings = map.get(symbol.parent) ?? [];
      siblings.push(symbol);
      map.set(symbol.parent, siblings);
    }
    return map;
  }, [index]);
  const matching = useMemo(() => {
    const needle = query.trim().toLowerCase();
    return needle ? index.symbols.filter((symbol) => symbol.search_text.toLowerCase().includes(needle)) : null;
  }, [index, query]);

  function toggle(path: string) {
    setExpanded((current) => {
      const next = new Set(current);
      if (next.has(path)) next.delete(path); else next.add(path);
      return next;
    });
  }

  return (
    <>
      <div className="search-box"><input aria-label="Filter symbols" type="search" placeholder="Filter symbols" value={query} onChange={(event) => onQuery(event.target.value)} /></div>
      <div className="filter-count">{matching ? `${matching.length} matches` : `${index.symbols.length} local symbols`}</div>
      <div className="symbol-tree">
        {(matching ?? children.get(null) ?? children.get(index.root) ?? []).map((symbol) => (
          <SymbolRow key={symbol.path} symbol={symbol} children={children} expanded={expanded} selected={selected} toggle={toggle} navigateSymbol={navigateSymbol} flat={matching !== null} />
        ))}
      </div>
    </>
  );
}

function SymbolRow({ symbol, children, expanded, selected, toggle, navigateSymbol, flat }: { symbol: SymbolSummary; children: Map<string | null, SymbolSummary[]>; expanded: Set<string>; selected: string | null; toggle: (path: string) => void; navigateSymbol: (path: string) => void; flat: boolean }) {
  const descendants = children.get(symbol.path) ?? [];
  const open = expanded.has(symbol.path);
  return (
    <div className="symbol-branch">
      <div className={`symbol-row ${selected === symbol.path ? "selected" : ""}`}>
        {descendants.length > 0 && !flat ? <button className="disclosure" aria-label={`${open ? "Collapse" : "Expand"} ${symbol.label}`} onClick={() => toggle(symbol.path)}>{open ? "▼" : "▶"}</button> : <span className="disclosure-spacer" />}
        <button className="symbol-link" onClick={() => navigateSymbol(symbol.path)}><span>{symbol.label}</span><small>{symbol.presentation.eyebrow}</small></button>
        {symbol.children.status === "incomplete" && (
          <span
            className="incomplete-children"
            role="img"
            aria-label={`Incomplete children for ${symbol.label}`}
            title={`${symbol.children.reason.code}: ${symbol.children.reason.message}`}
          >⚠</span>
        )}
      </div>
      {open && !flat && descendants.length > 0 && <div className="symbol-children">{descendants.map((child) => <SymbolRow key={child.path} symbol={child} children={children} expanded={expanded} selected={selected} toggle={toggle} navigateSymbol={navigateSymbol} flat={false} />)}</div>}
    </div>
  );
}

function SymbolDetail({ selected, activeProduct, product, navigateSymbol, grown, onGrow }: { selected: SelectedSymbol; activeProduct: string | null; product: ProductPage | null; navigateSymbol: (path: string, product?: string | null) => void; grown: boolean; onGrow: () => void }) {
  return (
    <>
      <div className="symbol-header">
        <div className="eyebrow">{selected.presentation.eyebrow}</div>
        <h1>{selected.label}</h1>
        <code>{selected.display_path}</code>
        <div className="badges">{selected.presentation.badges.map((badge) => <span key={badge.label} className={`badge ${badge.tone}`}>{badge.label}</span>)}</div>
        {selected.parent && <div className="parent-link">Parent: <SymbolLink target={selected.parent} navigateSymbol={navigateSymbol} /></div>}
      </div>
      <div className="product-tabs" role="tablist">{selected.products.map((descriptor) => <button role="tab" aria-selected={descriptor.id === activeProduct} className={descriptor.id === activeProduct ? "active" : ""} key={descriptor.id} onClick={() => navigateSymbol(selected.path, descriptor.id)}>{descriptor.label}</button>)}</div>
      <GrowablePanel id="product-panel" className="product-panel" title={product?.title ?? "Product"} grown={grown} onToggle={onGrow}>
        {product ? <RenderTree node={product.content} navigateSymbol={navigateSymbol} /> : <div className="loading-card">Loading product…</div>}
      </GrowablePanel>
    </>
  );
}

function SymbolLink({ target, navigateSymbol }: { target: SymbolReference; navigateSymbol: (path: string) => void }) {
  return <button className="semantic-link" onClick={() => navigateSymbol(target.path)}>{target.label} ↗</button>;
}

export function RenderTree({ node, navigateSymbol }: { node: RenderNode; navigateSymbol: (path: string) => void }) {
  switch (node.kind) {
    case "group": return <div className={`render-group ${node.layout}`}>{node.children.map((child, index) => <RenderTree key={index} node={child} navigateSymbol={navigateSymbol} />)}</div>;
    case "heading": {
      const Heading = `h${node.level}` as "h1" | "h2" | "h3";
      return <Heading>{node.text}</Heading>;
    }
    case "text": return <p>{node.text}</p>;
    case "code": return <pre><code>{node.text}</code></pre>;
    case "notice": return <aside className={`notice ${node.tone}`}>{node.title && <strong>{node.title}</strong>}<p>{node.text}</p></aside>;
    case "navigation": return <SymbolLink target={node.target} navigateSymbol={navigateSymbol} />;
    case "value": return <ValueTree node={node.value} navigateSymbol={navigateSymbol} />;
    default: return assertNever(node);
  }
}

export function ValueTree({ node, navigateSymbol, label }: { node: ValueNode; navigateSymbol: (path: string) => void; label?: string }) {
  switch (node.kind) {
    case "record": return <TreeContainer label={label} title={node.type_name}>{node.fields.map((field) => <ValueTree key={field.name} label={field.name} node={field.value} navigateSymbol={navigateSymbol} />)}</TreeContainer>;
    case "variant": return <TreeContainer label={label} title={`${node.enum_name}::${node.variant_name}`}>{node.fields.map((field) => <ValueTree key={field.name} label={field.name} node={field.value} navigateSymbol={navigateSymbol} />)}</TreeContainer>;
    case "sequence": return <TreeContainer label={label} title={`${node.type_name} [${node.items.length}]`}>{node.items.map((item, index) => <ValueTree key={index} label={String(index)} node={item} navigateSymbol={navigateSymbol} />)}</TreeContainer>;
    case "scalar": return <div className="value-leaf"><span className="field-name">{label}</span><span className="type-name">{node.type_name}</span><code>{String(node.value)}</code></div>;
    case "reference": return <div className="value-leaf"><span className="field-name">{label}</span><span className="type-name">Symbol</span><SymbolLink target={node.target} navigateSymbol={navigateSymbol} /></div>;
    case "cycle": return <div className="value-leaf"><span className="field-name">{label}</span><span className="type-name">cycle</span><code>↩ {node.identity}</code></div>;
    case "truncated": return <ContinuationNode node={node} label={label} navigateSymbol={navigateSymbol} />;
    default: return assertNever(node);
  }
}

function ContinuationNode({ node, label, navigateSymbol }: { node: Extract<ValueNode, { kind: "truncated" }>; label?: string; navigateSymbol: (path: string) => void }) {
  const [items, setItems] = useState<ValueNode[] | null>(null);
  const [failure, setFailure] = useState<string | null>(null);
  async function load() {
    if (!node.continuation) return;
    try {
      const response = await api.continuation(node.continuation);
      setItems(response.value.items);
    } catch (error) {
      setFailure(error instanceof Error ? error.message : String(error));
    }
  }
  if (items) {
    return <TreeContainer label={label} title={`continued ${node.summary}`}>{items.map((item, index) => <ValueTree key={index} label={String(index)} node={item} navigateSymbol={navigateSymbol} />)}</TreeContainer>;
  }
  return <div className="value-leaf truncated"><span className="field-name">{label}</span><span>{failure ?? node.summary}</span>{node.continuation && <button onClick={() => void load()}>Continue…</button>}</div>;
}

function TreeContainer({ label, title, children }: { label?: string; title: string; children: React.ReactNode }) {
  return <details className="value-node" open><summary><span className="field-name">{label}</span><span className="type-name">{title}</span></summary><div className="value-children">{children}</div></details>;
}

function ResizableTrace({ run, navigateSymbol, grown, onGrow, onRerun }: { run: RunObservation | null; navigateSymbol: (path: string) => void; grown: boolean; onGrow: () => void; onRerun: (() => void) | null }) {
  const [width, setWidth] = useState(360);
  const [filter, setFilter] = useState("");
  const [expansion, setExpansion] = useState({ version: 0, open: true });
  const dragStart = useRef<{ x: number; width: number } | null>(null);
  useEffect(() => {
    function move(event: PointerEvent) {
      if (!dragStart.current) return;
      setWidth(Math.max(280, Math.min(window.innerWidth - 400, dragStart.current.width + dragStart.current.x - event.clientX)));
    }
    function up() { dragStart.current = null; }
    window.addEventListener("pointermove", move);
    window.addEventListener("pointerup", up);
    return () => { window.removeEventListener("pointermove", move); window.removeEventListener("pointerup", up); };
  }, []);
  return (
    <aside className={`trace-panel panel ${grown ? "grown" : ""}`} style={{ width }}>
      <div className="resizer" onPointerDown={(event) => { dragStart.current = { x: event.clientX, width }; }} />
      <div className="panel-heading"><span>Execution tree</span><button aria-label={`${grown ? "Restore" : "Grow"} execution tree`} onClick={onGrow}>{grown ? "↙" : "↗"}</button></div>
      <div className="trace-controls"><input aria-label="Filter execution tree" type="search" placeholder="Filter queries and keys" value={filter} onChange={(event) => setFilter(event.target.value)} />{onRerun && <button onClick={onRerun}>Rerun</button>}<button onClick={() => setExpansion(({ version }) => ({ version: version + 1, open: true }))}>Expand all</button><button onClick={() => setExpansion(({ version }) => ({ version: version + 1, open: false }))}>Collapse all</button></div>
      {run ? <TraceTree key={expansion.version} node={run.root} filter={filter.toLowerCase()} navigateSymbol={navigateSymbol} defaultOpen={expansion.open} /> : <div className="empty-trace">Select a product to inspect its computation.</div>}
    </aside>
  );
}

function TraceTree({ node, filter, navigateSymbol, defaultOpen }: { node: TraceNode; filter: string; navigateSymbol: (path: string) => void; defaultOpen: boolean }) {
  const key = node.key.kind === "semantic" ? node.key.value : node.key.ingredient;
  const ownMatch = `${node.source} ${node.operation} ${key} ${node.disposition}`.toLowerCase().includes(filter);
  const childMatches = node.children.filter((child) => traceMatches(child, filter));
  if (filter && !ownMatch && childMatches.length === 0) return null;
  return <details className={`trace-node ${node.disposition}`} open={filter ? true : defaultOpen}><summary><span>{node.source}</span><strong>{node.operation}</strong>{(node.observations ?? 1) > 1 && <small>×{node.observations}</small>}<small>{node.disposition}</small></summary><button className="trace-key" onClick={() => key.startsWith("local/") || key.startsWith("external/") ? navigateSymbol(key) : undefined}>{key}</button><div className="trace-children">{(filter ? childMatches : node.children).map((child, index) => <TraceTree key={`${child.operation}-${index}`} node={child} filter={filter} navigateSymbol={navigateSymbol} defaultOpen={defaultOpen} />)}</div></details>;
}

function traceMatches(node: TraceNode, filter: string): boolean {
  if (!filter) return true;
  const key = node.key.kind === "semantic" ? node.key.value : node.key.ingredient;
  return `${node.source} ${node.operation} ${key} ${node.disposition}`.toLowerCase().includes(filter) || node.children.some((child) => traceMatches(child, filter));
}

function RevisionsView({ revision, navigate }: { revision: string | null; navigate: ReturnType<typeof useNavigate> }) {
  const [page, setPage] = useState<RevisionPage | null>(null);
  const [detail, setDetail] = useState<RevisionDetail | null>(null);
  const [failure, setFailure] = useState<string | null>(null);
  const [from, setFrom] = useState("");
  const [to, setTo] = useState("");
  const [symbol, setSymbol] = useState("");
  const [product, setProduct] = useState("");
  const [comparison, setComparison] = useState<RunComparison | null>(null);
  useEffect(() => {
    api.revisions().then((response) => {
      setPage(response.value);
      const latest = response.value.revisions[0]?.revision_id ?? "";
      const previous = response.value.revisions[1]?.revision_id ?? latest;
      setFrom(previous);
      setTo(latest);
    }).catch((error: InspectorError) => setFailure(error.message));
  }, []);
  useEffect(() => {
    const selected = revision ?? page?.revisions[0]?.revision_id;
    if (!selected) return;
    api.revisionDetail(selected).then((response) => setDetail(response.value)).catch((error: InspectorError) => setFailure(error.message));
  }, [page, revision]);
  return <main className="revisions-view">
    <h1>Revisions</h1>
    {failure && <div role="alert" className="error-banner">{failure}</div>}
    {!page ? <p>Loading revisions…</p> : <div className="revision-layout">
      <nav className="revision-list" aria-label="Retained revisions">{page.revisions.map((item) => <button className={detail?.summary.revision_id === item.revision_id ? "active" : ""} key={item.revision_id} onClick={() => navigate(`/revisions/${encodeURIComponent(item.revision_id)}`)}><strong>{item.revision_id}</strong><small>{revisionCauseLabel(item.cause)} · {item.input_delta_count} edits · {item.run_count} runs</small></button>)}</nav>
      {detail && <section className="revision-detail"><h2>{detail.summary.revision_id}</h2><p className="revision-cause">{revisionCauseLabel(detail.summary.cause)}</p><h3>Input changes</h3>{detail.input_deltas.length === 0 ? <p>No input changes.</p> : detail.input_deltas.map((delta) => <article key={`${delta.input.path}:${delta.input.field}`}><code>{delta.input.path}</code><pre>{delta.diff}</pre></article>)}<h3>Demanded work</h3>{detail.runs.length === 0 ? <p>No inspection work was requested in this revision.</p> : <ul>{detail.runs.map((run) => <li key={run}><code>{run}</code></li>)}</ul>}
        <h3>Compare inspected products</h3>
        <div className="comparison-form">
          <label>From<select aria-label="Compare from revision" value={from} onChange={(event) => setFrom(event.target.value)}>{page.revisions.map((item) => <option key={item.revision_id}>{item.revision_id}</option>)}</select></label>
          <label>To<select aria-label="Compare to revision" value={to} onChange={(event) => setTo(event.target.value)}>{page.revisions.map((item) => <option key={item.revision_id}>{item.revision_id}</option>)}</select></label>
          <label>Symbol<input aria-label="Comparison symbol" value={symbol} onChange={(event) => setSymbol(event.target.value)} /></label>
          <label>Product<input aria-label="Comparison product" value={product} onChange={(event) => setProduct(event.target.value)} /></label>
          <button disabled={!from || !to || !symbol || !product} onClick={() => void api.compare(from, to, symbol, product).then((response) => setComparison(response.value)).catch((error: InspectorError) => setFailure(error.message))}>Compare</button>
        </div>
        {comparison && <div className="comparison-result"><strong>{comparison.value_changed ? "Value changed" : "Value unchanged"}</strong><p>{comparison.executed_only_before.length} executed only before · {comparison.executed_only_after.length} executed only after</p><p>{comparison.reused_only_before.length} reused only before · {comparison.reused_only_after.length} reused only after</p></div>}
      </section>}
    </div>}
  </main>;
}

function revisionCauseLabel(cause: RevisionSummary["cause"]): string {
  switch (cause.kind) {
    case "initial": return "Initial workspace state";
    case "input-edit": return `Input edit ${cause.edit_batch}`;
    case "workspace-reload": return `Workspace reload: ${cause.reason.message}`;
    default: return assertNever(cause);
  }
}

function assertNever(value: never): never {
  throw new Error(`unknown protocol node: ${JSON.stringify(value)}`);
}
