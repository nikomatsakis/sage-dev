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

`rustc_interface` requires a crate to compile. We feed it a dynamically
generated stub with `extern crate` declarations for each direct dep:

```rust
#![crate_type = "lib"]
#![allow(unused_extern_crates)]
extern crate serde;
extern crate clap;
// ...
```

The key insight is matching **cargo's own `--extern` pattern**:

- `--extern name=path` for **direct deps only** (not transitive)
- `-L dependency=<target/debug/deps>` so rustc resolves transitive deps itself

Rustc finds transitive deps by reading each rlib's embedded metadata, which
records dependencies by hash. It matches the exact rlib in the search path.
This avoids sysroot version conflicts entirely — rustc never falls back to
sysroot copies because the hash match is exact.

**Do NOT pass `--extern` for every transitive dep.** This causes E0460 errors
when a transitive dep also exists in the sysroot with a different hash. Cargo
doesn't do this and neither should we.

By `after_expansion`, every dep (direct and transitive) is loaded into `TyCtxt`
and queryable.

### Pipeline (target state)

1. `cargo metadata` → workspace members, resolved dep graph, direct deps
2. `cargo build --message-format=json` → ensure rlibs exist, collect paths
3. Stub crate + `-L dependency` + `--extern` (direct only) → `TyCtxt`
4. Extract dep snapshot into sage's own IR
5. tree-sitter parse workspace crates
6. Name resolution against dep snapshot
7. Type inference + trait solving via `rustc_next_trait_solver`

## Current state

- **Toolchain:** pinned to `nightly-2026-03-15` with `rustc-dev` component
- **CLI:** `sage (-p CRATE)*` via clap. No `-p` = all workspace members.
- **Cargo metadata integration:** parses workspace members, resolved dep graph,
  identifies direct normal deps of selected crates
- **Dep building:** shells out to `cargo build --message-format=json`, collects
  rlib paths for direct deps
- **Stub driver:** generates a temp stub with `extern crate` for each direct
  dep, passes `-L dependency=<deps_dir>` + `--extern` for direct deps only.
  Loads all deps (direct + transitive) into `TyCtxt` at `after_expansion`.
- **Dep stats:** walks `tcx.crates(())` + `tcx.module_children()`, counts items
  by `DefKind`. Self-test: 46 crates loaded, 0 errors.
- **Tree-sitter parsing:** parses workspace `.rs` files, counts AST nodes by
  kind. Self-test: 2 files, 460 lines, 4234 nodes.

## Next steps

1. Design sage's own type IR (owned types decoupled from `'tcx`)
2. Name resolution for workspace crates against the dep snapshot
3. Wire up `rustc_next_trait_solver` via `Interner` impl

## Out of scope (for now)

- Proc macro expansion
- Borrow checking
- Codegen
- Multi-file / module resolution
- Incremental compilation
