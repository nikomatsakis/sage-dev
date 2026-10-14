# Crates

The compiler is organized around symbols: a definition has a stable identity,
and its signature, members, or body are computed when requested. The crate
subchapters expose that architecture as searchable `.sage` declarations,
operations, and dependency envelopes. Open **Rust source** on an entity to
inspect its corresponding implementation.

## Following a compilation

Start with [module parsing and expansion](sage_ir/modules.sage), then follow
[symbols](sage_ir/symbols.sage) to their signature and body operations.
[Checking](sage_ir/checking.sage) introduces the computation-local contexts
that [resolve names](sage_ir/resolution.sage), [infer types](sage_ir/inference.sage),
and [solve trait goals](sage_ir/solver.sage). These are demand boundaries:
requesting one body need not compute every body in a crate.

## Semantic values and storage

[Types and checked results](sage_ir/types.sage) describe the semantic values
passed between operations. The [Stash storage model](sage_stash/storage.sage)
explains how those values are owned. [External metadata](sage_ir/metadata.sage)
supplies facts about dependency definitions.

## Understanding and checking results

The [Semantic Inspector](sage_inspector/crate.sage) exposes compiler products
for exploration. The [oracle harness](sage_oracle_harness/crate.sage) compares
Sage with independently emitted rustc results. Their command-line usage is
covered in the [User's Guide](../../guide/README.md).

The [inline compilation tests](sage_test_harness/compile-tests.md)
keep source fixtures, type observations, diagnostics, and query expectations
inside ordinary annotated Cargo tests. Its
[RFD](../../rfds/inline-compile-tests/README.md) explains the design.

The [specification tooling](sage_meta/crate.sage) checks the correspondence
between this model, named specification rules, and Rust code and tests.
The [Anchors](../anchor/README.md) chapter gathers those rules and their
implementation and test status. [Worked examples](../examples.md) provide
concrete programs to follow through the compiler.
