# Implementation plan and status

This RFD is a draft. No implementation work should begin until its service,
trace, and reload boundaries are accepted.

Each step is intended to be independently reviewable and to leave existing
CLI, unit, and oracle tests working.

## Step 1: Pin the observation and trace contracts

- [ ] Define typed selectors, selection failures, inspection requests, and
  self-contained inspection results.
- [ ] Define stable structured events for Salsa execution, semantic lookups,
  and external metadata requests.
- [ ] Define trace phases and the freeze-before-render rule.
- [ ] Identify which existing raw query-log assertions migrate immediately
  and which remain temporary diagnostics.
- [ ] Add recorder unit tests for set/multiset normalization, required events,
  forbidden events, and unmapped raw events.

## Step 2: Extract a persistent workspace host

- [ ] Refactor the current one-shot driver setup into an owning workspace host
  plus a read-only analysis view.
- [ ] Keep `run_sage_with` as a compatibility wrapper where useful.
- [ ] Update an existing `SourceFile` input without rebuilding the database.
- [ ] Keep the rustc dependency-metadata service alive for the host lifetime.
- [ ] Represent and report the boundary between a source-input revision and a
  full Cargo/dependency reload.
- [ ] Test cold, warm, relevant-edit, and unrelated-edit revisions against one
  host.

## Step 3: Implement semantic selection

- [ ] Resolve stable absolute paths for modules, free items, and associated
  items.
- [ ] Return structured ambiguity and not-found results.
- [ ] Adapt source-position selection from the Resolve at Position work to the
  same selected-item type.
- [ ] Test that semantic-path and position selectors identify the same item
  without exposing internal IDs.

## Step 4: Implement the inspection service

- [ ] Add `item`, `signature`, and completed `body` inspection.
- [ ] Add focused `impls`, `prove`, and input-only `normalize` operations over
  their existing semantic APIs.
- [ ] Produce owned observation values before freezing the analysis trace.
- [ ] Preserve diagnostics, ambiguity, overflow, and candidate-source
  incompleteness instead of rendering them as completed results.
- [ ] Test that inspection does not read unrelated bodies or associated
  values.

## Step 5: Add deterministic human rendering

- [ ] Pretty-print signatures and completed typed bodies with stable semantic
  paths and no raw allocation identities.
- [ ] Render resolved dispatch, substitutions, and explicit elaboration at
  useful default and verbose levels.
- [ ] Snapshot representative `DbDropGuard::db` and `Parse::next` output.
- [ ] Prove that rendering a self-contained observation executes no semantic
  queries.
- [ ] Keep oracle comparison and its exact serialized form unchanged.

## Step 6: Add batch and interactive CLI clients

- [ ] Add a batch `cargo sage inspect` form suitable for scripts.
- [ ] Add an interactive session with `trace last`, `rerun`, and explicit
  revision information.
- [ ] Add machine-readable output without making that schema the oracle
  schema.
- [ ] Add focused end-to-end CLI tests backed by the shared service.

## Step 7: Add watch mode and edit-test facilities

- [ ] Watch existing source files, debounce changes, and update inputs in the
  persistent host.
- [ ] Detect changes which require a full workspace reload and report them.
- [ ] Expose reusable test helpers for edit sequences and trace assertions.
- [ ] Migrate the Trait Impl Candidate Discovery edit-invalidation matrix to
  the structured recorder when that RFD is implemented.
- [ ] Document a reviewer workflow using a pinned mini-redis function.

## Step 8: Record the client boundary for future LSP work

- [ ] Verify that the inspection service has no terminal, Clap, JSON-RPC, or
  LSP position dependencies.
- [ ] Document how an LSP host would apply document changes and serve custom
  requests or virtual documents.
- [ ] Open a separate RFD before choosing or implementing an LSP extension.

## Completion

- [ ] All required tests in the RFD pass.
- [ ] Repository-required full validation passes.
- [ ] The accepted destination is reflected in architecture documentation,
  including built versus planned status.
- [ ] This RFD moves from Draft to Completed only after the CLI, persistent
  edit session, and structured testing facility are usable; an LSP server is
  not required for completion.
