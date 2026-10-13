# Agent Guidelines for Sage

## Architecture documentation

The `md/` directory contains an mdbook with sage's design docs and RFDs.

**Before starting design or coding work**, read `md/design/README.md` and `md/design/crates/README.md` and
any relevant architecture pages under `md/design/`. This ensures your work
aligns with the intended system design. If an accepted RFD covers the area
you're working in, read that too (`md/rfds/<name>/README.md`).

The full update contract is in `md/contributing/maintaining-the-docs.md` — read
it before editing the book.

**You MUST keep these documents up-to-date as the codebase evolves.** When you
make a change that affects the architecture, pipeline, key decisions, or current
state described in any doc under `md/`, update the relevant doc in the same
commit.

Specifically:

- **`md/design/`** — architecture pages describe the *destination*. Update when
  adding new pipeline stages, changing the overall approach, or introducing major
  new components.
- **`md/rfds/`** — change and decision records. Keep rationale, alternatives,
  affected Design links, and implementation ordering here. The same change
  updates the normative Design specification. Preserve historical decisions
  in an existing RFD or an explicitly retrospective record.
- **Generated Anchors** — implementation and test status comes from Rust
  associations, TODO/N/A modules, and reviewed claim/test snapshots. Update
  those sources rather than writing a second manual status table. Use the
  review-checklist skill when its inputs change.

## Rust conventions

- Use `cargo fmt` after modifying Rust source files.
- Run `cargo build` (at minimum) before presenting results.
- See `.agents/skills/rust-best-practice/SKILL.md` for additional Rust guidelines.
