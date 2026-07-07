# Agent Guidelines for Sage

## Architecture documentation

The `md/` directory contains an mdbook with sage's design docs and RFDs. The
full update contract is in `md/contributing/maintaining-the-docs.md` — read it
before editing the book.

**You MUST keep these documents up-to-date as the codebase evolves.** When you
make a change that affects the architecture, pipeline, key decisions, or current
state described in any doc under `md/`, update the relevant doc in the same
commit.

Specifically:

- **`md/design/`** — architecture pages describe the *destination*. Update when
  adding new pipeline stages, changing the overall approach, or introducing major
  new components.
- **`md/design/decisions.md`** — cross-cutting architecture decisions (`D<n>`).
  Add a new entry when a load-bearing decision is made or changed.
- **`md/rfds/`** — planning documents. Each RFD is a directory
  (`rfds/<name>/README.md` + `implementation.md`). Update the accepted RFD's
  `implementation.md` as work progresses. When an RFD's scope is complete, move
  it from Accepted to Completed in `SUMMARY.md` and update the architecture
  pages to reflect the final state.
- **`md/implementation/roadmap.md`** — cross-RFD status and ordering. Update
  when an RFD is accepted, completed, or its group dependencies change.

## Rust conventions

- Use `cargo fmt` after modifying Rust source files.
- Run `cargo build` (at minimum) before presenting results.
- See `.kiro/skills/rust-best-practice/SKILL.md` for additional Rust guidelines.
