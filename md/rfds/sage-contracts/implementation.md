# Implementation plan and status

**Accepted; first infrastructure implemented.** Correspondence migration is
complete for the current corpus. Default `check --json` reports model,
Rust-correspondence, specification-test, and saved-review checks separately, with source
coverage independent from executable declarations. A public library check is
shared by CLI, CI, and a workspace Cargo test. The isolated Salsa fixture
and explicit-event tests establish the initial runtime checker. The canonical
rule index, Rust implementation references, and anchor pages are implemented.
Broad compiler acceptance, automatic session initialization, async propagation,
browser replay, and agent transports remain staged work. Checked boxes describe
completed work, not blanket completion of their containing stage.

Evidence lives in `crates/sage-meta/tests/correspondence.rs`, `cli.rs`,
`runtime_contracts.rs`, and `contracts.rs` plus its isolated fixture and reviewed
`expected/` files. Reproduce the core checks with `cargo test -p sage-meta`.

## Shared build-time check and specification anchors

- [x] Expose `sage_meta::check(workspace_root)` and a report whose
      `ensure_valid()` rejects model, correspondence, or specification errors.
      Keep full checking of a selected model directory through `check_model`;
      retain model-only loading/validation and the legacy positional CLI.
- [x] Make `cargo run -p sage-meta -- check` run the shared checks by default,
      invoke it in the CI runner, and call the library from a Cargo integration
      test with a manifest-relative workspace path.
- [x] Parse named specification paragraphs and inert `#[sage::test(...)]`
      references. Reject malformed/duplicate anchors, uncovered rules, and
      dangling or misplaced references; exclude Markdown code examples.
- [x] Render named paragraphs in mdBook using the same parser. Cover the
      architectural-language chapter's class, model-path, and runtime-call
      rules with focused language/runtime tests.

**Evidence boundary:** Eligible evidence consists of non-ignored `#[test]`
functions in Cargo targets with tests and the standard harness enabled. These
checks inventory declarations across cfg alternatives without evaluating cfg. A passing report establishes the reference graph, not that every
annotated test executed or that its assertions are sufficient. Runtime
conformance remains a separate checked session.

## Review checklist

The [review specification](../../design/meta/reviews.md) makes semantic test
review a committed, incrementally checked artifact. This extends the shared
check before the broader book migration; it uses the existing Markdown and
Rust test inventories.

- [x] Extract each anchor's claim and complete test set, preserving test
      source, Cargo identity, configuration conditions, and placeholder status.
      The initial paragraph-only extraction is superseded by the
      [complete-content boundaries](#anchor-content-boundaries) below.
      Test claim boundaries and whole-function extraction.
- [x] Compare saved claim/test inputs and schema/policy versions; test changed
      claims, test additions/removals, changed bodies and metadata, and movement
      that changes only source offsets.
- [x] Provide `review start` to capture initial inputs and invalidate old reports,
      `review read` to extend captured context, and `review record` with
      fresh-input validation and rejection of invalid reports. Preserve unrelated current reviews and flag retired ones for explicit cleanup.
- [x] Include current review decisions in the shared check, CI, and workspace
      Cargo test. Exercise missing, malformed, stale, unsuccessful, and
      incomplete reports and requested edits.
- [x] Add the repository review skill, delegate the initial claim/test review,
      and record its reasoned decisions alongside the supporting changes. Keep
      TODO/N/A evidence distinct from implemented behavior.

**Current evidence:** `crates/sage-meta/tests/review.rs` exercises extraction,
freshness, placeholder evidence, start invalidation, rejected reports,
and preservation of unrelated reviews through the library and CLI. The committed `validations/<anchor-id>/` records preserve reviewed claims and explicit
TODO/N/A entries; generated anchor pages expose their current decisions.

**Evidence boundary:** The cached judgment concerns what the snapshotted test
bodies visibly check. Captured fragments and association metadata are inputs; the validation does not
record that a test ran. Source-level association, agent review, test execution,
and runtime conformance remain separate evidence.

## Canonical anchor system

The destination is specified in [The Design Book](../../design/meta/book.md)
and the anchor mechanics in
[Architectural Pseudocode](../../design/meta/anchors.md#specification-tests-and-workspace-checks).
These checkpoints extend the existing check and browser; they do not replace
the runtime-conformance stages below. The RFD explains the change and stages
work while the Design chapter edits on the same branch state the normative
rules. Completion does not require promoting specification text out of the RFD.

- [x] **Review the specification and authoring workflow.** Review the book and
      anchor-system rules together, retain coverage for existing guarantees,
      and account for pending implementation and tests through explicit
      `todo` or `not_applicable` entries. Apply the same rules to the anchor
      system itself, with no privileged exemptions. Keep current limitations
      separate from the destination.
- [x] **Render Crates chapters from the relocated model.** Move the 32 model
      sources into `md/design/crates/<crate>/`, preserving their model
      identities. Generate crate overviews and nested `.sage` module chapters
      using the shared searchable explorer, including embedded Rust source.
      The default CLI/library loader follows the book's configured source
      directory. Standalone fixtures use the same configured book layout;
      explicit model paths remain available. Missing book configuration or a
      missing configured model fails instead of selecting another source tree.
      Build and serve the book without requiring a separate Sage server.
      Preprocessor integration tests cover generated chapters, custom
      source roots, crate ownership, relocation, safe embedding, regeneration,
      and path diagnostics. Browser tests cover member routes and parent URL
      synchronization. A Chromium check of the built book verified search,
      direct links, Rust source show/hide, cross-chapter navigation,
      Back/Forward, and reload without page-script errors.
      The embedded explorer also follows mdBook's resolved theme on load and
      when it changes; the standalone explorer retains its own toggle.
- [x] **Complete the shared specification inventory.** Use
      the Design chapter tree as the normative inventory, with Markdown and
      `.sage` sources under the book source directory. Specify path and local
      name normalization, then derive namespaces from the complete chapter
      source path relative to Design, excluding headings. Resolve `.sage`
      definition/reference syntax and enforce crate-directory/`crate.sage`
      ownership against the Rust workspace. Test duplicate rule names,
      reference resolution, precise context, and matching model information in
      served and generated explorer views. Inventory direct links for every modeled entity under
      the [entity-link contract](../../design/meta/book.md#meta.book.entity_links),
      independently of named specification rules.
- [x] **Enforce rule correspondence coverage and migrate anchor names.**
      With `.sage` chapters now in `md/design/crates/<crate>/`, migrate existing
      anchors to chapter-derived names, update references, and redirect old
      URLs. Index point and balanced region markers in real Rust comments;
      record excerpts, enclosing modules, and ordinary, `todo`, or
      `not_applicable` status independently for implementation and tests.
      Resolve conflicting nested special modules explicitly. Require both
      entries for every rule and reject dangling references or malformed
      regions. Exercise the shared check, CLI, and workspace test, without
      adding a `cargo build` hook.
- [x] **Generate the rule index and anchor pages.** Render a hierarchical index
      of anchor text and status. Display unfinished entries as **TODO** and
      inapplicable entries as **N/A**. Each anchor page shows its definition, context,
      implementation excerpts, tests, and separately labeled placeholders or
      recorded evidence. Point excerpts offer surrounding source via
      **See more…**; regions preserve explicit boundaries. Link specification
      badges to the pages. Define evidence ingestion before claiming that a
      listed test ran or that a trace established a rule.
- [ ] **Complete book and explorer navigation.** Extend the integrated
      explorer to verify the full specified
      [structure](../../design/meta/book.md#meta.book.browser_structure),
      [search](../../design/meta/book.md#meta.book.browser_search),
      [dependency navigation](../../design/meta/book.md#meta.book.browser_dependencies),
      [Rust source pane](../../design/meta/book.md#meta.book.browser_rust_source),
      and [delivery modes](../../design/meta/book.md#meta.book.browser_delivery)
      across the book, standalone export, and local server. Define
      cross-reference syntax for direct
      entity links; cover all top-level declaration kinds, explicit methods,
      and crate/module entries, including equal local names under different
      owners. Test unique routes, incoming links from elsewhere in the book,
      and preservation of model entity identities after chapter relocation.
      Verify [selection URLs](../../design/meta/anchors.md#meta.anchors.explorer_navigation):
      direct links, redirects, and browser navigation restore the selected
      entity or rule in its book context.
      Verify the generated book works without a running `sage-meta serve`.

**Current boundary:** The shared inventory derives names from Design chapter
paths and indexes Markdown and `.sage` definitions and references. Rust comment
entries and annotated tests supply independent implementation/test coverage;
explicit TODO/N/A entries retain their explanations. The book generates root,
namespace, and individual anchor pages, linked in both directions with source
context. Index rows display the final name component beneath their namespace headings;
tooltips and link destinations retain full identities. Saved test reviews
remain separate from execution results.

Independent claim/test review covers every current rule. Review findings led
to stronger receiver, virtual-dispatch, associated-owner, macro-completeness,
placeholder-coverage, and namespace-filtering assertions. Remaining gaps retain
claim-specific TODO explanations; they are not inferred to pass from the
workspace test result. The generated index exposes the saved review alongside
the independent implementation and test entries.

The handwritten Crates landing page links into generated model and companion
Markdown chapters. Existing subsystem claims now live beside their modeled
entities; detailed algorithms remain in crate guides. RFDs preserve decision
rationale and sequencing. Tenet content and worked examples remain for later
editorial discussion. Runtime-conformance and agent-transport work below keeps
its own unfinished checkpoints; this book migration does not establish those
behaviors.

CI's orphan-chapter lint uses the same `sage-meta` chapter inventory as book
generation, including companion crate guides. It checks the configured book
source directory and rejects unpublished Markdown without requiring generated
chapters to be repeated in `SUMMARY.md`.

## Anchor content boundaries

An independently testable claim can need several paragraphs, a list, or an
example. Paragraph-only extraction silently omits those qualifications from
both the generated rule and its saved review. The normative edits are in
[Anchor content](../../design/meta/anchors.md#anchor-content) and
[Review inputs](../../design/meta/reviews.md#review-inputs); nonnormative
[Anchor tenets](../../design/tenets.md#anchor-tenets) guide authors in splitting
independent guarantees. The existing nonempty opening-paragraph syntax stays.

1. **Specify and review the independent claims.** The initial specification
   stage edited the Design sources directly. Content inclusion, next-definition boundaries, headings,
   containing-source boundaries, explicit ends, unmatched ends, and literal
   examples each have an anchor. `source_representation` and `review_scope`
   were split from the commentary after `complete_test_set`. The paragraph
   regression remained a test of then-current behavior during that stage;
   explicit TODO entries identified the unimplemented complete-content rules.
2. **Implement extraction over Markdown structure.** Select a contiguous source
   extent using the first closing boundary. Share the logic with `.sage`
   prose while retaining exact source offsets and the containing modeled entity.
   Exercise repeated definitions, every heading level and both heading styles,
   all included block types, EOF, independent `.sage` strings, optional standalone
   `<!-- end anchor -->`, orphan/repeated terminators, and literal examples.
   Treat the delimiter as syntax, never as part of the rendered or reviewed claim.
3. **Audit and migrate the existing corpus.** Inspect each changed extent before
   switching the default extractor. Keep substantive qualifications within the
   guarantee; split independent requirements and put explicit ends or headings
   before unrelated commentary. In particular, audit the eligibility paragraph
   after `test_references` and general `TyExprData` commentary following
   `body.macro_expansion`. Parser activation and those source edits land together
   so no intermediate version silently changes ownership of specification text.
4. **Connect rendering, snapshots, and evidence.** Use the shared extent in anchor
   pages, table disclosures, context/source spans, and review preparation. Verify
   later-paragraph/list/example edits stale the corresponding saved review, while
   edits beyond a real or explicit boundary do not. Verify implementation-only
   edits preserve the saved claim/test input and assert line-array JSON directly.
   Replace each TODO with focused evidence only when its claim is established.
5. **Review changed inputs and validate the book.** Prepare exact current inputs,
   delegate every pending claim/test set, and record fresh judgments. Do not copy
   old approvals over enlarged inputs. Retain unaffected reviews only when their
   inputs and schema/policy versions still match. Run the shared check, workspace
   tests, and book build; inspect Markdown and `.sage` context round trips and
   full disclosures, including literal delimiter examples.

**Current boundary:** Extraction now selects the complete Markdown extent in
both source formats, including multi-block content and explicit end markers.
Generated anchor views and review snapshots consume that same extent. Focused
parser, rendering, and review regressions replace the content-boundary TODO
entries; review staleness covers changes within later blocks and excludes
changes after a closing boundary.

The corpus audit inspected all 212 definitions. Twenty claims retain additional
paragraphs or examples, including test eligibility, contract verification
scenarios, and source-review qualifications. Ten explicit ends separate topic
changes, including crate dependency notes and the general `TyExprData`
description after `body.macro_expansion`. The remaining definitions already end
at another definition, a heading, or their containing source boundary.
Independent review of the exact changed inputs and final shared-check,
workspace-test, and book-build validation complete the fifth stage.

## Stage 0: Prove source-index matching against Salsa

- [x] Establish the isolated test crate and model under
      `crates/sage-meta/tests/fixtures/contracts/`, driven by a `sage-meta`
      integration harness. Pin the same vendored Salsa in the fixture workspace.
- [x] Register `sage` as a tool namespace on the pinned nightly and compile
      inert `#[sage::item(...)]` annotations on sample types, functions, methods,
      and marker modules. No identity proc-macro crate is needed.
- [x] Parse annotated declarations and join them to real Salsa ingredient
      identities using source provenance. Expose location data to callbacks
      without parsing debug strings.
- [ ] Cover free queries, tracked methods, macro-generated queries, shared
      expansion sites, and cfg alternatives. Establish path normalization and
      a way to reject source/build mismatches.
- [x] Observe cold/reused queries, internal reexecution, and accumulator
      traversal. Identify required balanced execution events and request-cause
      distinctions. Preserve existing signatures and generated query APIs.
- [x] Record supported source matching forms. Missing or ambiguous matches
      prevent conformance success; if locations are insufficient, prototype
      richer provenance before considering compiled operation metadata.
- [x] Record independently reviewed mapped-traversal expectations, including
      expected Rust declaration sites. Include differing Rust/Sage names and
      swapped/missing/ambiguous annotation cases.

**Current evidence:** The fixture checks free queries, two tracked methods in
one impl, cold/warm/internal-validation traversals, accumulator requests, panic
cleanup, and deliberately missing/swapped/ambiguous mappings. Salsa's tracked
impl macro now preserves each original method's source span. General generated
queries remain rejected when provenance cannot establish a unique declaration.
Source snapshot equality is tested; complete build-configuration identity is
still pending, so the broad provenance checkpoint remains unchecked.

**Exit:** A proven source-to-query association for the first compiler slice,
with explicit gaps and required callback changes. This replaces the previous
nested query-wrapper prototype.

## Stage 1: Inert attributes and correspondence index

- [x] Add tool registration to relevant Cargo crate roots. Validate the
      `#[sage::item]` payload in the source loader; Rust accepts tool metadata
      syntax but does not resolve these Sage paths.
- [x] Define stable Sage unit IDs, annotation path grammar, Cargo owner/target
      identity, and compatibility rules. Confirm correspondence granularity
      from the RFD before populating the index.
- [x] Index annotated functions, associated methods, types, traits, and
      modules with source spans. Handle inline/out-of-line modules, cfg sites,
      and multiple Cargo targets. Use the canonical tool-attribute spelling.
- [x] Audit `define_kind_symbols!` and other generators. Forward annotations
      through generated declarations and retain their actual source provenance;
      do not infer anchors from arbitrary token patterns.
- [x] Add a proposed `sage-meta check --correspondence` entry point and a
      machine-readable report. Report model-only consistency independently.
      During migration this opt-in report may expose incomplete coverage.

**Current evidence:** The source index inventories library and binary targets
by Cargo package ownership, preserves cfg conditions, recognizes supported macro
input grammar, and rejects unsupported annotation placements. It does not claim
that an inventoried cfg site was compiled. The CLI emits distinct model and
correspondence results and source/executable coverage in JSON.

**Exit:** Positive and negative fixtures establish [`meta.contracts.correspondence`](../../design/meta/contracts.md#meta.contracts.correspondence).
Inert annotations do not imply runtime checking. No existing `#[rust]`
mapping is removed yet.

## Stage 2: Migrate the corpus and source browser

- [x] Annotate actual Rust declarations and explicitly declared methods, crate
      and module anchors, and logical `mod r#abstract {}` anchors. Map abstract
      interfaces and external/primitive representations without inventing
      corresponding concrete Rust types.
- [ ] Expand the tooling's own model to describe correspondence, workspace
      checks, and runtime contracts. These facilities live in `sage-meta`;
      no separate runtime-crate manifest is needed. Avoid compiler dependency
      cycles while refining that model.
- [x] Treat multiple implementation sites as intentional. Review broad
      grouped operations against real code; refine the model where necessary.
      Keep recursion explicit in envelopes.
      Allow multiple non-executable correspondences per site, but resolve
      overlapping operation identities before instrumenting a function.
- [x] Derive browser links from the correspondence index, including method
      sites, multiple anchors, and clearly labeled marker modules.
- [x] Establish complete source correspondence coverage across the repository,
      and distinguish this from the current build's matched query definitions.
- [x] Remove every `.sage` `#[rust]` annotation, its parser/AST support, corpus
      expectations, and the obsolete Rust-name lookup route. Update docs and
      make strict correspondence part of routine repository validation.

**Current evidence:** All current corpus correspondence units have Rust anchors,
including marker-only operations. Source-only coverage is explicitly reported;
those anchors cannot substitute for executable boundaries. Runtime support is
currently part of `sage-meta`, with an optional Salsa adapter, so no new runtime
crate or model manifest was introduced in this increment.

**Exit:** No dual-maintained mapping remains. Deleting or misspelling an anchor
fails validation; browser source navigation still works for all model units.
Annotation-only compiler builds remain behaviorally unchanged.

## Stage 3: Runtime contract sessions and checking engine

- [x] Expose resolved model/dispatch APIs and the runtime contract engine in
      the `sage-meta` library, with no compiler dependency.
- [ ] Keep serving dependencies optional for runtime library consumers.
- [ ] Implement immutable session loading, model discovery, caching, digest,
      and actionable initialization failures ([`meta.contracts.validated_session`](../../design/meta/contracts.md#meta.contracts.validated_session)).
      Load the Rust index alongside contracts, require source/build compatibility,
      and keep session bootstrap outside recursive enforcement.
- [x] Implement checking over explicit entry/exit events: direct permissions,
      inherited prohibitions, recursion, and receiver identity for `self` edges.
      Design separately how ordinary functions will emit events; inert
      annotations alone cannot observe them.
- [ ] Resolve abstract-interface dispatch semantics against the actual
      `FnSymbol::sig` to `LocalFnSym::sig` chain. Preserve both invocations,
      authorize the override transition explicitly, and retain ancestor rules.
- [ ] Define explicit root operations, invocation IDs, balanced events,
      source sites, and violation records. Fail the checked test and preserve
      its diagnostic even when user code catches a panic.
- [ ] Add isolated tests for missing files, model edits between sessions,
      same-operation recursion, cross-receiver rejection, dispatch,
      parallel sessions, and unwind cleanup.

**Current evidence:** `ContractModel` owns immutable resolved contracts behind
`Arc`; `ContractSession` supplies explicit roots, operation IDs, receiver checks,
inherited prohibitions, recursion, dispatch, and persistent violations/gaps.
The explicit-event tests cover these rules and malformed boundaries. Source
loading is reusable, but process-wide cache/discovery, digesting, automatic
initialization, and optional HTTP dependencies remain unimplemented. Runtime
support stays in `sage-meta`; a separate runtime crate is no longer planned.
No automatic ordinary-function instrumentation is claimed.

**Exit:** Focused event fixtures demonstrate both passing and forbidden paths.
Coverage identifies uninstrumented categories; full compiler enforcement is
not enabled yet.

## Stage 4: Salsa and async execution integration

- [x] Integrate the proven source index with raw Salsa callbacks and cache
      ingredient-to-operation matches. Preserve public behavior and generated
      query APIs without custom query wrappers.
- [x] Establish balanced request/execution events before trace coalescing.
      Attribute internal reexecution correctly; distinguish source requests,
      verification, and accumulator traversal. Never reconstruct identity from
      debug strings. Failure in a callback must not leave unmatched events.
- [ ] Select and implement separate ordinary-function instrumentation (explicit
      guards or an opt-in instrumentation macro). Feed the same checking engine.
      Uninstrumented helpers retain the active caller; annotation-only sites
      remain visible as runtime coverage gaps.
- [ ] Exercise cold, warm, validated, invalidated, recursive, accumulated,
      cancelled, and panicking requests. Include an unannotated query and calls
      through generated APIs; no request may disappear through those paths.
- [x] Run the fixture's valid model against deliberately nonconforming Rust:
      root → helper → forbidden leaf. Assert both undeclared-edge and ancestor
      prohibition diagnostics with the complete path. Repeat with the forbidden
      leaf prewarmed outside the root operation, using unchanged annotations.
- [ ] Carry active architectural context across async poll boundaries and
      explicitly propagated tasks/threads. Test interleaving, suspension,
      migration, and cancellation. Reject unsupported forms until implemented.

**Current evidence:** The optional `salsa` feature exposes a reusable observer
feeding the same checker from balanced request/execution events. Paired source
snapshots gate mapping; cold and prewarmed forbidden requests report both the
undeclared call and ancestor prohibition. The observer is synchronous and
single-threaded; unsupported threading, receiver projection, ordinary-function
boundaries, and unobserved effects remain visible gaps. It is not general async
compiler enforcement.

**Exit:** [`meta.contracts.cache_invocations`](../../design/meta/contracts.md#meta.contracts.cache_invocations) passes against real Salsa behavior without
double-counted invocations or TLS state leaking between operations.

## Stage 5: Enforce one real compiler slice, then expand

- [ ] Start with a checked-in caller/callee Rust project through the production
      body-checking path. Inventory its mapped operations and all observed
      query boundaries; explicitly account for compiler input/accessor events.
- [ ] Show cold execution, warm reuse, a callee-body edit, and a callee-signature
      edit. A callee body must not be requested while checking the caller.
- [ ] Add a controlled negative fixture or injected forbidden edge and prove
      the checked test fails with the complete call path and violated rule.
      Also prove that an unmapped request prevents a conformance success.
- [ ] Review uncovered operations and model discrepancies. Implement or fix
      missing mappings; propose envelope changes only when justified by design.
- [ ] Wire enforcement into normal test commands/CI for this fully covered
      scope. Expand scope incrementally to signature checking, resolution,
      inference, and solver entry points. Keep unconverted scopes explicit.
- [ ] Report call/prohibition/receiver coverage separately from `reads` and
      `creates`. Plan accessor and context-construction hooks before claiming
      enforcement of those effect categories.

**Current evidence:** `sage-test-harness/tests/architecture_contracts.rs` runs
a checked-in caller/leaf Rust fixture through production queries. An empty
function signature passes cold/warm checks with an independently asserted Rust
site. The caller-body trace explicitly remains incomplete because receiver and
ordinary-function boundaries are not fully observed, while an independent query
assertion excludes callee-body requests. This does not complete the first body
acceptance target or justify general compiler enforcement.

**Exit:** The first real source fixture demonstrates [`meta.contracts.observation_coverage`](../../design/meta/contracts.md#meta.contracts.observation_coverage).
Tests enforce invariants automatically without a browser or an agent. Broader
runtime coverage is a visible follow-on, not a prerequisite for this milestone.

## Stage 6: Observed browser walkthrough

- [ ] Connect the existing Semantic Inspector provider and trace facilities
      to the architecture browser through a typed API. Avoid a second compiler
      backend and avoid reverse dependencies from `sage-meta` into `sage-ir`.
- [ ] Replay the accepted fixture with selected operation, Rust definition,
      input source, permission, cache disposition, and violation path visible.
- [ ] Preserve the model digest and source revision. Mark stale/mismatched
      artifacts and separate observed calls from merely permitted calls.
- [ ] Drive the real source scenario in a browser engine; verify stepping,
      source links, reused queries, and a displayed violation.

**Exit:** One trace serves as both a learning walkthrough and validation evidence.

## Stage 7: Shared local tools and MCP

- [ ] Stabilize typed operations for lookup, correspondence, validation,
      fixture runs, trace inspection, and model-diff impact review. Provide
      scriptable CLI forms and bounded trace retrieval.
- [ ] Expose those operations through MCP in serve mode (or an adapter to its
      host service). Select transport and SDK after confirming agent support.
- [ ] Define structured outputs with stable item/event IDs and model revisions.
      Verify tool results against direct CLI/service results on the same fixture.

**Exit:** An external agent can explain a real run using the same checked data
as the browser; no duplicated validator or agent-generated execution evidence.

## Stage 8: ACP sessions and agent workflows

- [ ] Add server-hosted ACP sessions to the browser, with selected model item,
      source, fixture, and trace as context; configure the MCP tools for the
      connected agent. Support cancellation, progress, and normal tool approval
      behavior without requiring an agent for standalone browsing or tests.
- [ ] Add workflows/skills for explaining a fixture, reviewing an envelope diff,
      and investigating a violation. Explanations cite event IDs; predictions
      remain labeled; proposed model edits remain reviewable changes.
- [ ] Demonstrate the iteration loop: propose a dependency change, inspect
      affected callers, discuss it, update implementation/model as agreed, and
      rerun the conformance fixture with a new model revision.

**Exit:** Iteration, explanation, and validation share one model and evidence
API. Static implementation call analysis remains a separate later milestone.

## Completion checklist

- [ ] Verify that the Design chapters and their evidence reflect the completed
      scope, retaining observation-coverage and unchecked-effect limitations.
      Check the [contract rules](../../design/meta/contracts.md) against their
      recorded evidence and remaining gaps.
- [x] Extend D21 on acceptance; preserve the distinction between model
      consistency, source correspondence, and runtime conformance.
- [x] Update architecture Current Status, roadmap, and RFD indexes as stages
      land. Link claim-specific tests and walkthrough evidence.
- [ ] Build the book and run relevant Rust, macro, compiler, browser, and tool
      integration checks for each implementation stage.


## Implementation completeness reviews

The [review contract](../../design/meta/reviews.md) now asks reviewers to judge
implementation completeness and test adequacy together. Policy version 2
invalidates earlier test-only approvals; it does not reinterpret them as code
reviews. Explicit TODO entries still acknowledge incomplete work.

Implementation point markers provide short navigation previews, so caching only
those excerpts would miss edits to most of the implementation. The review input
therefore includes complete associated files and association metadata. This
conservative boundary increases checklist size and review churn for unrelated
edits in the same file, while keeping the source inspected by reviewers visible
in saved inputs. Essential implementation logic in other files needs its own
association; arbitrary transitive dependencies are not automatically captured.

The review skill directs reviewers to add and run regression tests for suspected
bugs, record reproduced failures, and consult the user when reproduction or test
construction is unsuccessful. Tests added during review require fresh input
preparation before the decision is recorded.

## Per-anchor validation records

The [review contract](../../design/meta/reviews.md) stores each anchor's review in
`validations/<anchor-id>/{summary,input,output}.json`. The main agent delegates
one reviewer per failing anchor. `review start` resets that anchor's directory,
captures initial context, and leaves the gate failing until `review record`
saves a complete report with no requested edits. `review read` captures additional
context before returning it. Recording preserves the input already reviewed.

Uniform fragments identify a whole file, line range, claim, or test function and
store its exact source text. Identifier-based claim/test selectors avoid review
churn from unrelated line movement. Freshness compares contents directly; hashes
are unnecessary. TODO/N/A status belongs to the evidence, while the report's edit
list states whether further work is needed. Records for removed IDs require
explicit cleanup. Reviewers declare dependencies; automatic read capture can use
the same fragment representation.
