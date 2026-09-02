# Oracle Test Harness

The oracle test harness compares sage's output against rustc's output for the same Rust source file. If they differ, the test fails. This is the primary mechanism for validating sage's correctness.

## Quick reference

```bash
# Run all oracle comparison tests
cargo test -p sage-oracle-harness

# Run a specific fixture
cargo test -p sage-oracle-harness -- cross-module

# Inspect output for a single file
cargo run -p sage-oracle -- test-fixtures/oracle/basics/hello.rs
cargo run -p sage-emit -- test-fixtures/oracle/basics/hello.rs

# Multi-file crate (oracle resolves modules from the filesystem)
cargo run -p sage-oracle -- test-fixtures/oracle/cross-module/src/lib.rs
cargo run -p sage-emit -- test-fixtures/oracle/cross-module/src/lib.rs test-fixtures/oracle/cross-module/src/types.rs
```

## How it works

```
test-fixtures/oracle/basics/hello.rs
         │                        │
         ▼                        ▼
   ┌───────────┐           ┌───────────┐
   │sage-oracle│           │ sage-emit │
   │(rustc drv)│           │(sage pipe)│
   └─────┬─────┘           └─────┬─────┘
         │                        │
         ▼                        ▼
  Crate<NormalizedDef>    Crate<NormalizedDef>
         │                        │
         ▼                        ▼
 deterministic JSON       deterministic JSON
         │                        │
         └────────┬───────────────┘
                  ▼
       exact textual identity
```

1. **Discover fixtures** — the harness walks `test-fixtures/oracle/` recursively:
   - A `.rs` file → single-file test
   - A directory with `src/lib.rs` or `src/main.rs` → multi-file crate test

2. **Run the oracle** (`sage-oracle`) — invokes `rustc_driver::run_compiler`, hooks `after_analysis`, walks the fully type-checked HIR, and emits a `Crate<NormalizedDef>`.

3. **Run sage** (`sage-emit`) — creates a salsa database, registers source files, triggers parsing + macro expansion + type checking, and walks the typed IR to emit the same `Crate<NormalizedDef>`.

4. **Compare** — serialize both with the same deterministic renderer and
   require byte-for-byte textual identity. A secondary diff may explain a
   mismatch, but it does not decide whether the test passes.

<a id="oracle-a1"></a>
> **ORACLE-A1 — Conformance is exact identity of independent emissions.** Sage
> and rustc each produce and validate a complete value in the shared semantic
> schema, serialize it deterministically, and pass only when the serialized
> bytes are identical. A structural or textual diff may diagnose inequality
> but cannot decide equality.
>
> **Required verification:** Harness tests perturb each side independently,
> reject every byte difference, demonstrate that diagnostic diffing runs only
> after inequality, and compare both emissions against a reviewed exact
> snapshot for pinned slices.

## Thin adapters and exact comparison

The oracle is intentionally asymmetric in authority but symmetric in output:
rustc supplies the reference semantics, while both emitters must independently
produce the same shared representation. The comparison layer is deliberately
dumb.

The anchors in this chapter are the auditable consequences of
[D4](./decisions.md#d4-oracle-test-harness).

An emitter may perform only the adaptation needed to cross from its native IR
into the shared schema. Examples include:

- mapping native definition handles to the schema's stable local IDs or
  external definition paths;
- translating rustc receiver adjustments into the explicit operations which
  the shared typed tree represents; and
- mapping lifetimes to `Dummy` because the shared Sage architecture explicitly
  defers lifetime semantics.

These adaptations are specified without consulting the other emitter's
output. When the shared model cannot represent a required distinction, extend
the model and both emitters; do not erase the distinction from both sides.
An external path segment carries its native definition kind as well as its
name. Type/value namespace is not a substitute: for example,
`core::clone::Clone::clone` is `Mod("clone")`, `Trait("Clone")`, then
`Fn("clone")`. The rustc oracle and the rustc metadata bridge independently
walk the definition-parent chain to obtain those facts.

<a id="oracle-a2"></a>
> **ORACLE-A2 — Each adapter is a fixed projection into the shared model.** An
> emitter may translate its native identities, adjustments, and deliberately
> deferred lifetime representation, but the projection cannot inspect the
> other output or erase a distinction merely because Sage does not yet match
> it. Missing shared vocabulary is repaired by extending the schema and both
> independent projections.
>
> **Required verification:** Adapter-focused tests cover native definition
> kinds, substitutions, receiver adjustments, lifetimes, and unsupported
> constructs; adversarial fixtures prove that one emitter's omission or wrong
> classification remains an exact mismatch rather than being normalized away.

The harness must not:

- perform paired normalization or rewrite one output in response to the other;
- replace inference variables, literal values, definitions, or unsupported
  nodes with common placeholders;
- strip bodies or fields merely to make a conformance test pass;
- sort or renumber output after emission to hide nondeterministic producers;
- normalize aliases solely for comparison; or
- use a semantic-equivalence algorithm in place of textual identity.

Deterministic ordering, stable definition identities, and complete values are
emitter responsibilities. Each output is also validated independently for
coverage and forbidden placeholders. Only then are the serialized bytes
compared. A JSON-path or tree diff is useful diagnostics after inequality has
already been established; it cannot convert unequal text into a passing test.

Both emitters pre-register emitted local definitions in semantic item order
before translating any signature or body. Forward references therefore use
the same local identity as later definitions; no comparison-side renumbering
repairs an order-dependent emitter.

<a id="oracle-a3"></a>
> **ORACLE-A3 — Determinism is an emitter responsibility.** Each side chooses
> stable definition identities and semantic ordering before emission,
> including forward references. The comparator never sorts, renumbers, or
> otherwise repairs nondeterministic or order-dependent output.
>
> **Required verification:** Repeated fixtures and fixtures with forward
> references or deliberately reordered items each produce their specified
> deterministic emission independently on each side; namespace-kind tests fail
> when either emitter changes identity assignment or semantic order.

The comparison path contains no paired normalization for unresolved inference
variables, literal values, or other known limitations. Such placeholders are
ordinary mismatches and cannot be normalized away.

## Semantic and coverage boundary

The destination comparison value is the [elaborated typed
IR](./typed-ir.md). The rustc side minimally projects its selected definitions,
substitutions, and adjustment lists into the shared calls, borrows,
dereferences, and coercions. Sage's internal method-candidate or adjustment
representation is not part of the oracle contract. This per-emitter projection
into a shared schema is not a license for a later pairwise normalization pass.

The destination contract pairs exact textual equality with coverage
accounting. Each side reports the items and bodies in scope, including
associated and macro-generated items, and a successful comparison rejects
unsupported expression placeholders and debug-formatted fallback types. Two
emitters omitting the same body or collapsing the same expression to
`?unsupported` is not conformance. Until the general coverage validator lands,
each vertical slice must add structural checks proving that its in-scope body
is present, successfully checked, and contains no source-shaped or unsupported
nodes; textual equality alone is never accepted as evidence of coverage.

<a id="oracle-a4"></a>
> **ORACLE-A4 — Equality is accepted only after independent coverage.** Each
> emitter accounts for every in-scope item and body and rejects forbidden
> placeholders before comparison. Paired omission, paired unsupported nodes,
> or paired debug fallback values therefore cannot satisfy conformance even if
> the remaining serialized bytes agree.
>
> **Required verification:** Coverage tests independently remove or replace
> each representative item, associated body, generated/source-written body,
> expression family, and type family on either side and require rejection
> before exact comparison.

Source-written impl methods are part of that item/body scope. Each emitter
pre-registers them and projects them as function items in the shared flat
schema. Methods emitted by source bang/procedural macros remain in scope.
Compiler-generated derive methods are excluded independently by native
provenance (`ParseSource::Derive` in Sage and derive expansion identity in
rustc), because the current milestone validates the source-associated body and
the generated impl evidence, not derive implementation bodies. This exclusion
is a stated coverage boundary, not a comparison-time rewrite. A separate macro
impl fixture prevents the rustc adapter from broadening that exclusion to all
expanded methods.

The `mini_redis/db_drop_guard.rs` fixture exercises the rule directly: rustc
projects its receiver adjustments into explicit dereference, field, and shared
borrow nodes; Sage emits its already elaborated body; and the harness decides
the result using the serialized bytes alone. Before comparison, fixture-specific
checks independently require that each side contains the source-written body
and its resolved call/borrow/field/dereference shape; these checks establish
coverage and do not rewrite either output.

The fixture also has a checked-in exact JSON snapshot. The harness requires
the independently serialized rustc output and the independently serialized
Sage output to each equal those bytes before comparing them to one another.
The snapshot is a reviewable record of the shared IR, not a normalization
input: no output is rewritten to match it. Its structural coverage assertion
also requires the external call path to be exactly `Mod → Trait → Fn`, so two
adapters cannot pass merely by repeating the same lossy kind inference.

The shared schema expands with the accepted [Typed IR Elaboration
RFD](../rfds/typed-ir-elaboration/README.md); the implemented coverage frontier
is recorded below rather than changing this destination boundary.

## Output directory

Every test run writes JSON files to a fresh temp directory:

```
/tmp/sage-oracle-output/run-N/
  basics/
    hello.rs.oracle.json      ← what rustc produced
    hello.rs.sage.json        ← what sage produced
  cross-module.oracle.json
  cross-module.sage.json
```

The path is printed at the start and end of the run. You can `diff` these files, pipe them to `jq`, or open them in an editor.

## Crate layout

| Crate | Role |
|-------|------|
| `crates/rust-ref` | Shared data model: `Crate<Def>`, `Module`, `Item`, `Expr`, `Type`, etc. Serde-serializable, generic over `Def`. |
| `crates/sage-oracle` | rustc custom driver. Compiles a `.rs` file, walks HIR, emits `Crate<NormalizedDef>`. Also provides a CLI binary. |
| `crates/sage-emit` | Sage-side emitter. Walks sage's symbol tree + typed bodies, emits `Crate<NormalizedDef>`. Also provides a CLI binary. |
| `crates/sage-oracle-harness` | Test harness. Discovers fixtures, runs both sides, compares. Uses `libtest-mimic` with `harness = false`. |
| `crates/sage-test-harness` | Test infrastructure for sage: `with_test_crate`, `with_test_crate_files`. Sets up salsa database + root module. |

## Adding a new test fixture

Just drop a file:

```bash
# Single-file test
echo 'fn foo(x: i32) -> i32 { x + 1 }' > test-fixtures/oracle/basics/arithmetic.rs

# Multi-file crate test
mkdir -p test-fixtures/oracle/my-crate/src
echo 'mod helper; fn main() { helper::greet(); }' > test-fixtures/oracle/my-crate/src/lib.rs
echo 'pub fn greet() {}' > test-fixtures/oracle/my-crate/src/helper.rs
```

Next `cargo test -p sage-oracle-harness` will auto-discover and run them. No code changes needed.

## Fixing a failing test

When a test fails, the output looks like:

```
test basics/hello.rs ... FAILED

---- basics/hello.rs ----
fixture 'basics/hello.rs' diverges between oracle and sage:
json atoms at path ".root.items[3].fn.body.block.tail.struct_lit.ty.def.target.local" are not equal:
    lhs: 3
    rhs: (missing)

json atom at path ".root.items[3].fn.body.block.tail.struct_lit.ty.primitive" is missing from lhs

Output files:
  oracle: /tmp/sage-oracle-output/run-0/basics/hello.rs.oracle.json
  sage:   /tmp/sage-oracle-output/run-0/basics/hello.rs.sage.json

Reproduce:
  cargo run -p sage-oracle -- test-fixtures/oracle/basics/hello.rs
  cargo run -p sage-emit -- test-fixtures/oracle/basics/hello.rs
```

The workflow to fix it:

1. **Understand the divergence** — look at the JSON path. In this example, sage emits a `primitive` type string where the oracle emits a `def` reference. This means sage's type checker didn't resolve the type to the struct definition.

2. **Inspect the full output** — `diff` or `jq` the output files:
   ```bash
   diff /tmp/sage-oracle-output/run-0/basics/hello.rs.oracle.json \
        /tmp/sage-oracle-output/run-0/basics/hello.rs.sage.json
   ```

3. **Reproduce independently** — run each binary to see the full JSON:
   ```bash
   cargo run -p sage-oracle -- test-fixtures/oracle/basics/hello.rs | jq '.root.items[3]'
   cargo run -p sage-emit -- test-fixtures/oracle/basics/hello.rs | jq '.root.items[3]'
   ```

4. **Fix sage** — the bug is in `sage-ir` (type checking, resolution, body lowering) or in `sage-emit` (the translation from sage's IR to `rust-ref` types).

5. **Verify** — re-run `cargo test -p sage-oracle-harness -- hello` to confirm the fix.

## The `rust-ref` data model

Both sides emit the same types. Key structures:

- **`Crate<Def>`** — root, contains a `Module<Def>`
- **`Module<Def>`** — `def`, `name`, `items: Vec<Item<Def>>`
- **`Item<Def>`** — `Fn(FnItem)` | `Struct(StructAst)` | `Mod(Module)`
- **`FnItem<Def>`** — `name`, `params`, `return_ty`, `body: Option<Expr<Def>>`
- **`Expr<Def>`** — `Local` | `Literal` | `BinaryOp` | `Call` | `StructLit` | `Field` | `Block` | `Deref` | `Ref`
- **`Type<Def>`** — `Primitive(String)` | `Def { target, type_args }` | `Ref` | `Unit` | `Tuple`

`Def` is instantiated as `NormalizedDef`. Here, “normalized” means only that a
native compiler handle has been mapped to the stable identity required by the
shared schema; it does not mean that the comparison layer normalizes typed IR:
- `NormalizedDef::Local(u32)` — a definition within this crate, identified by sequential numbering (source order)
- `NormalizedDef::External(DefPath)` — a definition from another crate (e.g., std), identified by crate name + path segments

Every external path segment records its own `DefKind`; it is not labeled with
the leaf definition's kind. A type such as
`Type::Def { target: NormalizedDef::Local(2), type_args: [] }` refers to the
item whose normalized definition is `Local(2)`. The empty type-argument list is
omitted from JSON.

## Common divergence patterns

| Pattern | Meaning | Where to fix |
|---------|---------|--------------|
| sage has `"?InferVar(...)"` where oracle has a concrete type | sage's type inference didn't resolve | `sage-ir/src/check/infer_ctx.rs` or `check/infer/` |
| sage literal has `value: "0"` where oracle has `value: "42"` | sage doesn't store literal values | `sage-ir/src/cst/expr.rs` → need to thread literal text through |
| sage `Call` target is `External { krate: "?" }` | sage couldn't resolve the callee | `sage-ir/src/check/resolve/` |
| Item count differs | sage dropped or duplicated items during expansion | `sage-ir/src/local_syms/mods.rs` |
| `local.index` differs | Parameter/let-binding numbering mismatch | Check `LocalId` assignment in `sage-ir` body lowering |

## Design decisions

- **Exact serialized equality** — deterministic JSON output must be
  byte-for-byte identical. If Sage cannot resolve a type or preserve a literal,
  the test fails.
- **No paired normalization** — neither output is rewritten based on a known
  limitation or on the contents of the other output.
- **Deterministic ordering** — both sides emit items in source order. Defs are numbered sequentially. No hash-map iteration order or pointer addresses.
- **Rich diff is diagnostic only** — after exact comparison fails, a JSON diff
  may report a precise structural path. It never determines equality.

## Current status

### Current frontier and evidence

The harness independently emits rustc and Sage reference IR, validates
fixture-specific coverage for the two pinned mini-redis slices, requires their
checked-in exact snapshots, and then compares deterministic serialized text.
The [oracle-checked method review
packet](./examples/oracle-checked-method.md#review-packet) links the source,
structural Sage assertion, snapshot, exact comparison, query trace, and edit
experiment. Together these artifacts exercise the exact snapshot and
comparison portion of [ORACLE-A1](#oracle-a1), the adjustment and external-kind portions of
[ORACLE-A2](#oracle-a2), exercise [ORACLE-A3](#oracle-a3)'s local/external
identity ordering, and provide the slice-specific coverage evidence currently
available for [ORACLE-A4](#oracle-a4).
The same full fixture suite is the regression gate that keeps the Semantic
Inspector's richer canonical external-navigation paths out of this shared
conformance projection.

Run all fixtures with `cargo test -p sage-oracle-harness`, or the pinned body
with:

```bash
cargo test -p sage-oracle-harness --test oracle_compare -- mini_redis/db_drop_guard.rs
```

### Current limitations

- General coverage accounting is not yet schema-wide; each new vertical slice
  must add explicit structural coverage checks so paired omission cannot pass.
  ORACLE-A4 is therefore established only for the pinned fixtures.
- The shared `rust-ref` schema covers only the currently represented Typed IR
  families, so the full adapter matrix required by ORACLE-A2 grows with Typed
  IR coverage.
- ORACLE-A1's independent perturbation and diagnostic-ordering matrix, and
  ORACLE-A2's adversarial one-sided omission matrix, are not yet presented as
  focused evidence.
- Oracle JSON is exact and reviewable but not the intended ergonomic
  human-facing semantic inspection format.
- The adversarial repetition, forward-reference, and reordered-input matrix
  required by ORACLE-A3 is not yet presented as focused evidence.

### Related roadmap slices

- [Shutdown::recv](../implementation/roadmap.md#next-application-slice-shutdownrecv)
  will add the next exact vertical fixture.
- [Semantic inspector and persistent edit
  testing](../implementation/roadmap.md#implemented-slice-semantic-inspector-and-persistent-edit-testing)
  adds readable Sage-only inspection without relaxing oracle equality.
