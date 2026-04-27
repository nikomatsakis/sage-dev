# RFD: Initial Setup — Stub Driver for Dependency Metadata

**Status:** In progress

## Goal

Build an alternative Rust compiler ("sage") optimized for the fastest possible
path from editing a test to seeing the result. The first milestone is proving we
can load and inspect dependency metadata from real Rust crates.

## Approach

Sage uses **Shape B** from the `rustc_driver` architecture: a workspace-level
driver that feeds a stub crate to `rustc_interface`, loads all dependency
metadata into `TyCtxt`, and then does its own frontend work on top.

### Why Shape B

Per-crate rustc startup overhead dominates in a tight edit→check loop. By
loading all transitive deps once in a single process, we pay the `CStore`
population cost exactly once and can compile workspace crates ourselves with
maximum control over what work gets done.

### Key decisions

| Decision | Choice | Rationale |
|---|---|---|
| Parsing | tree-sitter | Speed + stability, decoupled from nightly churn |
| Trait solving | `rustc_next_trait_solver` | Production-proven (rust-analyzer shipped Oct 2025), avoids reimplementing coherence/specialization |
| Type storage | Arena-allocated | Batch compiler, no incremental — same design as rustc for the same reasons |
| Dep metadata | `rustc_driver` stub crate | Only viable path; rmeta format is unstable and undocumented |

### The stub-crate trick

`rustc_interface` requires a crate to compile. We feed it a trivial one:

```rust
// stub/lib.rs
#![crate_type = "lib"]
```

With `--extern` flags for every transitive dep, `creader` populates `CStore`
from every rlib. By `after_expansion`, every dep is loaded into `TyCtxt` and
queryable. Cost is negligible — one line of parsing, empty name resolution.

### Pipeline (target state)

1. `cargo metadata` → resolved dep graph + rlib paths
2. `cargo check` with pinned nightly → ensure rlibs exist
3. Stub crate + `--extern` flags → `TyCtxt` with all dep metadata
4. Extract dep snapshot into sage's own IR
5. tree-sitter parse workspace crates
6. Name resolution against dep snapshot
7. Type inference + trait solving via `rustc_next_trait_solver`

## Current state

- **Toolchain:** pinned to `nightly-2026-03-15` with `rustc-dev` component
- **CLI:** `sage (-p CRATE)*` via clap. No `-p` = all workspace members.
- **Cargo metadata integration:** parses workspace members, resolved dep graph,
  identifies transitive external deps (excluding proc-macros)
- **Dep building:** shells out to `cargo build --message-format=json`, collects
  rlib paths for external deps
- **Stub driver:** generates a temp stub with `extern crate` for each dep,
  passes `--extern` flags. Loads deps into `TyCtxt` at `after_expansion`.
  Tolerates partial load failures (sysroot version conflicts) via
  `catch_fatal_errors`.
- **Dep stats:** walks `tcx.crates(())` + `tcx.module_children()`, counts items
  by `DefKind`. Self-test: 38 crates, 23K+ items.
- **Tree-sitter parsing:** parses workspace `.rs` files, counts AST nodes by
  kind. Self-test: 2 files, 521 lines, 4794 nodes.

### Known limitations

- Crates that overlap with the sysroot (e.g., `hashbrown`, `regex`, `memchr`)
  may fail to load due to version conflicts. This is inherent to running a
  rustc driver with `--extern` flags for crates that also exist in the sysroot.
  Affects sage-on-sage more than typical target workspaces.
- Proc-macro crates are excluded from `--extern` (correct — they're host-side
  dylibs), but their non-proc-macro transitive deps are also excluded (may miss
  some deps that are shared between proc-macro and normal dep trees).

## Next steps

1. Design sage's own type IR (owned types decoupled from `'tcx`)
2. Name resolution for workspace crates against the dep snapshot
3. Wire up `rustc_next_trait_solver` via `Interner` impl
4. Improve dep loading: resolve sysroot conflicts by providing complete
   transitive closure of `--extern` flags

## Out of scope (for now)

- Proc macro expansion
- Borrow checking
- Codegen
- Multi-file / module resolution
- Incremental compilation
