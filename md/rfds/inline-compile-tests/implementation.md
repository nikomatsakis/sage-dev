# Implementation Plan: Inline Compilation Tests

**Status: Draft. No runner implementation is authorized by this documentation
change.** The [RFD](README.md) records rationale; the
[Design specification](../../design/crates/sage_test_harness/compile-tests.md)
owns behavior. Checklist items below are proposed work, not current coverage.

## 1. Settle execution and rendering boundaries

- [ ] Identify the production workspace/metadata adapter that can materialize an
      inline Cargo project, provide real std, and accept invalid fixture source
      without requiring that source to pass rustc first.
- [ ] Locate shared setup code so test harness consumers do not form dependency
      cycles. Update `.sage` dependency declarations if actual crate boundaries
      change; keep semantic algorithms in their production crates.
- [ ] Specify exact source-expression/type snapshot records, including equal
      source spans, nested expressions, macro provenance, and explicit error
      types. Freeze the cold-query projection format using existing trace rules.
- [ ] Account for all 21 proposed anchors with explicit implementation and test
      entries before requiring the shared coverage gate to pass. Use TODO entries
      only for genuinely unimplemented work; never mark placeholders approved.

Affected rules: `edition`, `dependencies`, `project_isolation`, `production_execution`, `starting_expressions`,
`rendering`, `query_capture`, and `query_projection` under
`crates.sage_test_harness.compile_tests`.

## 2. Fixture and annotation parsing

- [ ] Add the ordinary-test macro/API, anonymous and named fixture forms,
      deterministic Cargo configuration, the fixed `src/lib.rs` root, and edition override.
- [ ] Add actual-comment parsing, universal target-line selection, line-based type observations,
      bare `ty` markers, severity markers, and `if SAGE` / `if rustc` conditions.
- [ ] Exercise nested directories, modules, missing `src/lib.rs`, duplicate/escaping
      paths, and file/directory conflicts. Verify fixtures are isolated across
      concurrent tests and independent of the caller's working directory.
- [ ] Cover inline and stacked directives, intervening blank lines, ordinary
      comments, absent target lines, tabs, marker-like literals, unknown syntax,
      combined type output across files, missing/unused expectations, and inactive
      compiler conditions. Explicitly test supported and rejected coordinate
      cases, including Unicode text and differing comment indentation, without
      silently treating malformed annotations as comments.

Affected rules: `invocation`, `fixtures`, `edition`, `dependencies`, `project_isolation`, `coordinates`,
`target_line`, `annotation_validation`, and `compiler_conditions`.

## 3. Source-driven checking and observations

- [ ] Check all in-scope local signatures and bodies using production paths,
      including submodules, associated items, and expansion products. Report
      unsupported work explicitly rather than accepting an incomplete walk.
- [ ] Establish real std access, the default edition, and an explicitly older
      edition using Rust fixtures with observably different behavior.
- [ ] Add no-marker and marked versions of a fixture proving marker presence
      does not change which bodies are checked or constrain their inference.
- [ ] Exercise sibling expressions starting on one line, multi-line expressions
      starting on the target line, exclusion of earlier-starting ancestors and
      later-starting children, child-before-parent ordering, equal-span
      source nodes, coercion wrappers exposing original and resulting types, other
      generated elaboration, error types, and absent semantic results.
- [ ] Verify filename/source ordering across multiple type markers, coalescing
      repeated markers on one line, preservation of distinct identical-looking
      expressions, and snapshots without line numbers, columns, or ranges. Cover
      multiline source descriptions, omitted annotation comments, token-preserving
      whitespace normalization, and escaped literal contents.
- [ ] Match structured diagnostics exhaustively. Cover primary-span matching,
      rejection of markers located only at labels (including primary-styled
      labels), multi-line and empty primary spans (including EOF), nested macro
      invocations, diagnostics from every compilation stage, deduplication of
      repeated reports without collapsing distinct identical-looking errors, repeated
      markers on one line, unmatched diagnostics,
      missing diagnostics, severity mismatch, and a case in which greedy matching
      fails despite an available complete assignment. Notes must not inflate
      the diagnostic count.
- [ ] Record cold query evidence with the exact specified capture boundary;
      ensure unknown operations remain visible and rendering adds no semantic
      work after outputs are frozen.

Affected rules: `production_execution`, `checking_scope`,
`starting_expressions`, `completed_types`, `diagnostic_locations`,
`diagnostic_counts`, `rendering`, `query_capture`, and `query_projection`.

## 4. Updating and anchor review

- [ ] Pass explicit `expect!` values through the macro and prove updates target
      the author-written literals. Run update tests on temporary copies so they
      cannot mutate the real test suite while it runs.
- [ ] Establish `blessing_gate` by reviewing the preparation/comparison boundary:
      preparation returns all validated and rendered results or fails, contains
      no expectation assertions, and failure cannot reach comparison. Include
      an implementation association and a comment explaining the ordering.
      A dedicated failure-injection test is not required when this separation
      is directly evident; record a justified test N/A association for review
      rather than claiming executable coverage. If the implementation obscures
      that separation, simplify it or add focused evidence.
- [ ] Prove a valid update changes only expectation contents, preserves source
      markers and compiler conditions, and then passes with updating disabled.
- [ ] Inspect prepared anchor-review input for a macro-using test: it contains
      source, annotations, options, and expectations directly in the function;
      it does not expand or copy the harness macro implementation.
- [ ] Apply the `sage-review-checklist` workflow to each changed claim/test set.
      Replace TODO entries only with substantive evidence and record reviews
      through `sage-meta review record`.

Affected rules: `blessing_gate`, `callsite_updates`, and `review_visibility`,
plus the annotation and diagnostic rules exercised by the negative cases.

## 5. Adopt in a bounded semantic slice

- [ ] Convert representative source-driven inference tests, keeping lower-level
      transaction tests where direct state assertions are necessary.
- [ ] Demonstrate a std-backed anonymous test, a multi-file negative test, a
      historical-edition test, and a type plus query snapshot test.
- [ ] Run focused runner and semantic tests, formatting, workspace build and
      tests, the shared anchor check, and the book build. Inspect source → anchor
      → test navigation and the reviewed snapshots.
- [ ] Move the RFD through acceptance/completion only when the relevant work is
      accepted and every claim has adequate evidence. No blanket approval based
      solely on passing fixtures or successful snapshot updates.

## Deferred extensions

Rustc structured-diagnostic execution; explicit point demands; warm/edit query
runs, additional selectors, and third-party dependency declarations are
separate extensions. Rustc will check
severity, source lines, and counts, not type snapshots or diagnostic wording.
