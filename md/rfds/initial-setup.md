# RFD: Initial Setup — Stub Driver for Dependency Metadata

**Status:** In progress

## Goal

Build an alternative Rust compiler ("sage") optimized for the fastest possible
path from editing a test to seeing the result. The first milestone is proving we
can load and inspect dependency metadata from real Rust crates.

The target use case: given a `#[test]` function, resolve all names in its body,
expand any macros it encounters, and (eventually) type-check it — pulling in
only what's needed, demand-driven.

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
| Type storage | Arena-allocated | Batch compiler — same design as rustc for the same reasons. Salsa for incremental tracking of inputs/items, NOT for solver types. |
| Dep metadata | `rustc_driver` stub crate | Only viable path; rmeta format is unstable and undocumented |
| Name resolution | Single-pass, no fixed-point | Glob imports from deps supported (already resolved). Workspace-to-workspace globs deferred. |
| Proc macros | On-demand via `proc_macro::bridge` | Call proc-macro dylibs individually as needed, like rust-analyzer |
| Incrementality | Salsa-based, resident process | Long-lived server keeps Salsa DB across edits. CLI communicates with server. |

### Toolchain and packaging

Sage is installed as `cargo-sage` (enables `cargo sage` subcommand). At build
time, `build.rs` captures the sysroot path and embeds it as:

- **`SAGE_SYSROOT` env** — so sage can find `bin/rustc` to pass as `RUSTC` to
  cargo when building target workspace deps
- **rpath** — so the dynamic linker finds `librustc_driver-*.dylib` at runtime

This means the binary is tied to the specific rustup toolchain it was compiled
with. The toolchain must remain installed. The binary is not relocatable across
machines.

When building target workspace deps, sage sets `RUSTC=<sysroot>/bin/rustc` to
force cargo to use the exact same rustc that's linked into sage. This
guarantees rlib metadata version compatibility.

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
2. `cargo build --message-format=json` (with `RUSTC` set) → ensure rlibs exist
3. Stub crate + `-L dependency` + `--extern` (direct only) → `TyCtxt`
4. Extract dep snapshot into Salsa DB
5. tree-sitter parse workspace crates → ItemTree per file (Salsa-tracked)
6. Name resolution against dep snapshot + workspace ItemTrees
7. Proc-macro expansion on demand
8. Type inference + trait solving via `rustc_next_trait_solver`

## Current state

- **Toolchain:** pinned to `nightly-2026-03-15` with `rustc-dev` component
- **CLI:** `cargo sage (-p CRATE)*`. No `-p` = all workspace members.
- **Sysroot:** embedded at compile time via `build.rs`. rpath for dylib loading.
- **Dep building:** sets `RUSTC` to sage's own sysroot rustc, shells out to
  `cargo build --message-format=json`, collects rlib paths for direct deps.
- **Stub driver:** generates temp stub with `extern crate` for each direct dep,
  passes `-L dependency=<deps_dir>` + `--extern` for direct deps only.
- **Dep stats:** walks `tcx.crates(())` + `tcx.module_children()`, counts items
  by `DefKind`.
- **Tree-sitter parsing:** per-file item listing — structs (field counts), enums
  (variant counts), impls (method counts, trait), functions (async), uses, mods,
  consts, types, derives.
- **Tested on mini-redis:** 69 crates loaded (including tokio, tracing, bytes,
  async-stream), 20 source files, 0 errors.

## Target project: mini-redis

Language features used:
- async/await (44 async fns, 103 await sites)
- Derives: Debug, Clone, Default, Parser, Subcommand
- Trait impls: Iterator, From, Display, Error, PartialEq, Drop
- Proc-macro attributes: `#[tokio::main]`, `#[tokio::test]`, `#[instrument]`
- No `macro_rules!`, no custom trait definitions, no workspace glob imports
- Clean module tree with explicit `pub use` re-exports

## Next steps

1. Salsa DB + FileId + ItemTree — the foundation for incremental analysis
2. Name resolution: single-pass, resolve paths against dep snapshot + ItemTrees
3. Proc-macro bridge: call into proc-macro dylibs on demand
4. Body resolution: walk a test function's body, resolve every path, expand macros

## Out of scope (for now)

- Borrow checking
- Codegen
- Incremental compilation (Salsa DB is the foundation, but the resident
  process / file watcher comes later)
- Workspace-to-workspace glob imports
