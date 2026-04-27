# Agent Guidelines for Sage

## Architecture documentation

The `md/` directory contains an mdbook with sage's design docs and RFDs.

**You MUST keep these documents up-to-date as the codebase evolves.** When you
make a change that affects the architecture, pipeline, key decisions, or current
state described in any doc under `md/`, update the relevant doc in the same
commit.

Specifically:

- **`md/design/arch.md`** — the big-picture architecture. Update when adding
  new pipeline stages, changing the overall approach, or introducing major new
  components.
- **`md/rfds/`** — planning documents. Update the active RFD's "Current state"
  and "Next steps" sections as work progresses. When an RFD's scope is complete,
  mark it as such and ensure the design docs reflect the final state.

## Rust conventions

- Use `cargo fmt` after modifying Rust source files.
- Run `cargo build` (at minimum) before presenting results.
- See `.kiro/skills/rust-best-practice/SKILL.md` for additional Rust guidelines.
