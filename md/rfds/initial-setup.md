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
- **Stub driver:** working. Implements `Callbacks::after_expansion`, walks all
  foreign crates via `tcx.crates(())` + `tcx.module_children()`, prints module
  tree with `DefKind` annotations
- **Loads sysroot:** 19 crates (std, core, alloc, compiler_builtins, libc, etc.),
  21K+ items enumerated
- **No `--extern` plumbing yet** — only sysroot deps are loaded

## Next steps

1. Add `cargo metadata` integration to discover workspace dep rlib paths
2. Pass `--extern` flags to load real dependencies
3. Begin designing sage's own type IR (owned types decoupled from `'tcx`)
4. tree-sitter parsing of workspace `.rs` files

## Out of scope (for now)

- Proc macro expansion
- Borrow checking
- Codegen
- Multi-file / module resolution
- Incremental compilation
