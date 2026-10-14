# Reviewing Specification Implementations and Tests

The committed `review-checklist.json` records which specification claims and
associated implementations and tests have been reviewed together. `sage-meta`
extracts the inputs and checks whether a recorded decision still applies. An
agent reviews whether the implementation completely satisfies the claim
and whether the test bodies demonstrate its functionality. Running the tests
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
Claim, implementation, and annotated test source snapshots are stored in the checklist JSON as
arrays of source lines. Joining an array with newline characters recovers the
snapshot's text, including internal blank lines, whitespace, and markup.

{anchor}`review_scope`
One review judges both implementation completeness and test adequacy against
an anchor's complete claim. Its saved input includes every implementation
association's crate, target, module, file, cfg conditions, excerpt, region flag,
placeholder status, and explanation, in deterministic order, plus the complete
source of each associated implementation file, stored once per file. Line
numbers and byte offsets are not saved. Full files prevent a point marker's
five-line preview from silently bounding the code reviewed. Adding, removing,
or changing an association or any associated file makes the review stale.

Code outside those files is not a cached input. Reviewers follow relevant
helpers and dependencies and add implementation associations for essential
code in other files before approval. Reviews are source judgments, not proofs;
test execution remains separate evidence.

## Saved decisions

{anchor}`staleness`
A saved review applies only when its anchor, complete claim, implementation,
and test input, schema version, and review-policy version match the current
inventory. A
changed claim, changed implementation or test source or metadata, or added or
removed association makes the review stale. Moving a claim without changing
its content does not require review. Edits within a snapshotted implementation
file, including blank-line changes, conservatively invalidate its reviews.
Approvals under the previous test-only policy cannot satisfy this policy.

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
An `approved` decision requires ordinary implementation and test entries and
records that the implementation fully satisfies the claim and the tests
visibly demonstrate it. A `todo` decision requires at least one TODO entry on
either side and accounts for all implementation and test entries. A
`not_applicable` decision requires every entry on both sides to have that
status. These two decisions acknowledge explicit gaps without claiming
implemented behavior. A `pending` or `needs_work` decision cannot satisfy the
validation gate. Every accepted decision records the reviewer and claim-specific
reasoning for both implementation completeness and test adequacy.

{anchor}`gate`
The shared `sage-meta check` fails when any current specification anchor lacks
a current acceptable review in `review-checklist.json`, or when that file is
missing or invalid. CI and the workspace Cargo integration test use this same
gate. Diagnostics identify the anchors requiring attention and direct the
author to the review skill; checking does not create approvals.

## Reviewing implementation and tests

Use the repository's `sage-review-checklist` skill to prepare and review the
changed claims. The coordinator assigns every pending anchor to a reviewer in
small related groups and reconciles the findings before recording decisions.
Reviewers trace every requirement to implementation code, checking missing
cases and contradictory behavior. They consider the complete associated test
set and explain which assertions exercise which parts of the claim. They identify a plausible
violation and whether those assertions would detect it.

The scenario, expected outcome, and assertion should be clear from the body of
the test function itself. Parsing, fixture construction, and setup helpers are
reasonable. A call such as `verify_everything()` that hides the meaningful
assertions is insufficient evidence: request a clearer test body instead of
approving it from its name or annotation. Reviewers may read helpers to
understand the test, but helper bodies outside implementation files are not saved review inputs.

A helper edit outside the associated implementation files and annotated test
bodies does not invalidate a saved review. Running the tests can detect
behavioral changes, but the cached review covers only the snapshotted source.
Add implementation associations to include additional essential source files.

When reviewers suspect a bug, they add an anchor-associated regression test
that exercises the intended behavior and run it against the implementation.
A reproduced behavioral failure yields `needs_work`, with the test, command,
and observed failure in the reasoning. Do not treat a build or environment
failure as reproduction. If a test cannot reproduce the concern, or a reviewer
cannot work out how to write one, consult the user with the concern and attempts;
leave the affected review unresolved until the concern is resolved. New tests
or associations require preparing fresh inputs before recording a decision.

## Implementation and test evidence

The generated [anchor pages](../anchor/meta/reviews.html) report implementation entries,
annotated tests, explicit TODO/N/A gaps, and saved implementation/test reviews. These
associations do not themselves establish that a test ran.

The [earlier coverage snapshot](../../rfds/implementation-sequencing/coverage-history.md#specification-test-review)
preserves the former capability descriptions, edit experiments, and limitations
as historical evidence. Current coverage comes from the generated entries;
implementation plans and decision rationale belong in the relevant RFDs.
