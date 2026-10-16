---
name: sage-review-checklist
description: Review Sage implementations and tests for each anchor reported by cargo meta check, capturing inputs and recording complete reports.
---

# Review specification implementations and tests

Follow `md/design/meta/reviews.md`. A passing test run or annotation alone does
not establish implementation completeness or test adequacy.

## Coordinator: delegate each failing anchor

Run `cargo meta check --json` or read the shared Cargo validation test failure.
Diagnostics identify each anchor requiring review and why. Fix structural source
errors before starting reviews. Do not weaken claims, delete associations, or
reclassify ordinary evidence as TODO merely to make the gate pass.

Start one review subagent per anchor, including TODO/N/A anchors. Give each the
anchor ID and the initial instruction to follow this skill, run
`cargo meta review start ANCHOR`, and review the printed inputs. Only one agent
owns an anchor's validation directory at a time; different anchors may be
reviewed concurrently. Use the available agent slots and dispatch remaining
anchors as slots become free. Avoid supplying uncaptured source context.
Additional coordinator instructions must be saved with `start --instructions`.

## Reviewer: capture initial context

From the workspace run:

```sh
cargo meta review start ANCHOR
# With additional coordinator instructions saved verbatim in a text file:
cargo meta review start ANCHOR --instructions /tmp/review-instructions.txt
```

Start resets `validations/ANCHOR/`, writes summary and initial input files, and
prints the same inputs and verification prompt. Read them. The gate now fails
until a complete current report with no requested edits is recorded. An
interrupted review cannot fall back to a previous approval.

The initial fragments contain the complete claim, all associated annotated test
functions, and associated implementation source files, plus association metadata.
Identify and capture every additional source dependency before using it in the
assessment:

```sh
cargo meta review read ANCHOR path/to/file.rs
cargo meta review read ANCHOR path/to/file.rs --lines 10 40
cargo meta review read ANCHOR md/design/meta/reviews.md --claim meta.reviews.gate
cargo meta review read ANCHOR crates/example/tests/check.rs --test example test:check example::verifies_result
```

Paths are workspace-relative. Line bounds are one-based and inclusive. Claim and
test selectors resolve identifiers rather than line numbers. The command saves
the exact selected contents before returning them. All fragments use the same
representation and freshness checks. Dependency completeness is agent-declared.

## Reviewer: evaluate every aspect

1. Enumerate every requirement, qualification, edge case, and scenario in the
   complete claim. Trace each to implementation behavior and actual test
   assertions, or explain justified TODO/N/A evidence. Follow relevant helpers
   and capture them. Add implementation associations for essential code in
   other files so future association changes invalidate review.
2. Read the complete associated test set. Different tests can cover different
   aspects, but names, annotations, comments, and passing results alone are
   insufficient. Meaningful assertions must be visible in annotated test bodies;
   opaque `verify_everything()` wrappers do not establish adequacy.
3. Investigate suspected bugs with an anchor-associated regression test and run
   it. Preserve a reproduced failure, including test name, command, expected
   behavior and actual result, and request concrete edits. Compilation or tool
   failures are not behavioral reproductions. Do not weaken expectations.
4. If a concern cannot be reproduced, or you cannot determine how to test it,
   report it to the coordinator for user consultation. Leave the review
   unfinished; a passing attempt does not resolve the concern. Continue other
   independent work.
5. Write a complete, anchor-specific report distinguishing source inspection
   from commands actually executed. Account for every implementation/test entry
   and all requirements. Explain each TODO/N/A rationale and prerequisite; an
   accepted deferral remains visibly deferred, not implemented.

Reviewers may add regression tests and implementation associations, coordinating
shared files with the main agent. Production changes are coordinated by the
main agent. Any changes to reviewed inputs require a fresh start and review.

## Reviewer: record the report

Write a separate JSON report file containing only:

```json
{
  "reviewer": "reviewer identity",
  "report": "Complete claim-specific explanation with code and test evidence.",
  "requested_edits": []
}
```

An empty edit list means all aspects are accounted for and no edits are needed.
Otherwise list concrete, nonblank edits. Edits may request tests or an explicit
TODO test with a comment documenting its prerequisite. Never submit an empty or
generic report merely to pass the gate. TODO/N/A classification comes from the
associated evidence; do not duplicate it in the output.

```sh
cargo meta review record ANCHOR /tmp/review-report.json
```

Record validates the saved inputs against current source and preserves them
exactly. On stale-input rejection, start again and review the fresh inputs;
never transplant a verdict without checking the changed context. A recorded
report requesting edits remains failing. Return the report and requested edits
to the coordinator. Do not edit saved JSON files directly.

## Coordinator: follow up and check

Reconcile findings and coordinate requested changes. After changes, delegate
fresh start/read/record reviews for affected anchors. Run relevant tests and
`cargo meta check` again. Every anchor needs a current completed report with no
requested edits. The coordinator cannot substitute its own draft for a
subagent's completed recording. Keep unresolved reviews failing and report them
accurately. Do not automatically publish the change.

For retired anchors, remove the obsolete validation directory. For renamed
anchors, reconcile the specification and associations, remove the old directory,
and review the new ID. Renaming a saved directory does not establish approval.
Unsupported records require a fresh `start`; checking never silently prunes them.
