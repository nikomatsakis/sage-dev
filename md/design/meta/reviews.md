# Reviewing Specification Tests

The committed `review-checklist.json` records which specification claims and
associated tests have been reviewed together. `sage-meta` extracts the inputs
and checks whether a recorded decision still applies. An agent reviews whether
the test bodies express an adequate check of the claim. Running the tests
remains a separate obligation.

## Review inputs

Claim snapshots use the shared
[content boundaries](anchors.md#anchor-content), including later paragraphs,
lists, tables, and examples within an anchor. Changing any included block
changes the review input; prose beyond a closing boundary does not.

{anchor}`claim_text`
An anchor's review text is the complete source content selected by the
[anchor-content boundaries](anchors.md#anchor-content), with surrounding
whitespace removed. For `.sage` prose, its enclosing common indentation is
removed first; remaining internal whitespace and Markdown markup are preserved.
The saved input covers the same content that the generated anchor views
present as the claim, including any paragraphs, lists, or examples within its
extent.

{anchor}`test_source`
Each associated test's review input contains the complete annotated Rust
function source, including its attributes, signature, body, and contained
comments. It also records the Cargo crate and target, Rust path, file path,
inherited configuration conditions, and ordinary, `todo`, or `not_applicable`
status. Source line numbers and byte offsets are excluded from the input.

{anchor}`complete_test_set`
One review covers one anchor's claim and its complete set of associated tests.
The input records every eligible test associated with that anchor in a
deterministic order. Several tests may cover different parts of a claim; each
test need not establish the whole claim independently.

{anchor}`source_representation`
Claim and annotated test source snapshots are stored in the checklist JSON as
arrays of source lines. Joining an array with newline characters recovers the
snapshot's text, including internal blank lines, whitespace, and markup.

{anchor}`review_scope`
Implementation excerpts provide review context but are excluded from the
saved claim-and-test approval input. Changing only implementation source while
preserving valid associations does not invalidate an otherwise current review.

The checklist records the adequacy of the claim-to-test relationship. Source
conformance and test execution require their own evidence under the
[evidence-status rule](anchors.md#meta.anchors.evidence_status).

## Saved decisions

{anchor}`staleness`
A saved review applies only when its anchor, complete claim-and-test input,
schema version, and review-policy version match the current inventory. A
changed claim, changed test source or metadata, or added or removed test
association makes the review stale. Movement that changes only line numbers or
byte offsets does not require another review.

{anchor}`preparation`
Preparing a review checklist emits current input snapshots for anchors without
a current acceptable decision. Preparation does not change the committed
checklist or replace the inputs of a previous decision. Anchors with a current
acceptable decision are omitted from the work to be reviewed.

{anchor}`recording`
Recording review decisions checks each submitted input against a fresh
inventory before updating the committed checklist. A mismatched input,
duplicate anchor, unknown anchor, missing reviewer or reasoning, or unfinished
decision rejects the submission without changing the saved checklist. A
successful submission replaces the submitted anchors' decisions and preserves
other current anchors' saved reviews, removing entries for retired anchors.

{anchor}`decisions`
An `approved` decision requires that every associated test is ordinary and
records that those tests visibly check the claim. A `todo` decision requires at
least one `todo` test and accounts for the complete test set; a
`not_applicable` decision requires every associated test to have that status.
These two decisions account for explicit placeholders without claiming
behavioral evidence. A `pending` or `needs_work` decision cannot satisfy the
validation gate. Every accepted decision records the reviewer and
claim-specific reasoning.

{anchor}`gate`
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

## Implementation and test evidence

The generated [anchor pages](../anchor/meta/reviews.html) report implementation entries,
annotated tests, explicit TODO/N/A gaps, and saved claim/test reviews. These
associations do not themselves establish that a test ran.

The [earlier coverage snapshot](../../rfds/implementation-sequencing/coverage-history.md#specification-test-review)
preserves the former capability descriptions, edit experiments, and limitations
as historical evidence. Current coverage comes from the generated entries;
implementation plans and decision rationale belong in the relevant RFDs.
