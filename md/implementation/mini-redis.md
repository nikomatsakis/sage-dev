# Mini-redis Conformance Roadmap

The pinned `test-fixtures/mini-redis` crate is Sage's application-scale
conformance target. Work proceeds through reviewable vertical slices rather
than making the whole crate one opaque implementation task.

This page records milestone scope and ordering. Architectural meaning lives in
the [Typed IR](../design/typed-ir.md), [Checking](../design/checking.md), and
[Trait Solver](../design/trait-solver.md) pages. Per-RFD implementation steps
remain in each RFD's `implementation.md`.

This is the application-specific companion to the general [Build-Out
Roadmap](./roadmap.md). Each body is a cross-cutting acceptance slice; after a
slice lands, the affected architecture chapters record their local Current
Status and evidence.

## Initial target

The first package-wide target is the `mini_redis` library target at the pinned
submodule revision, with default features and its declared Rust edition. Bins,
examples, integration tests, and the optional `otel` feature are later rungs.

The Cargo target identity, selected root, enabled features, dependency world,
and edition are explicit analysis inputs. Sage and the rustc oracle must
analyze the same operational inputs. General source-side `cfg` evaluation is a
separate source-pipeline feature, not an acceptance requirement for an ungated
vertical slice.

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
- the rustc and Sage emitters produce byte-identical deterministic text for
  the shared semantic tree, with no paired comparison normalization;
- call checking reads the selected signatures and impl data but not the body
  of `Clone::clone` or any unrelated mini-redis body;
- a semantic query trace names the method/trait lookups and external metadata
  requests performed;
- a repeated unchanged query reuses its result; and
- existing unit and oracle tests remain green.

The query trace is compared as a normalized set or multiset unless order is
the behavior under test. Scheduler completion order is not part of this
milestone.

Current implementation checkpoint: the isolated `DbDropGuard::db` fixture
meets this slice's semantic and dependency criteria. `#[derive(Clone)]`
preserves `Db` and appends an ordinary parsed `impl Clone for Db` with
generated-source provenance. The external `Clone` and `Sized` contracts cross
the typed metadata boundary; lookup discovers `Clone::clone`, proves the fixed
`Db: Clone` goal, and consumes the dereference and shared borrow into completed
IR. The source-associated body is emitted on both sides and compares by exact
serialized identity. Each side must also match a checked-in exact JSON
snapshot, whose external `Clone::clone` identity records the native
`Mod("clone") → Trait("Clone") → Fn("clone")` path kinds. A focused trace
also proves that checking reads the represented trait/item/function metadata
but no callee body, and that the unchanged second body query is reused.

This does not complete general method resolution. Lookup now uses the crate's
actual edition prelude, resolves explicit import edges including renamed
external reexports, and enumerates external glob edges. Local globs remain an
incomplete source until their reexports can be enumerated recursively. It
accepts only the represented Self-only external trait-method shape, and treats
unresolved imports, macros, attributes, unhandled inherent providers,
explicit-bound providers, and unavailable metadata as uncertainty. External
inherent selection and relevant-impl enumeration are built for the rigid
receiver slices; local and builtin inherent methods, general receiver search,
and local unrelated-edit invalidation isolation remain later work.

## Slice 2: `Parse::next`

The pinned semantic slice is implemented by the completed [Associated Type
Normalization RFD](../rfds/associated-type-normalization/README.md).

The second slice adds:

- an upstream `Iterator` impl for `vec::IntoIter<Frame>`;
- normalization of `Iterator::Item`;
- an inherent generic `Option::ok_or` call; and
- chained elaborated calls with no callee-body dependency.

The pinned test selects the real non-proc-macro `mini_redis` library target,
uses its Rust 2018 edition, default-feature dependency world, and `src/lib.rs`
root, then forces only the source `Parse::next` body. The body and containing
module path are ungated. Its completed tree contains static `Iterator::next`
dispatch, direct `Option::ok_or` dispatch, separate owner/method substitutions,
and an explicit mutable `Dummy` borrow. The cold semantic trace reads two
relevant external impl headers and the selected `Iterator::Item` value; it
lowers no unrelated local impl header and checks no other mini-redis or external
body. Repeating the query is warm and rereads none of that metadata.

This is the first focused acceptance test for global, trait-keyed impl
discovery and associated-alias normalization. Other aliases may retain their
identity; this body normalizes `Iterator::Item` because its value is demanded
by the method chain and declared return type.

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
- deterministic shared typed-IR output is byte-identical to the oracle; and
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
