#!/usr/bin/env python3
"""Build a standalone review packet from the committed implementation.

Usage: python3 docs/build-contracts-walkthrough.py
Excerpts and full-source views are extracted from the pinned Git revision.
The output requires no server, network, or JavaScript package installation.
When choosing another --revision, review the excerpt ranges and narrative.
"""
import argparse
import html
import json
from pathlib import Path
import subprocess

ROOT = Path(__file__).resolve().parents[1]
parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument("--revision", default="edde4b0")
parser.add_argument("--output", type=Path, default=ROOT / "docs/sage-contracts-walkthrough.html")
args = parser.parse_args()
revision = subprocess.check_output(["git", "rev-parse", "--verify", args.revision + "^{commit}"], cwd=ROOT, text=True).strip()
sources = {}
excerpts = []
def source(path):
    if path not in sources:
        sources[path] = subprocess.check_output(["git", "show", f"{revision}:{path}"], cwd=ROOT, text=True)
    return sources[path]
def esc(value):
    return html.escape(str(value), quote=True)
def ref(path, label, line=1):
    source(path)
    return f'<a href="../{esc(path)}" data-source="{esc(path)}" data-line="{line}">{esc(label)} <span aria-hidden="true">↗</span></a>'
def excerpt(path, start, end, title, collapsed=False):
    lines = source(path).splitlines()
    assert 1 <= start <= end <= len(lines), (path, start, end, len(lines))
    excerpts.append({"path":path,"start":start,"end":end})
    code = "".join(f'<span class="code-line"><span class="ln" aria-hidden="true">{i}</span><span>{esc(line) or " "}</span></span>' for i, line in enumerate(lines[start-1:end], start))
    contents = f'<div class="code-head"><span>{esc(path)}:{start}–{end}</span>{ref(path,"Read full snapshot",start)}</div><pre tabindex="0" aria-label="{esc(title)}"><code>{code}</code></pre>'
    if collapsed:
        return f'<details class="excerpt"><summary>{esc(title)}</summary>{contents}</details>'
    return f'<figure class="excerpt"><figcaption>{esc(title)}</figcaption>{contents}</figure>'
def note(title, text, kind="review"):
    return f'<div class="note {kind}"><strong>{title}</strong><p>{text}</p></div>'
def section(number, key, title, body):
    return f'<section id="{key}" class="chapter"><div class="kicker">{number:02d} / Review stop</div><h2>{title}</h2>{body}</section>'
def command(value):
    return f'<div class="command"><code>{esc(value)}</code><button type="button" data-copy="{esc(value)}">Copy</button></div>'

FIX = "crates/sage-meta/tests/fixtures/contracts/"
META = "crates/sage-meta/src/"
TEST = FIX + "src/tests.rs"
PROD = "crates/sage-test-harness/tests/architecture_contracts.rs"
MODEL = FIX + "sage-src/sage_contract_fixture/nodes.sage"
QUERY = FIX + "src/queries.rs"
traces = {key: source(FIX + f"expected/{key}.txt").splitlines() for key in ["cold", "warm", "validated"]}
chapters = []
chapters.append(section(1,"contract","Start with three operations",f'''
<p>Read this small test model first. <code>Root::run</code> may call <code>Helper::work</code>, and forbids reaching <code>Leaf::read</code> through any nonempty call path. Helper has an empty envelope.</p>
{excerpt(MODEL,3,19,"A valid model with a transitive prohibition")}
<p>The model is consistent because the permitted graph never reaches Leaf. The Rust fixture will deliberately take an undeclared edge. This lets us test runtime failure without making the model itself invalid.</p>
{note("Review the abstraction", "An <code>invokes</code> entry is permission, not a required call or execution order. An empty envelope makes Helper a leaf at this architectural level; it can still perform internal work and read fixture inputs.")}
<p class="dig">Dig deeper: {ref(FIX+"sage-src/sage_contract_fixture/crate.sage","Fixture crate manifest")} · {ref("sage-src/sage_ir/symbols.sage","Production LocalFnSym model",69)}.</p>
'''))
chapters.append(section(2,"mapping","Give Rust declarations architectural names",f'''
<p>The attributes are inert metadata. Rust accepts the registered tool namespace; <code>sage-meta</code> reads the unexpanded source to interpret the payload. No Sage procedural macro wraps these queries.</p>
{excerpt(FIX+"src/lib.rs",1,2,"Register the tool namespace")}
{excerpt(QUERY,21,45,"Deliberately different Rust and Sage names")}
<table><caption>The identities to keep in mind</caption><thead><tr><th>Rust declaration</th><th>Sage operation</th></tr></thead><tbody><tr><td><code>start</code></td><td><code>Root::run</code></td></tr><tr><td><code>Input::compute</code></td><td><code>Helper::work</code></td></tr><tr><td><code>forbidden</code></td><td><code>Leaf::read</code></td></tr></tbody></table>
<p>Notice that two tracked methods share an <code>impl</code>, yet have different mappings. Architectural identity cannot be recovered just by comparing method names or using the enclosing impl’s location.</p>
{excerpt(QUERY,3,8,"Module anchors for concepts without one concrete implementation",True)}
{note("Source correspondence has its own meaning", "An annotated empty module gives a concept a source location. It does not create an executable boundary or instrument all its descendants. One Sage item may have several Rust sites.")}
<p class="dig">Compare with the production {ref("crates/sage-ir/src/local_syms/fns.rs","LocalFnSym::signature → sig annotation",118)} and {ref("crates/sage-ir/src/local_syms/fns.rs","body annotation",237)}.</p>
'''))
chapters.append(section(3,"index","Check correspondence in both directions",f'''
<p><code>CorrespondenceIndex::load</code> uses Cargo ownership and parses Rust syntax. Its index records the Sage identity, declaration kind, Rust path, file span, target, and cfg conditions. The validator then joins it to the model’s crate/module headers, declarations, and explicit methods.</p>
{excerpt(META+"correspondence.rs",357,386,"Missing anchors, wrong crates, and dangling annotations")}
<p>Kind checks follow: a method may map to a function, a method, a trait method, or a module anchor. The report separates source sites from declarations with executable bodies. A cfg inventory is not a claim that every alternative exists in this binary.</p>
{command("cargo run -p sage-meta -- check --correspondence --json")}
{note("What a green audit establishes", "At this revision, all 194 model units have anchors across 203 Rust sites. Of those sites, 52 have function bodies. This validates correspondence; it does not prove that every compiler call follows its envelope.")}
<p>When a query is observed, {ref(META+"correspondence.rs","lookup",251)} accepts one matching executable declaration at its original file/line. Missing and ambiguous matches are failures. An unannotated nested function cannot borrow its enclosing function’s Sage identity.</p>
<p class="dig">Review {ref("crates/sage-meta/tests/correspondence.rs","source-index tests")} for cfg alternatives, declaration kinds, nested functions, known generators, and rejected conditional annotations.</p>
'''))
chapters.append(section(4,"engine","Check one architectural stack",f'''
<p><code>ContractModel</code> resolves the same names, inheritance, and virtual call targets as the static validator. A checking session owns an explicit set of allowed roots, its active invocations, a trace, violations, and coverage gaps.</p>
{excerpt(META+"validate.rs",25,36,"Static validation and runtime contracts share the resolver")}
{excerpt(META+"contracts.rs",216,226,"Entry: direct permission, ancestor restrictions, then a new frame")}
<p>A source request is checked against the active caller. Every observed request or internal reexecution is also checked against all active ancestors’ prohibitions. Entering the same operation again counts as recursion.</p>
{excerpt(META+"contracts.rs",289,308,"Collect prohibitions from every active ancestor",True)}
<p>A <code>self</code> permission requires matching receiver identities. Different receivers produce a violation; missing identity produces a coverage gap. The result retains the offending call path and the rule’s location in the model.</p>
{excerpt(META+"contracts.rs",403,408,"A report cannot pass with a violation or observation gap")}
{note("Review the success criterion", "<code>calls_conform</code> is scoped to observed boundaries. Reads and context creation are recorded as unchecked effects. A warm memo’s historical dependency graph is not replayed.")}
<p class="dig">{ref(META+"contracts.rs","Receiver checks",230)} · {ref("crates/sage-meta/tests/runtime_contracts.rs","Recursion, dispatch, receiver, and validation tests")}.</p>
'''))
chapters.append(section(5,"salsa","Join Salsa events to that stack",f'''
<p>Salsa’s query definition supplies original source provenance. The observer matches that location against the Rust index and caches the result by ingredient identity. Query debug names are not used for matching.</p>
{excerpt("vendor/salsa/src/event.rs",34,39,"Source provenance comes from the compiled query configuration")}
{excerpt(META+"salsa_contracts.rs",90,103,"Pair loaded Rust with a compiled source snapshot")}
<p>The tests supply <code>include_str!</code> snapshots of the relevant Rust files. Reading two current disk copies would not establish compatibility with the running binary. A stale or unpaired source cannot silently pass.</p>
{excerpt(META+"salsa_contracts.rs",154,169,"Source location → executable Rust site → Sage operation",True)}
<p><strong>A request and an execution are different events.</strong> A cached request still needs permission. Conversely, Salsa can reexecute a dependency during incremental validation without a new source-level call to that dependency.</p>
{excerpt(META+"salsa_contracts.rs",253,275,"Avoid a duplicate frame; establish a validation frame when needed",True)}
{note("The two stacks", "The raw lifecycle stack verifies request/execution nesting. The architectural stack checks modeled operations. A normal request plus its execution produces one architectural invocation. Internal reexecution establishes its own context and still inherits active prohibitions.")}
<p>The production database forwards {ref("crates/sage-ir/src/db.rs","raw events before display-log coalescing",163)}. RAII guards balance execution events on unwind. The vendored macro change preserves each tracked method’s source span, so methods in one impl remain distinguishable.</p>
<p class="dig">{ref("vendor/salsa/src/function/execute.rs","ExecutionGuard",18)} · {ref(META+"salsa_contracts.rs","Lifecycle consistency checks",225)} · {ref("vendor/salsa-macros/src/tracked_impl.rs","Method-span fix",121)} · {ref("vendor/salsa-macros/SAGE_FORK.md","Small upstream patch explained")}.</p>
'''))
chapters.append(section(6,"traversals","Step through the reviewed traversals",f'''
<p>These are the fixture’s checked-in expected traces, asserted against real Salsa events. The controls replay those expectations; they do not run the compiler. Each line names both a Sage operation and its independently reviewed Rust declaration.</p>
<div class="trace-panel">
<div class="trace-controls"><label for="scenario">Scenario</label><select id="scenario"><option value="cold">Cold computation</option><option value="warm">Warm cache reuse</option><option value="validated">Incremental validation</option></select><button id="previous" type="button" aria-label="Previous trace event">← Previous</button><button id="next" type="button" aria-label="Next trace event">Next →</button><span id="step-count"></span></div>
<p id="scenario-description"></p>
<div class="trace-columns"><div><p class="label">Recorded expectation · select an event</p><ol id="trace-lines"></ol></div><div class="stack-panel"><p class="label">Architectural stack after this event</p><ol id="frames"></ol><p id="event-explanation" aria-live="polite"></p></div></div>
<p class="trace-source" id="trace-source"></p>
</div>
<noscript><p>The interactive replay needs JavaScript. The exact expected traces are also available below.</p></noscript>
{excerpt(FIX+"expected/cold.txt",1,8,"Cold expectation",True)}
{excerpt(FIX+"expected/warm.txt",1,2,"Warm expectation",True)}
{excerpt(FIX+"expected/validated.txt",1,4,"Validation expectation",True)}
<p>The validation case changes the input from 10 to 11. Helper computes integer division by two, so its result stays 5; Root can remain cached. Helper’s execution appears without a new Helper request. That is the behavior the validation frame represents.</p>
<p class="dig">Read the {ref(TEST,"test that checks all three traversals",126)}.</p>
'''))
chapters.append(section(7,"forbidden","Inspect the cached forbidden call",f'''
<p>The fixture flips <code>violate</code> to make Helper call <code>forbidden</code>. Two separate rules fail: Helper never declared permission to call Leaf, and Root forbids Leaf transitively.</p>
{excerpt(FIX+"expected/forbidden.txt",1,2,"Both diagnostics retain the complete path")}
{excerpt(TEST,155,167,"Exercise both a cold leaf and a prewarmed leaf")}
{excerpt(TEST,203,214,"Prove the cached leaf is requested without executing")}
{note("The precise guarantee", "The cached Leaf request is checked even though its body does not execute. This does not establish the hidden descendants of an entirely reused parent memo. The test deliberately forces Helper to make the leaf request in the observed session.","important")}
<p>Violations are retained as data. The adapter does not panic inside Salsa callbacks. Callers enforce a checked run with <code>assert_calls_conform()</code> at the report boundary; this negative fixture instead asserts the expected violations. This preserves both diagnostics and lets query unwinding close the active frames.</p>
<p class="dig">Review {ref(TEST,"missing or swapped annotations",269)}, {ref(TEST,"stale or ambiguous sources",301)}, {ref(TEST,"panic cleanup",219)}, and {ref(TEST,"malformed lifecycle replay",395)} for ways the checker must refuse success.</p>
'''))
chapters.append(section(8,"compiler","Finish at the real compiler boundary",f'''
<p>The production test loads the actual corpus and Rust index once per test process, supplies compiled snapshots, and arms the observer after fixture setup. The empty function’s signature passes on both cold and reused requests, with its exact Rust declaration checked.</p>
{excerpt(PROD,20,41,"Explicit roots and cached model/source loading",True)}
{excerpt(PROD,131,143,"Check the production Rust/Sage correspondence")}
<p>The caller-body test goes further: it asserts this exact request sequence and proves that caller and callee signature requests have different keys.</p>
<ol class="sequence"><li>Caller’s <code>LocalFnSym::body</code></li><li>Caller’s <code>LocalFnSym::signature</code></li><li><code>LocalModSym::expanded_items</code></li><li>Callee’s <code>LocalFnSym::signature</code></li></ol>
{excerpt(PROD,201,219,"The body test requires an honest incomplete report")}
<p>The body does not yet conform under this observation: ordinary <code>BodyCheck</code> and <code>Resolver</code> frames are absent, so a module request appears directly under the body; receiver identity is also unavailable. The test preserves these findings and independently asserts that no callee body was requested.</p>
{note("The next implementation decision", "Choose how ordinary methods emit boundaries and how receiver identity crosses those boundaries. The current Salsa adapter supplies <code>None</code> for receivers. Async/task propagation, reads/creates enforcement, full body conformance, and agent transports are still staged work.","important")}
<p class="dig">{ref(PROD,"Exact body traversal assertions",162)} · {ref(PROD,"No callee-body assertion",227)} · {ref("md/rfds/sage-contracts/implementation.md","Completed work and remaining stages")}.</p>
'''))

run = f'''
<section id="run" class="chapter"><div class="kicker">Try it locally</div><h2>Reproduce the evidence</h2>
<p>Run from the repository root. The fixture contains deliberately invalid runtime behavior; a passing test means the expected violations were detected.</p>
{command("cargo run -p sage-meta -- check --correspondence")}
{command("cargo test -p sage-meta --test contracts")}
{command("cargo test -p sage-test-harness --test architecture_contracts")}
{command("cargo run -p sage-meta -- serve 3001")}
<p>The final command serves the existing architecture explorer at <a href="http://127.0.0.1:3001/">127.0.0.1:3001</a>. It browses declarations and their source links; it does not yet replay runtime reports.</p>
<div class="note review"><strong>Questions worth discussing after reading</strong><ul><li>Do the envelopes capture the boundaries you want to enforce?</li><li>Are the Rust declarations the right correspondence sites?</li><li>Does the request/execution distinction match your expectations?</li><li>Is the next priority ordinary-function coverage, or richer trace presentation?</li></ul></div>
<p class="dig">{ref("md/design/validation/architecture-language.md","Architecture and current limitations")} · {ref("md/rfds/sage-contracts/README.md","Design rationale")} · {ref("crates/sage-meta/tests/fixtures/contracts/README.md","Fixture guide")}.</p>
</section>
'''

css = r'''
:root{color-scheme:light;--paper:#f5f4ef;--surface:#fff;--ink:#20302d;--muted:#596b64;--line:#dbe1da;--accent:#1e6655;--soft:#edf5ef;--code:#f7f9f7;--warn:#8a4728}*{box-sizing:border-box}html{scroll-behavior:smooth;scroll-padding-top:24px}body{margin:0;background:var(--paper);color:var(--ink);font:16px/1.65 system-ui,-apple-system,BlinkMacSystemFont,"Segoe UI",sans-serif}a{color:var(--accent);text-decoration-thickness:1px;text-underline-offset:3px}a:hover{color:#123f35}button,select{font:inherit}button{cursor:pointer;background:var(--surface);color:var(--ink);border:1px solid #c8d4cb;border-radius:7px;padding:6px 12px}button:hover{background:var(--soft)}button:disabled{opacity:.45;cursor:default}a:focus-visible,button:focus-visible,select:focus-visible,summary:focus-visible,pre:focus-visible,#source-body:focus-visible{outline:3px solid #b4762f;outline-offset:3px}code{font: .88em/1.55 ui-monospace,SFMono-Regular,Consolas,monospace;overflow-wrap:anywhere}p{margin:0 0 18px}h1,h2,h3{line-height:1.18;letter-spacing:-.035em}h1{font-size:clamp(34px,4.8vw,57px);max-width:820px;margin:15px 0 22px;font-weight:650}h2{font-size:31px;margin:9px 0 23px;font-weight:640}h3{font-size:21px}strong{font-weight:650}.shell{display:grid;grid-template-columns:230px minmax(0,1fr);max-width:1500px;margin:auto}.rail{position:sticky;top:0;height:100vh;padding:35px 22px;overflow:auto;border-right:1px solid var(--line)}.brand{font-weight:750;letter-spacing:.17em;font-size:14px}.rail-sub{font-size:13px;color:var(--muted);margin:8px 0 25px}.rail ol{padding:0;list-style:none;margin:0}.rail a{display:block;font-size:14px;text-decoration:none;border-left:2px solid transparent;padding:9px 12px;color:var(--muted)}.rail a.active{border-color:var(--accent);color:var(--accent);background:#e7eee7;border-radius:0 6px 6px 0}.rail .num{font-size:11px;display:block;letter-spacing:.12em;opacity:.8}.rail-footer{font-size:12px;color:var(--muted);margin-top:25px}main{min-width:0;max-width:1130px;padding:58px 55px 55px}.hero{padding-bottom:36px}.kicker{text-transform:uppercase;font-size:11px;letter-spacing:.17em;font-weight:750;color:var(--accent)}.lede{max-width:750px;font-size:20px;line-height:1.55;color:#445b50}.meta{font-size:13px;color:var(--muted)}.pill{display:inline-block;border:1px solid var(--line);padding:3px 10px;margin-right:8px;border-radius:20px}.map{display:grid;grid-template-columns:repeat(4,minmax(0,1fr));gap:10px;margin:28px 0}.map a{padding:16px;background:var(--surface);border:1px solid var(--line);border-radius:9px;text-decoration:none;font-size:14px}.map small{display:block;color:var(--muted);line-height:1.4;margin-top:7px}.scope{width:100%;border-collapse:collapse;background:var(--surface);font-size:14px;margin-top:25px}.scope th{width:25%;text-align:left}.scope td,.scope th{padding:14px;border-bottom:1px solid var(--line)}.chapter{padding:43px 0 30px;border-top:1px solid #cdd6ce;scroll-margin-top:18px}.chapter>p{max-width:860px}.excerpt{margin:24px 0;background:var(--surface);border:1px solid var(--line);border-radius:10px;overflow:hidden;min-width:0}figcaption,summary{padding:12px 17px;font-size:14px;font-weight:650;background:#eef2ed}summary{cursor:pointer}summary::marker{color:var(--accent)}.code-head{display:flex;justify-content:space-between;align-items:baseline;gap:20px;padding:10px 17px;font:11px/1.5 ui-monospace,monospace;color:var(--muted);border-bottom:1px solid var(--line)}.code-head>span{overflow-wrap:anywhere}.code-head a{flex-shrink:0;font:12px/1.4 system-ui}pre{margin:0;background:var(--code);overflow:auto;padding:14px 0;font-size:12px;line-height:1.6;tab-size:4}pre code{font-size:inherit;line-height:inherit;overflow-wrap:normal}.code-line{display:flex;min-width:max-content;padding-right:20px}.ln{display:inline-block;flex:0 0 4.3em;text-align:right;padding-right:1.5em;color:#7f9185;user-select:none}.note{padding:19px 23px;margin:24px 0;border-left:3px solid var(--accent);background:#eaf2ec;border-radius:0 9px 9px 0}.note>strong{font-size:14px}.note p{margin:5px 0 0;font-size:15px}.note.important{background:#f6eee2;border-color:#a56534}.dig{font-size:14px;color:var(--muted)}table{width:100%;border-collapse:collapse;margin:20px 0;font-size:14px}caption{text-align:left;font-size:13px;font-weight:650;margin-bottom:8px}th,td{text-align:left;vertical-align:top;padding:10px 12px;border-bottom:1px solid var(--line)}thead th{background:#eaf0e9}.command{display:flex;align-items:center;justify-content:space-between;gap:15px;padding:13px 15px;border:1px solid var(--line);border-radius:8px;background:var(--surface);margin:12px 0}.command code{font-size:13px}.command button{font-size:12px}.trace-panel{background:var(--surface);border:1px solid var(--line);border-radius:12px;padding:22px;margin:25px 0}.trace-controls{display:flex;align-items:center;gap:9px;flex-wrap:wrap}.trace-controls label{font-size:13px;font-weight:650}select{padding:7px 10px;border:1px solid #c8d4cb;border-radius:6px;background:var(--surface);color:var(--ink);font-size:14px}.trace-controls button{font-size:13px}#step-count{font-size:12px;color:var(--muted)}#scenario-description{font-size:14px;margin:19px 0;color:var(--muted)}.trace-columns{display:grid;grid-template-columns:1.6fr 1fr;gap:20px}.label{text-transform:uppercase;font-size:10px;letter-spacing:.1em;font-weight:750;margin-bottom:10px;color:var(--muted)}#trace-lines{padding:0;list-style:none;margin:0}#trace-lines button{text-align:left;display:block;width:100%;font:12px/1.6 ui-monospace,monospace;white-space:normal;overflow-wrap:anywhere;border:1px solid transparent;border-radius:5px;padding:8px 10px}#trace-lines button.selected{background:#e1efe5;border-color:#92b6a2;color:#174734}.stack-panel{background:#f2f5ef;padding:15px;border-radius:8px;align-self:start}.stack-panel ol{padding-left:21px;font-size:14px}.stack-panel li{padding:6px 0}#event-explanation{font-size:13px;margin:16px 0 0;color:var(--muted)}.validation{color:var(--warn);font-size:11px;display:block}.trace-source{font-size:12px;margin:18px 0 0}.sequence{display:grid;grid-template-columns:repeat(2,minmax(0,1fr));gap:8px 25px;padding-left:25px}.sequence li{padding:10px;background:var(--surface);border-radius:5px;font-size:14px}.skip{position:absolute;left:-9999px}.skip:focus{left:10px;top:10px;z-index:100;background:white;padding:10px}.footer{font-size:12px;color:var(--muted);padding-top:20px;border-top:1px solid var(--line)}dialog{width:min(1240px,94vw);height:90vh;padding:0;border:1px solid #9aafa0;border-radius:12px;box-shadow:0 20px 100px #102a2640;background:var(--surface);color:var(--ink)}dialog::backdrop{background:#10252188}.dialog-top{padding:18px 23px;display:flex;align-items:flex-start;justify-content:space-between;gap:20px;border-bottom:1px solid var(--line)}.dialog-top h2{font-size:17px;letter-spacing:0;margin:5px 0;overflow-wrap:anywhere}#source-context{font-size:12px;color:var(--muted);margin:0}#source-body{height:calc(90vh - 118px);overflow:auto;background:var(--code)}#source-body pre{min-height:100%}#source-body .highlight{background:#dcebdc}#copy-status{position:fixed;bottom:18px;right:22px;background:#203f32;color:white;border-radius:8px;padding:8px 14px;font-size:13px;z-index:20}#copy-status:empty{display:none}@media(max-width:1100px){main{padding:45px 28px}.shell{grid-template-columns:205px minmax(0,1fr)}.rail{padding:28px 14px}.trace-columns{grid-template-columns:1fr}.map{grid-template-columns:repeat(2,1fr)}}@media(max-width:720px){.shell{display:block}.rail{position:static;height:auto;border:0;border-bottom:1px solid var(--line);padding:20px}.rail ol{display:grid;grid-template-columns:repeat(2,1fr)}.rail a{padding:5px 9px;font-size:12px}.rail .num{display:inline;margin-right:6px}.rail-sub{margin:5px 0 12px}.rail-footer{display:none}main{padding:32px 20px}.chapter{padding-top:32px}h2{font-size:27px}.code-head{display:block}.code-head a{display:inline-block;margin-top:7px}.sequence{grid-template-columns:1fr}.scope th{width:33%}.scope td,.scope th{padding:10px}.command{align-items:flex-start}.dialog-top{padding:13px}#source-body{height:calc(90vh - 140px)}}@media(prefers-reduced-motion:reduce){html{scroll-behavior:auto}}@media print{.rail,.trace-controls,dialog,.command button,.code-head a{display:none!important}.shell{display:block}main{max-width:none;padding:0}.chapter{break-before:page}pre{white-space:pre-wrap}.excerpt{break-inside:avoid}body{background:white}.trace-columns{display:block}.map{display:none}}
'''

js = r'''
'use strict';
const packet=JSON.parse(document.getElementById('packet').textContent);
const sourceDialog=document.getElementById('source-dialog');
const escapeHtml=text=>String(text).replace(/[&<>"']/g,c=>({'&':'&amp;','<':'&lt;','>':'&gt;','"':'&quot;',"'":'&#39;'}[c]));
function openSource(path,line=1){
  const source=packet.sources[path];
  if(source===undefined)return;
  document.getElementById('source-title').textContent=path;
  document.getElementById('source-context').textContent=`Committed snapshot ${packet.revision.slice(0,7)} · line ${line} · complete file`;
  const working=document.getElementById('working-file');
  working.href='../'+path;
  document.getElementById('source-body').innerHTML='<pre><code>'+source.split('\n').map((text,i)=>`<span class="code-line ${i+1===line?'highlight':''}" data-source-line="${i+1}"><span class="ln" aria-hidden="true">${i+1}</span><span>${escapeHtml(text)||' '}</span></span>`).join('')+'</code></pre>';
  if(!sourceDialog.open)sourceDialog.showModal();
  requestAnimationFrame(()=>document.querySelector(`[data-source-line="${line}"]`)?.scrollIntoView({block:'center'}));
}
document.addEventListener('click',event=>{
  const sourceLink=event.target.closest('[data-source]');
  if(sourceLink){event.preventDefault();openSource(sourceLink.dataset.source,Number(sourceLink.dataset.line)||1);}
});
document.getElementById('close-source').addEventListener('click',()=>sourceDialog.close());
sourceDialog.addEventListener('click',event=>{if(event.target===sourceDialog){const r=sourceDialog.getBoundingClientRect();if(event.clientX<r.left||event.clientX>r.right||event.clientY<r.top||event.clientY>r.bottom)sourceDialog.close();}});
let copyTimer;
document.addEventListener('click',async event=>{
  const button=event.target.closest('[data-copy]');if(!button)return;
  const status=document.getElementById('copy-status');
  try{await navigator.clipboard.writeText(button.dataset.copy);status.textContent='Command copied';}catch{status.textContent='Select the command text to copy it.';}
  clearTimeout(copyTimer);copyTimer=setTimeout(()=>status.textContent='',3000);
});
const descriptions={
  cold:'Root and Helper both execute. Request and execution lifetimes are nested, but each operation has one architectural frame.',
  warm:'Only Root is requested. Its memo is reused, so neither Root nor Helper executes. This trace does not replay the memo’s historical descendants.',
  validated:'The input changes from 10 to 11. Helper reexecutes during validation and still returns 5, so Root’s existing result remains valid.'
};
let scenario='cold',step=0;
function architecturalFrames(lines,through){
  const frames=[],executions=[];
  for(const line of lines.slice(0,through+1)){
    const [action,operation]=line.split(' ');
    if(action==='request')frames.push({operation,validation:false});
    if(action==='execute'){
      const added=frames.at(-1)?.operation!==operation;
      executions.push(added);
      if(added)frames.push({operation,validation:true});
    }
    if(action==='executed'&&executions.pop())frames.pop();
    if(action==='return')frames.pop();
  }
  return frames;
}
function renderTrace(){
  const lines=packet.traces[scenario];
  document.getElementById('scenario-description').textContent=descriptions[scenario];
  document.getElementById('step-count').textContent=`${step+1} / ${lines.length}`;
  document.getElementById('trace-lines').innerHTML=lines.map((line,i)=>`<li><button type="button" data-step="${i}" ${i===step?'class="selected" aria-current="step"':''}>${escapeHtml(line)}</button></li>`).join('');
  const frames=architecturalFrames(lines,step);
  document.getElementById('frames').innerHTML=frames.length?frames.map(frame=>`<li><code>${escapeHtml(frame.operation)}</code>${frame.validation?'<span class="validation">internal validation context</span>':''}</li>`).join(''):'<li>No active invocation.</li>';
  const [action,operation]=lines[step].split(' ');
  let explanation={request:'Check the caller’s permission (or the explicit root allowlist) and active ancestor prohibitions, then enter the operation.',execute:'This execution belongs to the existing request. No second architectural invocation is added.',executed:'Execution ended; its request remains active until the return event.',return:'The request ends. Its architectural frame is removed; accumulated violations remain in the report.'}[action];
  if(scenario==='validated'&&operation==='Helper::work')explanation=action==='execute'?'No Helper source request is active. Establish a validation context and check ancestor prohibitions without inventing a direct call.':'The internal Helper execution has finished. Pop its validation context and resume Root’s pending request.';
  document.getElementById('event-explanation').textContent=explanation;
  document.getElementById('previous').disabled=step===0;
  document.getElementById('next').disabled=step===lines.length-1;
  const path=packet.fixture+`expected/${scenario}.txt`;
  document.getElementById('trace-source').innerHTML=`Exact expectation: <a href="../${escapeHtml(path)}" data-source="${escapeHtml(path)}">${escapeHtml(scenario)}.txt ↗</a>. Stack display is derived from these events.`;
}
document.getElementById('scenario').addEventListener('change',event=>{scenario=event.target.value;step=0;renderTrace();});
document.getElementById('previous').addEventListener('click',()=>{if(step>0){step--;renderTrace();}});
document.getElementById('next').addEventListener('click',()=>{if(step<packet.traces[scenario].length-1){step++;renderTrace();}});
document.getElementById('trace-lines').addEventListener('click',event=>{const button=event.target.closest('[data-step]');if(button){step=Number(button.dataset.step);renderTrace();}});
renderTrace();
if('IntersectionObserver' in window){
  const observer=new IntersectionObserver(entries=>{for(const entry of entries)if(entry.isIntersecting){for(const link of document.querySelectorAll('.rail nav a')){const active=link.hash==='#'+entry.target.id;link.classList.toggle('active',active);if(active)link.setAttribute('aria-current','location');else link.removeAttribute('aria-current');}}},{rootMargin:'-8% 0px -68% 0px'});
  document.querySelectorAll('main section[id]').forEach(section=>observer.observe(section));
}
'''

nav=[("contract","Three-operation model"),("mapping","Rust annotations"),("index","Correspondence index"),("engine","Contract checking"),("salsa","Salsa observation"),("traversals","Trace replay"),("forbidden","Forbidden cached call"),("compiler","Real compiler boundary")]
nav_html=''.join(f'<li><a href="#{key}"><span class="num">{i:02d}</span>{title}</a></li>' for i,(key,title) in enumerate(nav,1))
data=json.dumps({"revision":revision,"sources":sources,"traces":traces,"fixture":FIX,"excerpts":excerpts},ensure_ascii=False).replace('&','\u0026').replace('<','\u003c').replace('>','\u003e')
page=f'''<!doctype html>
<html lang="en"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width, initial-scale=1"><meta name="color-scheme" content="light"><title>Sage contracts · a source walkthrough</title><style>{css}</style></head>
<body><a class="skip" href="#main">Skip to walkthrough</a><div class="shell">
<aside class="rail"><div class="brand">SAGE / FIELD NOTES</div><p class="rail-sub">Contracts &amp; correspondence<br>A guided source review</p><nav aria-label="Walkthrough stops"><ol>{nav_html}<li><a href="#run"><span class="num">RUN</span>Reproduce the evidence</a></li></ol></nav><p class="rail-footer">Implementation snapshot<br><code>{revision[:7]}</code><br>About 20 minutes<br>Works offline</p></aside>
<main id="main"><header class="hero"><div class="kicker">Implementation walkthrough · eight stops</div><h1>From a .sage rule<br>to a checked call.</h1><p class="lede">Follow a small fixture through Rust annotations, source matching, Salsa events, and contract enforcement. Then inspect what the real compiler tests establish—and what still needs wiring.</p><p class="meta"><span class="pill">Source excerpts included</span><span class="pill">Full files one click away</span><span class="pill">Snapshot {revision[:7]}</span></p>
<div class="map"><a href="#contract">1. Declare<small>.sage intent + envelope</small></a><a href="#mapping">2. Correspond<small>Rust declaration + Sage ID</small></a><a href="#salsa">3. Observe<small>Salsa request / execution</small></a><a href="#engine">4. Check<small>Permissions + ancestor rules</small></a></div>
<table class="scope"><caption>Three claims to keep separate while reviewing</caption><tbody><tr><th>Model consistency</th><td>The declared graph respects its own rules.</td></tr><tr><th>Source correspondence</th><td>Sage items have compatible Rust anchors, and annotations resolve in the right crate.</td></tr><tr><th>Observed conformance</th><td>The supplied runtime boundaries respect their contracts, with no reported observation gaps.</td></tr></tbody></table>
<p class="meta" style="margin-top:16px">The first two cover the current model. Runtime enforcement is proven on isolated fixtures and a narrow production signature scope; full compiler-body coverage is unfinished.</p></header>
{''.join(chapters)}{run}
<footer class="footer">Excerpts and complete source snapshots were extracted directly from Git commit <code>{revision}</code>. “Read full snapshot” works without the repository or a network connection. “Working-tree file” opens the adjacent repository copy, which may differ after edits. Regenerate this packet with <code>python3 docs/build-contracts-walkthrough.py</code>.</footer></main></div>
<dialog id="source-dialog" aria-labelledby="source-title"><div class="dialog-top"><div><div class="kicker">Full source snapshot</div><h2 id="source-title"></h2><p id="source-context"></p><a id="working-file" target="_blank" rel="noopener">Working-tree file ↗</a></div><button id="close-source" type="button" autofocus>Close ×</button></div><div id="source-body" tabindex="0" role="region" aria-label="Source code snapshot"></div></dialog>
<div id="copy-status" role="status" aria-live="polite"></div><script id="packet" type="application/json">{data}</script><script>{js}</script></body></html>
'''
args.output.parent.mkdir(parents=True,exist_ok=True)
args.output.write_text(page)
print(f"Wrote {args.output} ({len(page.encode()):,} bytes; {len(excerpts)} excerpts; {len(sources)} source files; revision {revision[:7]})")
