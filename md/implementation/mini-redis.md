# Mini-redis Conformance Roadmap

The pinned `test-fixtures/mini-redis` crate is Sage's application-scale
conformance target. Work proceeds through reviewable vertical slices rather
than making the whole crate one opaque implementation task.

This page records milestone scope and ordering. Architectural meaning lives in
the [Typed IR](../design/typed-ir.md), [Checking](../design/checking.md), and
[Trait Solver](../design/trait-solver.md) pages. Per-RFD implementation steps
remain in each RFD's `implementation.md`.

## Initial target

The first package-wide target is the `mini_redis` library target at the pinned
submodule revision, with default features and its declared Rust edition. Bins,
examples, integration tests, and the optional `otel` feature are later rungs.

The Cargo target identity, enabled features, `cfg` values, dependency world,
and edition are explicit analysis inputs. Sage and the rustc oracle must
analyze the same target configuration.

Borrow checking and meaningful lifetime reasoning are not part of this
conformance target. Every checked lifetime is `Lifetime::Dummy`, as specified
by the typed-IR architecture.

## Slice 1: `DbDropGuard::db`

The first implementation goal checks this existing body in
`src/db.rs`:

```rust,ignore
pub(crate) fn db(&self) -> Db {
    self.db.clone()
}
```

This small body exercises the architectural seam without requiring async
lowering or projection normalization:

- associated-function symbol, signature, and body queries;
- receiver and resolved local field access;
- derive-produced `Db: Clone` evidence;
- discovery of the external `Clone` trait and its `clone` item;
- fixed-trait proof and method selection;
- explicit dereference and shared borrow with `Lifetime::Dummy`; and
- elaboration of method syntax into a resolved trait call.

Completion requires:

- the final body contains no `MethodCall`, unresolved field name, inference
  variable, adjustment list, or unsupported placeholder;
- the rustc oracle produces the same normalized semantic tree;
- call checking reads the selected signatures and impl data but not the body
  of `Clone::clone` or any unrelated mini-redis body;
- a semantic query trace names the method/trait lookups and external metadata
  requests performed;
- a repeated unchanged query reuses its result; and
- existing unit and oracle tests remain green.

The query trace is compared as a normalized set or multiset unless order is
the behavior under test. Scheduler completion order is not part of this
milestone.

## Slice 2: `Parse::next`

The second slice adds:

- an upstream `Iterator` impl for `vec::IntoIter<Frame>`;
- normalization of `Iterator::Item`;
- an inherent generic `Option::ok_or` call; and
- chained elaborated calls with no callee-body dependency.

This is the first focused acceptance test for global, trait-keyed impl
discovery and associated-alias normalization. The final tree may retain an
`AliasTy::Associated` where its identity matters; the test requires the solver
to establish the `Iterator::Item` relation needed by the call, not an eager
alias-erasure pass over the whole body.

## Slice 3: `Shutdown::recv`

The third slice adds:

- an async associated body;
- an external future-producing method;
- `IntoFuture`/`Future::Output` elaboration for `.await`;
- assignment and early return; and
- the high-level typed `Await` node without state-machine lowering.

## Library coverage

After the three slices, coverage expands by feature family and module. A full
library pass requires:

- every source and macro-generated item in the selected target is accounted
  for;
- every function and associated-function body is forced exactly through its
  body query;
- successful output contains no unsupported typed-IR node or debug-formatted
  type;
- Sage emits no diagnostics for the rustc-clean target;
- normalized typed IR agrees with the oracle; and
- focused incremental tests demonstrate narrow dependencies for signatures,
  external metadata, method lookup, trait candidates, macro expansion, and
  bodies.

## Later targets

Once the default-feature library passes, add in order:

1. the CLI and server binary targets;
2. examples;
3. integration-test targets; and
4. the optional `otel` feature.

Each target is a distinct Cargo compilation with its own query identity. A
green library result does not imply that bins, tests, or feature combinations
were analyzed.
