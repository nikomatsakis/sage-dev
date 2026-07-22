# Maintaining This Book

This page is the **update contract** for the AI agent (and human) that edits this book.
Much of sage is developed conversationally with an agent, drawing on the source code and
design documents. This book is a **source of truth** for sage's design and status, so it
only stays useful if it moves in lockstep with the work. When the design or the system
changes, update the docs in the *same* commit — treat it as part of the change, not a
follow-up.

The rest of this page is written in the imperative, addressed to that agent.

## How the pieces relate — the Architecture section and RFDs

Sage keeps two kinds of design document. They are complementary views of the same system:
**RFDs describe the journey, the architecture pages describe the destination.**

| | Architecture & Design | RFD |
|---|---|---|
| Describes | the *destination* — the intended architecture as a whole | a *change* — the journey to get there |
| Answers | *how is the system meant to be built?* | *what change are we making, and why?* |
| Lifespan | living, kept current | historical once completed |

An **RFD** proposes and discusses a change. That change is often architectural — an RFD
can describe how the architecture itself should evolve, not only an implementation plan and
its steps. The **Architecture & Design** section records the destination those changes
converge on, as a single present-and-intended picture, so the current design never has to
be reconstructed by replaying every RFD.

Two practical consequences:

- **An RFD can carry architectural design, not just implementation steps.** Don't force
  architectural reasoning out of an RFD; that is a normal part of proposing a change.
- **Keep the destination current.** When an RFD lands an architectural change, reflect the
  agreed end-state in the relevant architecture page so this section stays the coherent
  whole. A reader must be able to tell built from planned (the **Build-Out Roadmap**
  carries the authoritative done/in-flight/planned status).

## When to update — trigger → page

When one of these happens, update the matching page(s) before you consider the work done:

| Trigger | Update these pages |
|---|---|
| We settle (or revise) how part of the system *should* be designed | The relevant [architecture](../design/README.md) page — record the intended destination, marking anything not yet built as planned |
| We're planning a change (including an architectural one) | Open an **RFD** (`md/rfds/<name>/`) per the [RFD process](../rfds/README.md) to describe and discuss it; track steps in its `implementation.md`. When it lands an architectural change, reflect the end-state in the [architecture](../design/README.md) page |
| A draft RFD is added | List it under *Draft* in [`SUMMARY.md`](../SUMMARY.md); mark unsettled mechanisms as planned in any destination page |
| An RFD's implementation step lands | Tick the step in that RFD's `implementation.md` — this is the only place per-step status lives; do **not** touch the roadmap |
| An RFD is accepted (merged, in progress) | Move it to *Accepted* in [`SUMMARY.md`](../SUMMARY.md) and [`accepted.md`](../rfds/accepted.md); flip its group to **In flight** in the [Build-Out Roadmap](../implementation/roadmap.md) |
| An RFD completes | Move it to *Completed* in [`SUMMARY.md`](../SUMMARY.md) and [`completed.md`](../rfds/completed.md); update the relevant [architecture](../design/README.md) page; flip its group to **Done** in the [Build-Out Roadmap](../implementation/roadmap.md) |
| Observable behavior changes / something ships | The relevant [architecture](../design/README.md) page (reconcile it with what now exists) |
| A new subsystem, flow, or mechanism is built | Add/update the matching [architecture](../design/README.md) page |
| A cross-cutting, load-bearing decision is made or changed | Add/update an entry in [Architecture decisions](../design/decisions.md) with a new `D<n>` code; a feature-local decision stays in its RFD and is linked from there |
| A new term worth defining | [`terminology.md`](../terminology.md) |
| Any new page | Register it in [`SUMMARY.md`](../SUMMARY.md) — a page not listed there does not render |

When a change touches more than one row, update all of them in the same change.

## Conventions

- **Architecture pages describe the intended design, and separate built from planned.** They
  may cover parts not yet implemented — that is the point of the section. But mark planned
  design clearly (a status note, or defer the detail to the Build-Out Roadmap) so nothing
  reads as built when it isn't.
- **Ground built claims in the code.** For anything described as existing, tie statements to
  actual modules/files and keep references accurate. Planned design is grounded in the
  design discussion instead, and is labelled as planned.
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
