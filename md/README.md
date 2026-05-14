# Sage

Sage is an alternative Rust analysis tool optimized for the fastest possible
path from editing a test to seeing the result. It operates on a deliberately
restricted subset of Rust, trading generality for speed.

## Goals

- **Demand-driven analysis.** Given a `#[test]` function, resolve only the
  names and types that test actually touches. Don't analyze the whole crate.
- **Incremental from the ground up.** Built on [salsa](https://github.com/salsa-rs/salsa)
  so that editing a function body doesn't re-analyze signatures, and editing
  one file doesn't re-analyze unrelated files.
- **Real dependency metadata.** Use `rustc_driver` to load `.rlib`/`.rmeta`
  files for external crates, getting the same view of dependencies that `rustc`
  has — no stubs, no approximations.

## Current status

Sage parses Rust source files with tree-sitter, lowers them into a
salsa-based IR, and resolves names end-to-end:

- **Lowering.** All item kinds (functions, structs, enums, traits,
  impls, type aliases, consts, statics, modules, use declarations),
  function signatures, function bodies (expressions, statements,
  patterns), attributes, doc comments.
- **Module discovery.** `mod foo;` resolves to `foo.rs` or
  `foo/mod.rs` on demand; inline `mod foo { ... }` is also handled.
- **Macro expansion.** `macro_rules!` invocations expand inside the
  expanded-module pipeline; expansions feed back into the same
  resolution machinery as source-level items.
- **Name resolution.** Use redirects (`use foo::bar`), glob imports
  (`use foo::*`), `crate::` / `self::` / `super::` paths, the extern
  prelude, and the `std::prelude::v1::*` injection.
- **Derive resolution and expansion.** Builtin derives generate
  synthesized impls; proc-macro derives are dispatched through
  `rustc_driver`'s loaded dylibs.
- **Body resolution.** Local variables, function parameters, and
  paths inside function bodies resolve to `Symbol` / `LocalId`.

The pipeline runs against [mini-redis](https://github.com/tokio-rs/mini-redis)
end-to-end, with snapshot tests covering both signatures and resolved
bodies.

**Not yet implemented:** type checking, method resolution, trait
selection. These are the next milestones on the roadmap.
