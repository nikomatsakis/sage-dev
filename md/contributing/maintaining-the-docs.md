# Maintaining This Book

This page is the **update contract** for the AI agent (and human) that edits this book.
Much of sage is developed conversationally with an agent, drawing on the source code and
design documents. This book is a **source of truth** for sage's design and status, so it
only stays useful if it moves in lockstep with the work. When the design or the system
changes, update the docs in the *same* commit — treat it as part of the change, not a
follow-up.

The rest of this page is written in the imperative, addressed to that agent.

## How the pieces relate — the Design specification and RFDs

The book's **Design** tree is the canonical specification. Its Markdown and
`.sage` chapters state the normative rules, including the rules for this
documentation system itself. Introductory material explains how to use Sage
and is non-normative. Generated anchor indexes and pages provide views of the
specification and its implementation and test connections. The
[book specification](../design/meta/book.md) defines these responsibilities;
this page explains how authors maintain them.

Sage keeps four complementary views of the system:

- **Architecture pages describe the destination and the local current
  status.** Their main text states how the system is meant to work. A final
  **Current Status** section states what works today, concrete limitations,
  inspectable evidence, and related roadmap slices.
- **The build-out roadmap describes cross-cutting implementation slices.** A
  slice has an acceptance target, scope, ordering, dependencies, affected
  architecture, and a high-level implementation plan.
- **RFDs describe a change and its detailed checkpoints.** They record goals,
  alternatives, rationale, and how that scoped change lands. The same branch
  edits the Design specification to state the proposed normative behavior.
- **The known-deviations register tracks confirmed contradictions.** It names
  places where the current implementation violates an accepted design anchor,
  while the affected architecture chapter states the same fact locally.

| | Architecture & Design | Build-Out Roadmap | RFD | Known Deviations |
|---|---|---|---|---|
| Describes | destination, local status, limitations, evidence | cross-cutting slices and their ordering | one change and its detailed checkpoints | confirmed implementation contradictions |
| Answers | *how should this work, what works now, and how can I inspect it?* | *what coherent outcome are we building next?* | *why this change, and how does it land?* | *where does the code knowingly violate the accepted design?* |
| Lifespan | living, kept current | living, kept current | historical once completed | living until closure evidence establishes the anchor |

An **RFD** proposes and discusses a change. Its branch includes the substantive
Design diff: the specification paragraphs and `.sage` declarations being added
or revised. Give the RFD a high-level overview of the needed changes, linking
each directly to the affected Design pages and anchors, then connect its
implementation steps to those rules. The Design diff is reviewed with the rationale, so the current design never has to
be reconstructed by replaying every RFD.

Practical consequences:

- **Keep architectural reasoning in the RFD and normative rules in Design.**
  The RFD can explain architecture in detail, including rejected alternatives,
  while referencing the proposed Design anchors instead of defining a second
  authoritative copy of their requirements.
- **Keep the destination and local status current.** When an RFD changes an
  architectural contract or lands observable behavior, update the relevant
  architecture chapter. Keep destination design in the main text and current
  implementation facts in its **Current Status** section.
- **Keep execution planning cross-cutting.** Add or update a roadmap slice when
  a coherent acceptance target is introduced, reordered, blocked, or
  completed. Do not reshape the roadmap into one status row per architecture
  chapter.

## When to update — trigger → page

When one of these happens, update the matching page(s) before you consider the work done:

| Trigger | Update these pages |
|---|---|
| We settle (or revise) how part of the system *should* be designed | The main text of the relevant [architecture](../design/README.md) page |
| We're planning a change (including an architectural one) | Open an **RFD** (`md/rfds/<name>/`) per the [RFD process](../rfds/README.md) and edit the relevant [Design](../design/README.md) rules on the same branch; link those anchors from the rationale and `implementation.md` |
| A draft RFD is added | List it under *Draft* in [`SUMMARY.md`](../SUMMARY.md); mark unsettled mechanisms as planned in any destination page |
| An RFD's implementation step lands | Tick the step in that RFD's `implementation.md`. Update architecture status or roadmap progress only if the step changes those facts |
| An RFD is accepted (merged, in progress) | Move it to *Accepted* in [`SUMMARY.md`](../SUMMARY.md) and [`accepted.md`](../rfds/accepted.md); link it from any roadmap slice it implements |
| An RFD completes | Move it to *Completed* in [`SUMMARY.md`](../SUMMARY.md) and [`completed.md`](../rfds/completed.md); update affected architecture Current Status sections and roadmap-slice progress |
| Observable behavior changes / something ships | The relevant architecture **Current Status** section: current frontier, limitations, and evidence |
| Code inspection finds a confirmed violation of an accepted design anchor | The relevant architecture **Current Status** section and [`known-deviations.md`](../implementation/known-deviations.md), with the same anchor and required closure evidence |
| A cross-cutting acceptance target is added, reordered, blocked, or completed | The [Build-Out Roadmap](../implementation/roadmap.md): goal, scope, affected architecture, dependencies, high-level plan, and progress |
| A new subsystem, flow, or mechanism is built | Add/update the matching [architecture](../design/README.md) page |
| A cross-cutting, load-bearing decision is made or changed | Add/update an entry in [Architecture decisions](../design/decisions.md) with a new `D<n>` code; a feature-local decision stays in its RFD and is linked from there |
| A new term worth defining | [`terminology.md`](../terminology.md) |
| Any new page | Register it in [`SUMMARY.md`](../SUMMARY.md) — a page not listed there does not render |

When a change touches more than one row, update all of them in the same change.

## Conventions

- **Architecture pages separate destination from status.** The main text may
  describe unimplemented destination design. Put statements about what exists
  today, current limitations, and related slices in a final **Current Status**
  section.
- **Write one independently falsifiable claim per named paragraph.** Give
  each claim a meaningful, stable name and identify what observation would
  establish or contradict it. Split constraints that can fail independently,
  even when they are adjacent, concern the same subject, or share a test
  function. An umbrella anchor does not replace the individual claims and
  their meaningful test assertions.
  Prefix the claim with a custom MyST-style marker such as
  <code>{anchor}&#96;meta.calls.declared&#96;</code>, followed by its prose in the same
  paragraph. Put no blank line between the marker and prose. Keep examples,
  rationale, and explanatory prose outside the named paragraph and unanchored;
  ordinary headings do not need specification markers.
- **Connect each specification anchor to a test.** Annotate Rust tests with
  `#[sage::test(meta.calls.declared)]`, using the anchor's exact name. Every
  named specification rule requires at least one test; every annotation must
  name an existing rule and appear on a module-level function with the built-in
  `#[test]` attribute in a Cargo target with tests and the standard harness
  enabled. `test = false`, `harness = false`, and ignored or conditionally
  ignored tests cannot supply evidence. Several tests
  can cover a rule, and a test can cover several rules. `sage-meta check` rejects malformed or
  duplicate markers and broken or absent test references. It inventories source
  across cfg alternatives, so a mapping does not establish that the test ran
  or that its assertions are adequate. Review the assertions and run the tests.
  Fenced and inline examples are not declarations.
- **Refresh review decisions when claims or tests change.** The committed
  `review-checklist.json` stores the claim and complete annotated test set
  reviewed together. The [review specification](../design/meta/reviews.md)
  defines the inputs and freshness check. Use the repository's
  `sage-review-checklist` skill at
  `.agents/skills/sage-review-checklist/SKILL.md` to review pending entries and
  record decisions; do not refresh snapshots without reviewing them. CI and
  workspace tests reject absent, stale, or unsuccessful decisions. A reviewed
  TODO or N/A entry remains a visible gap, not proof of the behavior.
- **Use the canonical anchor plan for new specification work.** Define rules
  in the book's Design tree; Markdown and `.sage` chapters have equal
  authority there. Each rule has one definition, an anchor page, and independent
  implementation and test entries. The [book specification](../design/meta/book.md)
  defines chapter-derived namespaces, the generated index, and the RFD workflow.
  The
  [specification](../design/validation/architecture-language.md#specification-tests-and-workspace-checks)
  defines Rust point comments, optional `<<` / `>>` regions, and `todo` /
  `not_applicable` module conventions. These are destination rules: consult
  [anchor status](../design/validation/architecture-language.md#specification-anchor-status)
  before claiming the checker or browser supports them. During specification
  review, explicit no-op tests in `mod todo` may account for unimplemented
  rules; identify them as placeholders in Current Status and replace them as
  behavior lands. Their success does not verify the specification.
- **Distinguish the destination authoring model from the current loader.**
  The destination derives an anchor's namespace from the complete chapter
  source path relative to `design/`, removing the extension and joining path
  components with dots. Headings and chapter titles do not participate. It
  embeds `.sage` chapters under their owning crate in that tree. Write the full
  anchor name in Markdown until namespace derivation lands, and account for the
  checker's current scan of Markdown throughout `md/`. A chapter-relative marker
  is not yet supported. Keep `.sage` files in `md/design/crates/<crate>/`;
  book generation and the default model loader use that shared source tree.
- **Author Crates content for the shared explorer.** The Crates
  section uses `design/crates/<crate>/*.sage` sources, with `crate.sage`
  establishing each modeled crate. Follow the book's self-contained
  [display specification](../design/meta/book.md#displaying-sage-files):
  organized declarations, search, dependency navigation, a revealable Rust
  source pane, direct entity links, and defined delivery modes.
  The book preprocessor generates the crate and module subchapters; do not
  maintain Markdown copies or add each `.sage` file to `SUMMARY.md`.
  `mdbook build` and `mdbook serve` generate the ignored `md/sage-explorer.html`
  snapshot, which mdBook copies into its output. It is shared by every Crates
  chapter and is only rewritten when its contents change. Rebuild the book to
  refresh embedded Rust source after Rust edits.
  Chapter paths cannot contain `#`, `?`, or `%`: mdBook uses them directly in
  sidebar links, so preprocessing rejects those names with a diagnostic.
- **Link model entities independently of specification rules.** Every modeled
  entity needs a direct link that other book or model content can reference,
  including types, classes, contexts, explicit methods, and crate/module
  entries. The [entity-link contract](../design/meta/book.md#meta.book.entity_links)
  uses model identities independently of chapter-derived rule names. Merely
  making an entity linkable does not declare a specification paragraph. Exact
  cross-reference syntax is still open; do not assume a new directive exists.
- **Use TODO for unfinished coverage.** Render entries in `todo` modules as
  **TODO** in the index and anchor pages; use **N/A** for `not_applicable`.
  Keep implementation and test statuses separate, and do not present a
  passing placeholder test as evidence of the rule's behavior.
- **Preserve links when rule names change.** Update specification, Rust, and
  test references together. Keep a redirect or context alias for a published
  URL; compatibility links do not create another rule or satisfy a stale
  source reference. The named-rule syntax uses `sage-meta`'s mdBook
  preprocessor and does not require full MyST support.
- **Keep an anchor's requirement separate from its present evidence.** The
  anchor says what must be verified even when the destination is not yet
  implemented. The chapter's **Current Status** section says whether it is
  established and links the tests, snapshots, traces, edit experiments,
  inspector scenarios, or source anchors which currently support it. Do not
  turn planned verification into a claim that evidence already exists.
- **Reserve known deviations for confirmed contradictions.** Add an entry to
  [`known-deviations.md`](../implementation/known-deviations.md) only when the
  implementation is known to violate an accepted anchor. Give it a stable
  `KD-<n>` identifier, link the affected anchor and the chapter's Current
  Status, describe the observed behavior and consequence, and state the
  evidence required to close it. Do not use this register for ordinary planned
  work, missing test coverage, or intentional language restrictions.
- **Distinguish chapter anchors from architecture decisions.** A chapter
  anchor is a local, auditable consequence of the design. A `D<n>` entry in
  [Architecture decisions](../design/decisions.md) records a cross-cutting
  choice and its rationale. Anchors should link an applicable decision rather
  than duplicating its rationale; feature-local choices remain in their RFD.
- **Keep rules in Design from the proposal onward.** Review their normative
  text in the RFD branch's Design diff. Implementation updates evidence and
  status there; the RFD keeps rationale and implementation checkpoints.
- **Ground built claims in the code.** For anything described as existing, tie statements to
  actual modules/files and keep references accurate. Planned design is grounded in the
  design discussion instead, and is labelled as planned.
- **Make evidence claim-specific.** Link each important built claim to a
  focused test, readable snapshot, query trace, edit experiment, exact oracle
  result, inspector command, or code anchor. “The test suite passes” is not
  sufficient evidence for a specific architectural guarantee.
- **Include explanatory excerpts with ezanchor.** Put matching
  `// ANCHOR: name` and `// ANCHOR_END: name` comments around the smallest
  useful source region in one of the `scan-dirs` configured in `book.toml`,
  then reference it with an ezanchor block:

  ````text
  ```{anchor}
  name
  ```
  ````

  This emits both the excerpt and its GitHub source link. Do not
  hand-copy Rust implementation snippets into walkthroughs. These region
  labels remain independent of canonical specification references; an ezanchor
  label alone does not declare that the region implements a specification rule.
- **Diagrams use Mermaid** in fenced ` ```mermaid ` blocks.
- **Style** (inherited from the RFD process): no promotional or dramatic language; be factual
  and brief; lead with concrete concepts, then generalize; include examples.

## Evidence maintenance checklist

When a change affects an architecture claim or worked example:

- update each affected specification paragraph and its annotated tests;
- run the `sage-review-checklist` skill when the claim or complete test input
  differs from its saved review, then include the resulting checklist changes;
- run `cargo run -p sage-meta -- check` to validate model, Rust, and
  documentation/test correspondence and current review decisions;
- add a new `D<n>` only for a genuinely cross-cutting decision, and link its
  chapter-level consequences;
- update the chapter's Current Status claim, limitation, and roadmap link;
- keep the Rust input and readable expected output in its review packet
  accurate;
- update focused test names and reproduction commands;
- update snapshots only after reviewing the semantic change;
- verify query-trace assertions still name the intended semantic boundary;
- update edit experiments when identity, keys, or invalidation changes;
- keep every ezanchor small, unique, and attached to the load-bearing code;
- distinguish human-readable inspector evidence from exact Oracle
  conformance; and
- build the book so missing anchors, orphaned pages, and available link checks
  run before commit.

## Verify before you commit

Build the book locally — this is the same check CI runs on the PR, so it catches problems
before you push:

```bash
cargo run -p sage-meta -- check
mdbook build   # or `mdbook serve` to preview and rebuild on save
```

The shared check also runs through the library in the workspace Cargo tests.
The book build renders specification markers using the same parser and surfaces
the two other things easiest to get wrong:

- **Orphaned pages** — a `.md` under `md/` that isn't listed in [`SUMMARY.md`](../SUMMARY.md)
  warns ("not linked to in SUMMARY.md") and won't render. Add every new page to `SUMMARY.md`
  in the same edit that creates it.
- **Broken links** — `mdbook-linkcheck2` will report dead internal links; CI fails on these.

Then confirm the new/edited page appears in the sidebar and that content and intra-book links
resolve, and commit.
