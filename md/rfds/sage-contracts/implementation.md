# Implementation plan and status

**Accepted; first infrastructure implemented.** Correspondence migration is
complete for the current corpus. Default `check --json` reports model,
Rust-correspondence, and specification-test checks separately, with source
coverage independent from executable declarations. A public library check is
shared by CLI, CI, and a workspace Cargo test. The isolated Salsa fixture
and explicit-event tests establish the initial runtime checker; broad compiler
acceptance, automatic session initialization, async propagation, browser replay,
and agent transports are still pending. Checked boxes describe completed work,
not blanket completion of their containing stage.

Evidence lives in `crates/sage-meta/tests/correspondence.rs`, `cli.rs`,
`runtime_contracts.rs`, and `contracts.rs` plus its isolated fixture and reviewed
`expected/` files. Reproduce the core checks with `cargo test -p sage-meta`.

## Shared build-time check and specification anchors

- [x] Expose `sage_meta::check(workspace_root)` and a report whose
      `ensure_valid()` rejects model, correspondence, or specification errors.
      Keep full checking of a selected model directory through `check_model`;
      retain model-only loading/validation and the legacy positional CLI.
- [x] Make `cargo run -p sage-meta -- check` run all three checks by default,
      invoke it in the CI runner, and call the library from a Cargo integration
      test with a manifest-relative workspace path.
- [x] Parse named specification paragraphs and inert `#[sage::test(...)]`
      references. Reject malformed/duplicate anchors, uncovered rules, and
      dangling or misplaced references; exclude Markdown code examples.
- [x] Render named paragraphs in mdBook using the same parser. Migrate the
      architectural-language chapter's class, model-path, and runtime-call
      rules with focused language/runtime tests, preserving the `meta-a1` link.

**Evidence boundary:** Eligible evidence consists of non-ignored `#[test]`
functions in Cargo targets with tests and the standard harness enabled. These
checks inventory declarations across cfg alternatives without evaluating cfg. A passing report establishes the reference graph, not that every
annotated test executed or that its assertions are sufficient. Runtime
conformance remains a separate checked session. Other numbered book anchors
have not been automatically migrated.

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

**Exit:** Positive and negative fixtures establish [SC-A1](./README.md#sc-a1).
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
      and actionable initialization failures ([SC-A2](./README.md#sc-a2)).
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

**Exit:** [SC-A3](./README.md#sc-a3) passes against real Salsa behavior without
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

**Exit:** The first real source fixture demonstrates [SC-A4](./README.md#sc-a4).
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

- [ ] Promote SC-A1–SC-A4 and their verification into the architecture chapter,
      retaining limitations for observation coverage and unchecked effects.
- [x] Extend D21 on acceptance; preserve the distinction between model
      consistency, source correspondence, and runtime conformance.
- [x] Update architecture Current Status, roadmap, and RFD indexes as stages
      land. Link claim-specific tests and walkthrough evidence.
- [ ] Build the book and run relevant Rust, macro, compiler, browser, and tool
      integration checks for each implementation stage.
