# Maintaining This Book

This page is the **update contract** for the AI agent (and human) that edits this book.
Much of sage is developed conversationally with an agent, drawing on the source code and
design documents. This book is a **source of truth** for sage's design and status, so it
only stays useful if it moves in lockstep with the work. When the design or the system
changes, update the docs in the *same* commit — treat it as part of the change, not a
follow-up.

The rest of this page is written in the imperative, addressed to that agent.

## How the pieces relate — the Architecture section and RFDs

Sage keeps three complementary views of the system:

- **Architecture pages describe the destination and the local current
  status.** Their main text states how the system is meant to work. A final
  **Current Status** section states what works today, concrete limitations,
  inspectable evidence, and related roadmap slices.
- **The build-out roadmap describes cross-cutting implementation slices.** A
  slice has an acceptance target, scope, ordering, dependencies, affected
  architecture, and a high-level implementation plan.
- **RFDs describe a change and its detailed checkpoints.** They record why a
  design was chosen and how that scoped change lands.

| | Architecture & Design | Build-Out Roadmap | RFD |
|---|---|---|---|
| Describes | destination, local status, limitations, evidence | cross-cutting slices and their ordering | one change and its detailed checkpoints |
| Answers | *how should this work, what works now, and how can I inspect it?* | *what coherent outcome are we building next?* | *why this change, and how does it land?* |
| Lifespan | living, kept current | living, kept current | historical once completed |

An **RFD** proposes and discusses a change. That change is often architectural — an RFD
can describe how the architecture itself should evolve, not only an implementation plan and
its steps. The **Architecture & Design** section records the destination those changes
converge on, as a single present-and-intended picture, so the current design never has to
be reconstructed by replaying every RFD.

Two practical consequences:

- **An RFD can carry architectural design, not just implementation steps.** Don't force
  architectural reasoning out of an RFD; that is a normal part of proposing a change.
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
| We're planning a change (including an architectural one) | Open an **RFD** (`md/rfds/<name>/`) per the [RFD process](../rfds/README.md) to describe and discuss it; track steps in its `implementation.md`. When it lands an architectural change, reflect the end-state in the [architecture](../design/README.md) page |
| A draft RFD is added | List it under *Draft* in [`SUMMARY.md`](../SUMMARY.md); mark unsettled mechanisms as planned in any destination page |
| An RFD's implementation step lands | Tick the step in that RFD's `implementation.md`. Update architecture status or roadmap progress only if the step changes those facts |
| An RFD is accepted (merged, in progress) | Move it to *Accepted* in [`SUMMARY.md`](../SUMMARY.md) and [`accepted.md`](../rfds/accepted.md); link it from any roadmap slice it implements |
| An RFD completes | Move it to *Completed* in [`SUMMARY.md`](../SUMMARY.md) and [`completed.md`](../rfds/completed.md); update affected architecture Current Status sections and roadmap-slice progress |
| Observable behavior changes / something ships | The relevant architecture **Current Status** section: current frontier, limitations, and evidence |
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
- **Ground built claims in the code.** For anything described as existing, tie statements to
  actual modules/files and keep references accurate. Planned design is grounded in the
  design discussion instead, and is labelled as planned.
- **Make evidence claim-specific.** Link each important built claim to a
  focused test, readable snapshot, query trace, edit experiment, exact oracle
  result, inspector command, or code anchor. “The test suite passes” is not
  sufficient evidence for a specific architectural guarantee.
- **Include implementation excerpts with ezanchor.** Put matching
  `// ANCHOR: name` and `// ANCHOR_END: name` comments around the smallest
  useful source region in one of the `scan-dirs` configured in `book.toml`,
  then reference it with an ezanchor block:

  ````text
  ```{anchor}
  name
  ```
  ````

  This emits both the excerpt and its GitHub source link. Do not
  hand-copy Rust implementation snippets into walkthroughs.
- **Diagrams use Mermaid** in fenced ` ```mermaid ` blocks.
- **Style** (inherited from the RFD process): no promotional or dramatic language; be factual
  and brief; lead with concrete concepts, then generalize; include examples.

## Verify before you commit

Build the book locally — this is the same check CI runs on the PR, so it catches problems
before you push:

```bash
mdbook build   # or `mdbook serve` to preview and rebuild on save
```

The build surfaces the two things easiest to get wrong:

- **Orphaned pages** — a `.md` under `md/` that isn't listed in [`SUMMARY.md`](../SUMMARY.md)
  warns ("not linked to in SUMMARY.md") and won't render. Add every new page to `SUMMARY.md`
  in the same edit that creates it.
- **Broken links** — `mdbook-linkcheck2` will report dead internal links; CI fails on these.

Then confirm the new/edited page appears in the sidebar and that content and intra-book links
resolve, and commit.
