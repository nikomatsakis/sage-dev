# Agent Guidelines for Sage

## Architecture documentation

**Before doing any work in this repository**, read
[`md/contributing/maintaining-the-docs.md`](md/contributing/maintaining-the-docs.md).
It defines the required specification, implementation, test, and review workflow.

**Before starting design or coding work**, read `md/design/README.md` and `md/design/crates/README.md` and
any relevant architecture pages under `md/design/`. This ensures your work
aligns with the intended system design. If an accepted RFD covers the area
you're working in, read that too (`md/rfds/<name>/README.md`).

**New development ALWAYS starts with the specification:** establish the
testable anchors and `.sage` model first, creating or updating them as needed.
Only afterwards edit implementation code and connect it and its tests to those
anchors.

**Most bug fixes should have an associated anchor.** Before changing code,
identify the claim the bug violates; add or refine an anchor if the intended
behavior is missing or incomplete. Add a regression test associated with that
anchor.

Keep specification, code, tests, and reviews up to date in the same change:

- **`md/design/`** describes the destination; anchors state independently
  testable claims, including those beside modeled entities in `.sage` files.
- **`md/rfds/`** records rationale, alternatives, and implementation ordering;
  normative changes belong in Design, with affected pages linked from the RFD.
- **Generated Anchors** derive status from Rust associations, TODO/N/A modules,
  and reviewed claim/test snapshots. Update those sources, not manual status
  tables. Use the review-checklist skill whenever its inputs change.

## Rust conventions

- Use `cargo fmt` after modifying Rust source files.
- Run `cargo build` (at minimum) before presenting results.
- See `.agents/skills/rust-best-practice/SKILL.md` for additional Rust guidelines.
