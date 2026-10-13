# Reviewing Specification Tests

The committed `review-checklist.json` records which specification claims and
associated tests have been reviewed together. `sage-meta` extracts the inputs
and checks whether a recorded decision still applies. An agent reviews whether
the test bodies express an adequate check of the claim. Running the tests
remains a separate obligation.

## Review inputs

{anchor}`meta.reviews.claim_text`
A Markdown claim's review text is the source text of its anchored paragraph
after the marker, with surrounding whitespace removed. Internal whitespace and
Markdown markup are preserved. Following paragraphs, lists, code blocks, and
headings are outside this review input; the anchored paragraph must state the
complete claim being reviewed.

{anchor}`meta.reviews.test_source`
Each associated test's review input contains the complete annotated Rust
function source, including its attributes, signature, body, and contained
comments. It also records the Cargo crate and target, Rust path, file path,
inherited configuration conditions, and ordinary, `todo`, or `not_applicable`
status. Source line numbers and byte offsets are excluded from the input.

{anchor}`meta.reviews.complete_test_set`
One review covers one anchor's claim and its complete set of associated tests.
The input records every eligible test associated with that anchor in a
deterministic order. Several tests may cover different parts of a claim; each
test need not establish the whole claim independently.

Source text is stored as arrays of lines so a Git diff shows what was reviewed.
An implementation excerpt is useful context for review, but it is not part of
this checklist's approval input. The checklist records the adequacy of the
claim-to-test relationship, not a proof of implementation conformance.

## Saved decisions

{anchor}`meta.reviews.staleness`
A saved review applies only when its anchor, complete claim-and-test input,
schema version, and review-policy version match the current inventory. A
changed claim, changed test source or metadata, or added or removed test
association makes the review stale. Movement that changes only line numbers or
byte offsets does not require another review.

{anchor}`meta.reviews.preparation`
Preparing a review checklist emits current input snapshots for anchors without
a current acceptable decision. Preparation does not change the committed
checklist or replace the inputs of a previous decision. Anchors with a current
acceptable decision are omitted from the work to be reviewed.

{anchor}`meta.reviews.recording`
Recording review decisions checks each submitted input against a fresh
inventory before updating the committed checklist. A mismatched input,
duplicate anchor, unknown anchor, missing reviewer or reasoning, or unfinished
decision rejects the submission without changing the saved checklist. A
successful submission replaces the submitted anchors' decisions and preserves
other current anchors' saved reviews, removing entries for retired anchors.

{anchor}`meta.reviews.decisions`
An `approved` decision requires that every associated test is ordinary and
records that those tests visibly check the claim. A `todo` decision requires at
least one `todo` test and accounts for the complete test set; a
`not_applicable` decision requires every associated test to have that status.
These two decisions account for explicit placeholders without claiming
behavioral evidence. A `pending` or `needs_work` decision cannot satisfy the
validation gate. Every accepted decision records the reviewer and
claim-specific reasoning.

{anchor}`meta.reviews.gate`
The shared `sage-meta check` fails when any current specification anchor lacks
a current acceptable review in `review-checklist.json`, or when that file is
missing or invalid. CI and the workspace Cargo integration test use this same
gate. Diagnostics identify the anchors requiring attention and direct the
author to the review skill; checking does not create approvals.

## Reviewing the test bodies

Use the repository's `sage-review-checklist` skill to prepare and review the
changed claims. The coordinator assigns every pending anchor to a reviewer in
small related groups and reconciles the findings before recording decisions.
Reviewers consider the complete associated test set and explain which
assertions exercise which parts of the claim. They identify a plausible
violation and whether those assertions would detect it.

The scenario, expected outcome, and assertion should be clear from the body of
the test function itself. Parsing, fixture construction, and setup helpers are
reasonable. A call such as `verify_everything()` that hides the meaningful
assertions is insufficient evidence: request a clearer test body instead of
approving it from its name or annotation. Reviewers may read helpers to
understand the test, but helper bodies are not currently saved review inputs.

This boundary is deliberate. A helper edit alone does not invalidate a saved
review. Running the tests can detect behavioral changes, but the cached review
only establishes what the snapshotted test visibly checks. An explicit way to
include additional functions in the reviewed inputs can extend that boundary
when needed.

## Current Status

The review checklist extends the existing Markdown anchor and Rust test
inventory. `review prepare`, `review record`, and the shared check implement
extraction, stale-input detection, input-checked recording, and the validation
gate. `crates/sage-meta/tests/review.rs` exercises these behaviors through the
library and CLI, including mixed placeholder decisions and rejected
submissions that preserve the prior file. The
[rollout](../../rfds/sage-contracts/implementation.md#review-checklist)
records completion of the initial review. After the Crates integration,
`review-checklist.json` contains 51 approved claims and 34 reviewed TODO entries.
The shared check accepts all
85 current snapshots while keeping those statuses distinct. `.sage` rule
extraction remains part of the [book migration](./book.md); this checklist does
not introduce a second `.sage` anchor syntax.
