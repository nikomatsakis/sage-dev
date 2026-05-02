---
name: sage-architecture
description: "Understanding the sage codebase architecture. Activate this skill at the start of any session working on sage to load architectural context. Also activate before committing to check if design docs need updating."
---

# Sage Architecture Skill

## When starting a session

Read these design docs to understand the codebase:

1. **`md/design/arch.md`** — Pipeline overview, crate structure, TcxDb, testing strategy
2. **`md/design/ir.md`** — IR layers (items, syntactic bodies, resolved bodies), resolution algorithm, display conventions
3. **`md/design/subsetting.md`** — What's supported, what's not, and why

If there's a `WIP.md` in the repo root, read it too — it contains the current work-in-progress plan and implementation status.

## Key files to know

### Entry points
- `src/driver.rs` — `run_sage_with`: sets up rustc, TcxDb, salsa Database
- `src/main.rs` — CLI entry point

### sage-ir (the core)
- `crates/sage-ir/src/lib.rs` — `Db` trait, module list
- `crates/sage-ir/src/item.rs` — all item tracked structs
- `crates/sage-ir/src/body.rs` — syntactic body types (in Stash)
- `crates/sage-ir/src/resolved.rs` — resolved body types (in Stash)
- `crates/sage-ir/src/body_resolve.rs` — `BodyResolver` + `resolve_body` salsa tracked fn
- `crates/sage-ir/src/resolve.rs` — module-level name resolution
- `crates/sage-ir/src/lower.rs` — tree-sitter CST → IR lowering
- `crates/sage-ir/src/display.rs` — Display/PrettyPrint for all IR types
- `crates/sage-ir/src/tcx/mod.rs` — `TcxDb` trait (external crate metadata)

### Testing
- `tests/body_resolve_tests.rs` — resolved body snapshot tests (real TcxDb)
- `tests/expand_tests.rs` — module resolution + derive integration tests
- `crates/sage-ir/tests/snapshot_tests.rs` — signature + body snapshots
- `crates/sage-ir/tests/expand_tests.rs` — resolution unit tests

## When committing changes

Check if your changes affect any of these and update the corresponding doc:

| If you changed... | Update... |
|---|---|
| Pipeline, query graph, crate structure | `md/design/arch.md` |
| IR types, body types, resolution algorithm | `md/design/ir.md` |
| What's supported/unsupported | `md/design/subsetting.md` |
| TcxDb trait methods | `md/design/arch.md` (TcxDb section) |
| Display format | `md/design/ir.md` (Display section) |

Design docs should be high-level pointers, not code dumps. If a code
snippet in a doc no longer matches the source, update or remove it.
