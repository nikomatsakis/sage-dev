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

Body type checking and inference are implemented, including retained body
obligations, limited inherent/trait method resolution, reachable external impl
discovery, and associated-type normalization for the pinned mini-redis
`Parse::next` slice. Unsuffixed numeric expressions retain integer or float
family constraints without defaulting. Ground binary operators use ordinary
core trait impls and complete as resolved static trait calls; generic operator
associated bindings and inference from an initially unknown `Self` remain on
the roadmap alongside general method coverage, higher-ranked solving, and
specialization/GATs.

## Inspecting semantic results

Run the local Semantic Inspector for the current workspace with:

```bash
cargo sage inspect
```

It opens an Axum-served browser application on `127.0.0.1:2442`. Use
`--package <crate>` to select the one library target in a multi-library
workspace; an ambiguous bare invocation fails instead of silently choosing a
crate. Use `--port <port>` to override the listener or `--no-open` to leave
browser launch to the caller. The
[Semantic Inspector architecture chapter](design/crates/sage_inspector/inspector-guide.md)
describes its symbol navigation, structural products, query evidence, and
revision model.

Compiler architecture can also be reviewed in the searchable [Crates](design/crates/README.md)
chapters, rendered from `md/design/crates/`. Run
`cargo meta check` to check declared method dependencies, their
Rust correspondences, named rules' implementation and test coverage, and saved
reviews of implementation completeness and test adequacy in `validations/<anchor-id>/`. The same
library check runs during workspace tests and CI. The generated
[Anchors](design/anchor/README.md) chapter links each claim to its source, code,
tests, and review status. Add `--json` for a structured report;
see [Reviewing Implementations and Tests](design/meta/reviews.md) for preparing and
recording reviews with the repository skill, and
[Architectural Pseudocode](design/meta/language.md)
for the language and its current coverage. Generate an interactive explorer with
`cargo meta browse`, then open `target/sage-browser.html`.
