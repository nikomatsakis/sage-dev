# Language Subsetting

Sage intentionally supports a subset of Rust. This is a pragmatic choice — each
restriction eliminates significant implementation complexity while affecting
little real-world code. Restrictions are documented here with rationale and may
be lifted as sage matures.

## Restrictions

### No proc-macro crates defined in the workspace

**What:** Sage does not support workspace crates with `proc-macro = true` in
their `[lib]` target. Proc-macro crates from external dependencies (e.g.,
`serde_derive`, `tokio_macros`, `clap_derive`) are fully supported — sage calls
them via `proc_macro::bridge` like any other consumer.

**Why:** A workspace-defined proc-macro requires compiling it to a host-side
dylib before it can be expanded. This means sage would need to invoke rustc (or
itself) to produce a working dylib, manage host vs target compilation, and
handle the case where the proc-macro crate is being edited (invalidating the
dylib). This is a significant amount of machinery for a feature that most
application-level workspaces don't use — proc-macros are typically published as
separate crates.

**Impact:** Low. Application workspaces (web services, CLI tools, libraries)
almost never define proc-macros inline. Projects that do (e.g., a proc-macro
crate + its consumer in one workspace) can still use sage for the consumer
crate — just not for the proc-macro crate itself.

### No glob imports from workspace modules

**What:** `use some_workspace_module::*` is not supported. Glob imports from
external dependencies (`use std::collections::*`, `use serde::*`) work fine.

**Why:** Glob imports between workspace modules are the primary source of
fixed-point iteration in name resolution. Without them, every `use` import
names exactly what it brings in, and name resolution becomes a single
deterministic pass. With them, you need to iterate: resolve module A's exports
to know what B's glob imports, but A might glob from C, which might glob from
B...

Glob imports from external deps don't have this problem — the dep snapshot
already has complete, fully-resolved module contents. Looking up `std::io::*`
is a single query into the `TyCtxt` snapshot.

**Impact:** Low. Glob imports between workspace modules are uncommon in
practice. The most common glob pattern is `use MyEnum::*` inside a function
body (enum variant shorthand) — this is a *local* glob, not a cross-module one,
and sage can support it trivially. Cross-module globs like `use crate::types::*`
are easy to rewrite as explicit imports.

**Future:** This restriction can be lifted incrementally. A simple iterative
pass (not full fixed-point) handles the common case of non-cyclic glob chains.
Full fixed-point is only needed for pathological cases (A globs B, B globs A).

## Supported features

Everything not listed above is intended to be supported, including:

- async/await
- Trait definitions and implementations
- Generics, lifetimes, where clauses
- Pattern matching
- Closures and `impl Fn` / `dyn Fn`
- Derive macros (from external crates)
- Proc-macro attributes (from external crates), e.g. `#[tokio::test]`
- `macro_rules!` definitions and invocations within the workspace
- Module tree (`mod`, `pub use`, `pub(crate)`)
- Type aliases, constants, statics
- `cfg` attributes
