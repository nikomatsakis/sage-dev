---
name: sage-review-checklist
description: Review new or changed Sage specification claims against their annotated Rust tests and record decisions in review-checklist.json when sage-meta check reports missing or stale reviews.
---

# Review Sage specification tests

Review the exact claim and complete associated test set before recording a
decision. The saved conclusion concerns whether those test bodies visibly
check the claim; it does not prove implementation conformance or replace test
execution. Follow `md/design/meta/reviews.md` for the extraction and freshness
contract.

## Prepare the work

From the repository root, run:

```bash
cargo run -p sage-meta -- review prepare > /tmp/sage-review.json
```

The output contains current inputs for anchors needing review. Preparation
does not update `review-checklist.json`. If structural validation prevents
preparation, fix the reported source problems within the authorized task
before reviewing. Do not delete annotations, weaken claims, or reclassify
ordinary tests as placeholders merely to make the gate pass.

Dispatch review subagents with small, related groups of anchors. Give each
subagent the exact input records and repository access, and request one result
per assigned anchor. Assign every emitted anchor explicitly, including TODO
and N/A entries. Reviewers may inspect helpers and implementation context, but
must not modify code or edit the shared checklist as part of their review.

## Evaluate each anchor

For each record, the reviewer should:

1. Identify the independently testable assertions in the claim. Consider all
   associated tests together; different tests may cover different assertions.
2. Read the actual supplied test bodies. Identify the scenario, expected
   outcome, and assertions that exercise the claim. A test name, annotation,
   explanatory comment, or passing result alone is insufficient.
3. Describe a plausible violation and whether these assertions would detect
   it. Report assertions or important cases that lack evidence.
4. Require the meaningful check to be clear from the body of the annotated
   test itself. Fixture construction, parsing, and setup helpers are fine;
   `verify_everything()` that hides the scenario and assertions is not. Read
   helpers when needed for understanding, but their contents are not cached
   review inputs. If the test body is insufficient, request restructuring.
5. Run focused tests when useful, distinguishing observed results from source
   inspection. Do not infer a run from a saved review or test annotation.
6. Return the anchor, decision, reviewer identity, and claim-specific reasoning
   citing the relevant test names and assertions. Include concrete follow-up
   work for an unsuccessful review.

Use these decisions:

- `approved`: all associated tests are ordinary and their bodies collectively
  express an adequate check of the claim.
- `todo`: at least one associated test is in a `todo` module. Explain the
  unfinished behavior and account for any ordinary or N/A tests as well. This
  accepts the explicitly recorded gap, not the behavior as implemented.
- `not_applicable`: every associated test is in a `not_applicable` module and
  the recorded explanation justifies why executable evidence does not apply.
- `needs_work`: the evidence is inadequate or contradictory, or a placeholder
  explanation is not justified. State the smallest useful follow-up.

Never promote TODO or N/A entries to `approved`. Do not reuse one generic
approval rationale across unrelated claims. If review cannot finish, leave
the entry pending and report it as unfinished.

## Reconcile and record

The coordinator checks that every assigned anchor has a result, reconciles
conflicting findings, and reviews each decision's reasoning. Preserve every
input snapshot exactly. Populate only `decision`, `reason`, and `reviewer` on
completed records and write them to a separate JSON submission, preserving
the generated schema and policy versions. A submission may contain a subset
of completed anchors; omit unfinished entries instead of approving them.

```bash
cargo run -p sage-meta -- review record /tmp/sage-reviewed.json
cargo run -p sage-meta -- check
```

Recording reloads the current sources and rejects stale inputs. If it rejects
a source change, prepare and review the changed input again; do not copy the
new source into an old approval. Recording a `needs_work` finding is useful
but does not pass the gate. Resolve it through actual claim/test changes and
another review, or report the remaining blocker accurately.

Inspect the `review-checklist.json` diff, run relevant tests independently,
and report the decisions and any remaining gaps. Keep the updated checklist
with the claim/test changes for the user's normal commit workflow; do not
automatically commit or publish it.
