---
name: sage-review-checklist
description: Review Sage implementation completeness and test adequacy against specification claims and record decisions when sage-meta check reports missing or stale reviews.
---

# Review Sage implementations and tests

Review the exact claim, associated implementation code, and complete associated
test set before recording a decision. Judge whether the anchor is fully and
correctly implemented and whether the tests demonstrate its functionality.
Source review does not replace test execution. Follow
`md/design/meta/reviews.md` for the snapshot and freshness contract.

## Prepare the work

From the repository root, run:

```bash
cargo run -p sage-meta -- review prepare > /tmp/sage-review.json
```

Preparation emits current inputs for anchors needing review without updating
`review-checklist.json`. Fix structural validation errors within the authorized
scope before reviewing. Do not delete annotations, weaken claims, or reclassify
ordinary tests as placeholders merely to make the gate pass.

Dispatch review subagents with small, related groups of anchors. Give each the
exact input records and repository access; request one result per anchor.
Assign every emitted anchor explicitly, including TODO and N/A entries.
Give reviewers separate test-file ownership or isolated worktrees when
reproduction tests might overlap. Reviewers may add regression tests and
implementation associations, but must not change production behavior or edit
the shared checklist as part of review. Coordinate shared-file changes through
the parent agent.

## Evaluate each anchor

1. Identify every requirement in the complete claim. Read all associated
   implementation entries and their snapshotted source files. Trace each
   requirement to actual behavior; check missing cases, edge cases, and
   contradictions. An association or passing test does not establish
   implementation completeness. Follow relevant helpers and dependencies. Add
   associations for essential implementation code in other files so later edits
   invalidate approval; a five-line point preview is only navigation context.
2. Read the supplied test bodies as a set. Identify the scenario, expected
   outcome, and assertions demonstrating each requirement. Different tests may
   cover different parts. Names, annotations, comments, and passing results
   alone are insufficient. Meaningful checks must be visible in the annotated
   bodies; setup helpers are fine, but opaque `verify_everything()` calls are not.
3. Investigate potential bugs by adding an anchor-associated regression test
   asserting the specified behavior and running it against the implementation.
   Use an existing test target where practical. Preserve the test and report a
   reproduced behavioral failure as `needs_work`, citing its name, exact
   command, expected behavior, and observed failure. Compilation, tooling, or
   environment failures do not count as reproducing the suspected bug. Do not
   weaken the claim or test expectation to obtain a passing result.
4. If the concern cannot be reproduced with a test, or you cannot work out how
   to write the test, consult the user: explain the suspected violation, what
   was attempted, and what remains uncertain. Subagents report this to the
   coordinator for consultation. Leave that review unresolved; a passing attempt
   alone does not resolve the concern. Continue independent reviews meanwhile.
5. Distinguish source inspection from tests actually run. Return the anchor,
   decision, reviewer identity, and claim-specific reasoning covering both
   implementation completeness (with code locations) and test adequacy (with
   test names and assertions). Include reproduction results and follow-up work
   for unsuccessful reviews.

Use these decisions:

- `approved`: all implementation and test entries are ordinary, the implementation
  fully satisfies the claim, and the tests collectively demonstrate it.
- `todo`: at least one implementation or test entry is in a `todo` module.
  Explain the unfinished behavior and account for all entries on both sides.
  This accepts the explicitly recorded gap, not the behavior as implemented.
- `not_applicable`: every implementation and test entry is in a
  `not_applicable` module and the explanations justify inapplicability.
- `needs_work`: implementation or tests are incomplete, a bug is reproduced,
  or a placeholder explanation is unjustified. State the smallest useful follow-up.

Never promote TODO or N/A entries to `approved`. Do not reuse generic approval
reasoning across unrelated claims. If review cannot finish, leave it pending
and report it as unfinished.

## Reconcile and record

The coordinator checks every assigned result and reconciles conflicts. Preserve
input snapshots exactly. If reviewers added tests or associations, prepare again
and review the fresh affected records before recording; never transplant a
verdict onto changed inputs without checking them. Populate only `decision`,
`reason`, and `reviewer` on completed records, and write a separate JSON
submission preserving schema and policy versions. Omit unfinished entries.

```bash
cargo run -p sage-meta -- review record /tmp/sage-reviewed.json
cargo run -p sage-meta -- check
```

Recording reloads sources and rejects stale inputs. After rejection, prepare
and review again; do not copy new source into an old approval. A recorded
`needs_work` finding fails the gate. Resolve it through actual implementation
or test changes and another review, or report the remaining blocker accurately.

Inspect the checklist diff, run relevant tests independently, and report
outcomes and remaining gaps. Keep the checklist with the implementation/test
changes for the user's normal commit workflow; do not automatically publish it.
