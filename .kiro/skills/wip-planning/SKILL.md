---
name: wip-planning
description: "How to write WIP planning docs. Activate this skill when creating a new WIP.md or when asked to plan a multi-phase feature."
---

# WIP Planning Doc Skill

## When to create a WIP.md

Create `WIP.md` in the repo root when starting a feature that spans
multiple phases or touches multiple crates. Delete it when the work
is complete.

## Required sections

Every WIP doc must have these sections in order:

### 1. Goal
What we're building and why. Include a concrete target (e.g., "expand
`#[derive(Parser)]` on mini-redis's CLI structs and snapshot the result").

### 2. Background
Technical context needed to understand the approach. API surfaces,
data structures, prior art. Include code snippets from upstream sources
when they clarify the design.

### 3. Architecture
How the pieces fit together. ASCII diagrams or mermaid for data flow.
Call out threading, unsafe, and key design decisions.

### 4. Implementation plan
Phased, with TDD: tests listed first, then implementation steps.
Each phase should be independently committable.

#### Per-phase structure
- **Goal** — one sentence
- **Files** — what's created or modified
- **Tests** — exact test code (copy-paste ready)
- **Implement** — numbered steps
- **Commit message** — exact message to use

### 5. Documentation updates
**This section is mandatory.** List which design docs each phase affects:

```markdown
## Documentation updates

| Phase | Doc | Section to update |
|---|---|---|
| Phase 2 | `md/design/arch.md` | TcxDb trait listing |
| Phase 4 | `md/design/subsetting.md` | Macro expansion restrictions |
| Phase 4 | `md/design/ir.md` | Display section |
```

This ensures doc updates happen at commit time, not as an afterthought.
The sage-architecture skill has the full mapping of changes → docs.

### 6. FAQ
Questions that came up during design, with answers. Helps future
readers (and future AI sessions) understand non-obvious choices.

### 7. What's NOT in scope
Explicit boundaries. Prevents scope creep and sets expectations.

### 8. Implementation status
Checkboxes per phase, plus:
- **Deviations from plan** — what changed and why
- **Open issues** — problems discovered during implementation

## Process (per phase)

1. Write failing tests first
2. Implement until tests pass
3. Run `cargo fmt` + `cargo test --all --workspace`
4. Update design docs listed in the "Documentation updates" table
5. Update WIP.md status (check off phase, note deviations)
6. Commit with the phase's commit message

## Style

- Code snippets should be copy-paste ready
- Prefer ASCII diagrams over prose for data flow
- Keep the doc self-contained — a new session should be able to
  pick up from the implementation status section without reading
  the git log
