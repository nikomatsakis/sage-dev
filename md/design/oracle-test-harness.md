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

## Thin adapters and exact comparison

The oracle is intentionally asymmetric in authority but symmetric in output:
rustc supplies the reference semantics, while both emitters must independently
produce the same shared representation. The comparison layer is deliberately
dumb.

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

Exact textual equality is paired with coverage accounting. Each side reports
the items and bodies in scope, including associated and macro-generated items.
A successful comparison rejects unsupported expression placeholders and
debug-formatted fallback types. Two emitters omitting the same body or
collapsing the same expression to `?unsupported` is not conformance.

The current `rust-ref` model and emitters cover a smaller source-shaped subset;
expanding them to this boundary is planned by the [Typed IR Elaboration
RFD](../rfds/typed-ir-elaboration/README.md).

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

## Common divergence patterns

| Pattern | Meaning | Where to fix |
|---------|---------|--------------|
| sage has `"?InferVar(...)"` where oracle has a concrete type | sage's type inference didn't resolve | `sage-ir/src/check/body.rs` or the unification engine |
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
