---
name: rustc-driver-frontend
description: "Use this skill when building an alternate Rust compiler frontend, analyzer, or tool that needs to consume dependency metadata (types, traits, impls, MIR) from Rust crates without decoding rlibs by hand. Covers the rustc_driver custom-driver pattern, hooking rustc_interface callbacks, extracting TyCtxt data for foreign crates, and architectural choices around Salsa interning vs arena storage for solver types. Use whenever the user is designing a Rust compiler, type checker, verifier, or analysis tool that needs rustc's view of dependencies — especially if they mention rlibs, rmeta, cargo workspaces, the next trait solver, rustc_public, stable_mir, rustc_type_ir, or consuming Rust crate metadata. Do NOT use this skill for writing plain rustc plugins like Clippy lints (standard rustc_driver custom-driver docs suffice) or for rust-analyzer contributions (rust-analyzer has its own architecture docs)."
---

# rustc_driver-as-metadata-oracle: building an alternate Rust frontend

## When this applies

You're building something that reads Rust code at a semantic level — types, traits, impls, possibly MIR — and you need to handle real-world Rust, which means handling the stdlib and transitive dependencies. Options considered and their tradeoffs:

- **Decode rlibs directly.** Don't. The format is explicitly unstable (`METADATA_VERSION` byte + exact `rustc_version()` string match at load, both reject cross-version mismatches), the on-disk bytes are instances of rustc-internal types (`Ty<'tcx>`, `mir::Body`, interned `Symbol`/`Span`), and no third-party tool has ever shipped a standalone decoder. Every serious consumer — Miri, Clippy, Kani, Charon, Creusot, MIRAI, Prusti — runs in-process via `rustc_driver`.
- **Re-parse sources like rust-analyzer.** Viable but enormous scope. You reimplement rustc's name resolution, trait solver, and type inference, and you handle stdlib's `#[lang]` items, `#[rustc_intrinsic]` fns, `PhantomData`/`UnsafeCell`/`Pin` specialness, allocator shims, coroutine lowering, async desugaring. Rust-analyzer has been grinding through this for years.
- **rustc_driver custom driver (this skill).** Use rustc to load and digest dep metadata; do your own frontend work on top of the resulting `TyCtxt`. Foreign metadata comes pre-digested: `#[lang]` items wired up, intrinsics typed, trait impl tables populated. Nightly-only, `rustc_private`-coupled, requires rebasing every few weeks. The universal choice among real consumers.

This skill is for the third option.

## The three architectural shapes

Pick based on what the user needs to do:

**Shape A — Wrapper.** Your tool runs per-rustc-invocation via `RUSTC_WORKSPACE_WRAPPER`. Cargo invokes it once per crate. `TyCtxt` contains only the current crate + its direct deps. Best for: lints, per-crate analyses, anything that integrates into a normal `cargo build`. This is Clippy/Kani/Miri.

**Shape B — Workspace-level driver.** Your tool is a `cargo` subcommand. You invoke `cargo metadata` yourself to get the resolved dep graph, then spin up `rustc_interface` **once** with a stub crate and `--extern` flags for every transitive dep in the workspace. You process all workspace sources in one process. Best for: batch compilers, whole-program analyzers, anything where per-crate rustc startup overhead dominates.

**Shape C — Hybrid.** Wrapper mode for correctness/integration, workspace-level for speed. Skip unless you're sure you need both.

The sections below cover Shape B (the interesting one). Shape A is a simplification — skip the stub-crate and `cargo metadata` plumbing, hook `after_analysis` instead of `after_expansion`, return `Compilation::Continue` to let Cargo's artifact expectations be satisfied.

## The stub-crate trick (Shape B)

`rustc_interface` requires a crate to compile — `TyCtxt` is constructed inside the compilation pipeline, not standalone. Feed it a trivial one:

```rust
// stub/lib.rs
#![crate_type = "lib"]
```

Invoke rustc with:

```
rustc --edition=2021 --crate-type=lib \
    --extern serde=/path/to/libserde-abc123.rlib \
    --extern tokio=/path/to/libtokio-def456.rlib \
    ... (every dep in the workspace-wide resolved set) \
    stub/lib.rs
```

Cost is negligible: one line of parsing, empty name resolution, empty HIR lowering. The real work — `creader` populating `CStore` from every `--extern` — is exactly what you want. By `after_expansion`, every transitive dep is loaded into `TyCtxt` and queryable.

Get dep paths from `cargo metadata --format-version 1`: `resolve.nodes[].deps[]` gives the resolved dep graph per target, with `dep_kinds` distinguishing normal / build / dev / proc-macro. For a type-only frontend, only target-side normal deps matter.

Ensure deps are built: run `cargo build` (or `cargo check` with `--emit=metadata`) with your pinned nightly first so rlibs exist. Cargo caches them in `target/`; subsequent runs are free.

## Callback hook points

`rustc_driver::Callbacks` has four hook methods. Pick the earliest one that gives you what you need:

- `config(&mut Config)` — before compilation. Set `override_queries`, inject `--cfg` flags, override the `MetadataLoader`.
- `after_crate_root_parsing` — AST exists, no name resolution yet. `CStore` is **not** fully populated. Rarely useful.
- `after_expansion` — macros expanded, name resolution done, `CStore` fully populated, HIR lowered, `TyCtxt` exists. **This is the sweet spot for dep-metadata consumers.** You get everything about foreign crates, haven't paid for local type-check/borrow-check.
- `after_analysis` — type-check and borrow-check complete. Use if you want to compare against rustc's own analysis of the stub (or workspace crate in Shape A).

Return `Compilation::Stop` from your hook to skip remaining phases (Shape B; Cargo doesn't care because you're the top-level driver). Return `Compilation::Continue` to let rustc finish (Shape A; Cargo expects artifacts).

Skeleton:

```rust
#![feature(rustc_private)]
extern crate rustc_driver;
extern crate rustc_interface;
extern crate rustc_middle;
extern crate rustc_hir;
extern crate rustc_span;

use rustc_driver::{Callbacks, Compilation, run_compiler};
use rustc_interface::interface::Compiler;
use rustc_middle::ty::TyCtxt;

struct MyDriver {
    // your frontend state, populated during callback
}

impl Callbacks for MyDriver {
    fn after_expansion<'tcx>(
        &mut self,
        _compiler: &Compiler,
        tcx: TyCtxt<'tcx>,
    ) -> Compilation {
        self.extract_dep_snapshot(tcx);
        self.run_frontend(tcx);
        Compilation::Stop
    }
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let mut driver = MyDriver { /* ... */ };
    run_compiler(&args, &mut driver);
}
```

## Metadata is loaded lazily — lean into it

The `LazyValue` / `LazyArray` / `LazyTable` scheme means foreign rmeta stays on disk (mmap'd) until you touch it. Concretely:

- `CrateHeader` and `CrateRoot` struct decode eagerly at `CStore` load (identity check, dep list).
- `tcx.type_of(foreign_did)`, `tcx.fn_sig(...)`, `tcx.predicates_of(...)`, `tcx.optimized_mir(...)` — each does a single `LazyTable::get(i)` into the mmap, decodes, interns into `'tcx` arenas, caches in the query system. Subsequent calls hit the cache.
- Spans and foreign `SourceFile` data are the laziest: you pay for spans only when you format diagnostics.

This means: **walk only what you care about**. Memory grows monotonically over the callback's life but tops out at the fraction of foreign metadata your frontend actually inspects.

One caveat: impl lookup is per-(trait, crate), not per-(trait, item). The first `tcx.all_impls(trait_did)` call walks every foreign crate that has any impl of that trait, building an index. Subsequent calls are O(1). Budget a one-time scan per trait your frontend cares about; never a scan over everything.

## Enumerating foreign crates

`TyCtxt` doesn't expose "all `DefId`s in crate N" directly — walk the module tree from each crate root:

```rust
use rustc_hir::def_id::{CrateNum, DefId, CRATE_DEF_INDEX};
use rustc_hir::def::DefKind;

for &cnum in tcx.crates(()) {
    let root = DefId { krate: cnum, index: CRATE_DEF_INDEX };
    visit(tcx, root);
}

fn visit<'tcx>(tcx: TyCtxt<'tcx>, did: DefId) {
    for child in tcx.module_children(did) {
        let Some(child_did) = child.res.opt_def_id() else { continue };
        // record (child_did, tcx.def_kind(child_did), child.vis, ...)
        match tcx.def_kind(child_did) {
            DefKind::Mod => visit(tcx, child_did),
            DefKind::Enum | DefKind::Struct | DefKind::Trait => {
                // recurse into associated items via tcx.associated_items
            }
            _ => {}
        }
    }
}
```

Per-item queries that are commonly useful: `type_of`, `fn_sig`, `generics_of`, `predicates_of`, `explicit_item_bounds`, `adt_def`, `trait_def`, `impl_trait_header`, `associated_items`, `visibility`, `def_path_str`, `attrs`, `codegen_fn_attrs`. For impls: `all_impls(trait_did)`, `non_blanket_impls_for_ty(trait_did, self_ty)`, `inherent_impls(adt_did)`.

## The `'tcx` lifetime problem

`Ty<'tcx>`, `GenericArgs<'tcx>`, `mir::Body<'tcx>`, and friends are arena-allocated with lifetime tied to `TyCtxt`. You **cannot** persist them past the callback — the arenas are dropped when the compiler session ends.

Options for crossing the boundary:

1. **Do all frontend work inside the callback.** Your frontend runs, finishes, emits results. Simplest — use this unless you have a reason not to. You can absolutely run a full type-check over your workspace inside `after_expansion` and exit after.
2. **Translate into your own IR during the callback.** Walk the foreign items once, copy what you need into owned types (your own `Ty`, `Sig`, etc.). Then `Compilation::Stop` returns and your frontend proceeds on owned data.
3. **Use `rustc_public` (formerly `stable_mir`).** Stable, owned representations of types and MIR. Requires nightly at runtime but gives SemVer stability on the Rust-API surface. Missing pieces as of early 2026: trait-solver queries (Charon stayed on raw `rustc_private` for this reason).

Option 1 is the default. Option 2 is for "my frontend is a serious tool with its own architecture and rustc is just the loader."

## Storage strategy (if you're using Salsa)

If your frontend is Salsa-based (e.g., you're mimicking rust-analyzer's query architecture), **don't naively Salsa-intern your `Ty` representation**. Rust-analyzer tried this during their Chalk→next-solver migration and measured 648 MB + 31s of overhead on their own self-build. PRs #21295/#21307 (Dec 2025) moved solver types off Salsa interning onto a bulk-reset "GC" scheme.

The right split for a Salsa-based frontend:

- **Salsa-intern** stable cross-query things: `DefId`, `CrateId`, `AdtDef`, `Name`, span info, resolved paths. These benefit from Salsa's dedup + stable id + incremental tracking.
- **Arena-allocate** solver churn: `Ty`, `Const`, `Region`, `GenericArgs`, `Binder<_>`, `Predicate`. Type inference generates blizzards of intermediate values that get interned once and never reused. Session-lifetime arena is the right fit.

For a **batch** compiler (no revisions, process exits at end), this collapses to: one arena, lifetime = session, "GC" = `drop(arena)` at exit. Which is exactly rustc's own `'tcx` model — you've arrived back at rustc's memory design for the same reasons rustc picked it. `Interner::Ty` can just be `&'sess TyData<'sess>`, optionally with a `FxHashSet` for structural dedup on insertion.

## Using the next trait solver

`rustc_next_trait_solver` + `rustc_type_ir` are the reusable solver/type-IR layer now shared between rustc and rust-analyzer. Published on crates.io as `ra-ap-rustc_type_ir` / `ra-ap-rustc_next_trait_solver` (rust-analyzer's fork, tracks upstream closely, works on stable Rust).

To use it: implement the `Interner` trait (~40 associated types, ~40 methods) on a `Copy` handle to your database. Rust-analyzer's `DbInterner<'db>` is the reference implementation — it's a thin wrapper around a `&'db dyn HirDatabase` and every method delegates to a Salsa query. Your equivalent delegates to whatever mix of Salsa queries and in-callback `TyCtxt` queries you have.

The `Interner` associated types (`Ty`, `Const`, `Region`, `GenericArgs`, etc.) are your interned handles; the `inherent::*` traits define the operations the solver calls on them. See `compiler/rustc_type_ir/src/interner.rs` and `inherent.rs` for the trait surface, and rust-analyzer's `crates/hir-ty/src/next_solver/` for a working implementation.

The solver's entry points take a `TypingMode<I>` (`Coherence` / `Analysis` / `Borrowck` / `PostAnalysis`) — pick per-query based on what you're doing.

**Rust-analyzer's migration is complete** (October 2025, changelog #299, PR #20329 + ~40 companion PRs). The abstraction is production-proven. Being the third downstream is a much better risk profile than it was during the migration.

## Plumbing

**`rustc_private` linkage.** Your crate needs `#![feature(rustc_private)]` and `extern crate rustc_*;` for every rustc crate you use. Link against `librustc_driver-*.so` in `$(rustc --print sysroot)/lib`. `rustc_private` is viral — any dependent that links a `rustc_private` crate must also enable the feature.

**Toolchain pin.** Nightly-only, pinned via `rust-toolchain.toml`:

```toml
[toolchain]
channel = "nightly-2026-XX-XX"
components = ["rustc-dev", "rust-src", "llvm-tools"]
```

**Rebase cadence.** Expect weekly-to-monthly breakage. Charon pins nightly in `charon-pin` and bumps deliberately. Miri and Clippy are in-tree with rustc so never drift. For out-of-tree tools, budget automation time for the bump loop.

**Dep-rustc-version matching.** If Cargo built deps with stable rustc 1.85 and your tool's embedded rustc is nightly-2026-03-15, the rlibs won't load (`rustc_version()` mismatch on rmeta). Two options:
1. Build deps with your pinned nightly (what Kani/Miri/Clippy do as rustup components).
2. Invoke `cargo build` from your tool using `rustup run <pinned-nightly>` to force consistency.

**Cargo artifact expectations (Shape A only).** If you're wrapping and `Compilation::Stop`-ing, Cargo sees missing artifacts and thinks the build failed. Either let rustc complete (simplest — you extract metadata as a side effect), or emit a stub `.rmeta` / rlib to satisfy Cargo. See `rustc_codegen_ssa::back::metadata::create_compressed_metadata_file` for how to wrap bytes into a valid object-file.

## What rustc gives you vs what you still build

**Gives you (from loaded rlibs, via `TyCtxt`):**
- Foreign crate parsing + `CStore` population
- Name resolution into foreign crates (via `module_children` + `Res`)
- Foreign type info, trait impls, coherence
- Foreign MIR for generic and `#[inline]` fns (non-generic non-inline: no MIR encoded; available only via object symbols)
- Stdlib: lang items, intrinsics, compiler-magic types (`PhantomData`, `Pin`, allocator shims)
- Sysroot injection, target/cfg handling, proc-macro dylib loading

**You still build:**
- Parsing of workspace crates (use `syn` for independence, or `rustc_parse` if you don't mind coupling)
- Macro expansion of workspace crates (your own `macro_rules!` engine; proc-macros via `proc_macro::bridge` or delegate to rustc)
- Name resolution, type inference, trait selection for workspace crates (solver query against the foreign impl set — the next solver handles this if you wire `Interner` correctly)
- Borrow check (if you want it)
- Codegen (if you want it — or hand MIR to `rustc_codegen_cranelift` / `rustc_codegen_gcc`)

## Hazards not to step in

**Don't try to decode rmeta out-of-process.** Every part of the decoding stack (`DecodeContext`, shorthand caches, `LazyTable` indexing, span-tag compaction, `CrateNum` remapping) presupposes you're running inside the same rustc build that produced the rlib. This is the single most common wrong turn.

**Don't assume non-generic non-inline fns have MIR.** They don't, by default. If you need MIR for all fns (e.g., for interpretation like Miri), either force `-Zalways-encode-mir` on deps (requires custom sysroot — painful with Cargo) or recompile deps yourself.

**Don't hold `Ty<'tcx>` past the callback.** The arena dies with the compiler session. Copy into your own IR or `rustc_public` types if you need persistence.

**Don't ignore proc-macro-vs-target dep duplication.** `cargo metadata`'s `resolve.nodes[].deps[].dep_kinds` distinguishes host-side proc-macro dylibs from target-side rlibs. Same crate can appear twice; you want only the target-side rlib for `--extern`.

**Don't Salsa-intern solver types.** If you're using Salsa, the performance trap is real and well-documented. Arena is right for this.

## Source-tree pointers

For reading rustc internals (all paths in `rust-lang/rust` master):

- `compiler/rustc_driver_impl/src/lib.rs` — `Callbacks` trait, `run_compiler`
- `compiler/rustc_interface/src/interface.rs` — `Config`, `Compiler`
- `compiler/rustc_metadata/src/rmeta/{mod,encoder,decoder,table}.rs` — metadata schema + encoding
- `compiler/rustc_metadata/src/creader.rs` — `CStore`, `CrateLoader`, `MetadataLoader` trait
- `compiler/rustc_metadata/src/locator.rs` — how rustc finds rlibs (doc comment is the best prose overview)
- `compiler/rustc_type_ir/src/{interner,inherent}.rs` — `Interner` trait surface
- `compiler/rustc_next_trait_solver/src/solve/mod.rs` — solver entry points

For reference implementations:

- `src/tools/miri/` — in-tree custom driver, `-Zalways-encode-mir` usage
- `src/tools/clippy/clippy_driver/` — simplest custom-driver pattern
- `src/tools/rust-analyzer/crates/hir-ty/src/next_solver/` — `Interner` impl, `DbInterner`, Salsa integration (but note the solver types are NOT Salsa-interned, see `crates/intern/src/gc.rs`)
- Out-of-tree: Charon (github.com/AeneasVerif/charon) and Kani (github.com/model-checking/kani) — both do rustc_driver + workspace-level analysis

## Documentation

- rustc-dev-guide: https://rustc-dev-guide.rust-lang.org/rustc-driver/intro.html (driver), https://rustc-dev-guide.rust-lang.org/backend/libs-and-metadata.html (rlib format)
- `rustc_public` tracking: rust-lang/project-stable-mir
