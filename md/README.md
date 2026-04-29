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

Sage can parse Rust source files using tree-sitter and lower them into a
salsa-based IR that captures:

- All item kinds (functions, structs, enums, traits, impls, type aliases,
  consts, statics, modules, use declarations)
- Function signatures (parameters, return types, async/unsafe)
- Function bodies (expressions, statements, patterns)
- Attributes and doc comments
- Struct fields, enum variants

This IR is tested against the [mini-redis](https://github.com/tokio-rs/mini-redis)
codebase with snapshot tests that verify zero error or missing nodes.

**Not yet implemented:** name resolution, type checking, macro expansion,
module file discovery.
