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
         └────────┬───────────────┘
                  ▼
         JSON structural diff
         (assert-json-diff)
```

1. **Discover fixtures** — the harness walks `test-fixtures/oracle/` recursively:
   - A `.rs` file → single-file test
   - A directory with `src/lib.rs` or `src/main.rs` → multi-file crate test

2. **Run the oracle** (`sage-oracle`) — invokes `rustc_driver::run_compiler`, hooks `after_analysis`, walks the fully type-checked HIR, and emits a `Crate<NormalizedDef>`.

3. **Run sage** (`sage-emit`) — creates a salsa database, registers source files, triggers parsing + macro expansion + type checking, and walks the typed IR to emit the same `Crate<NormalizedDef>`.

4. **Compare** — serialize both to `serde_json::Value` and diff structurally. On mismatch, report the exact JSON path that diverges.

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

`Def` is instantiated as `NormalizedDef`:
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

- **No normalization** — the harness compares raw output. If sage can't resolve a type, the test fails. This ensures every InferVar or missing value is a tracked issue.
- **Deterministic ordering** — both sides emit items in source order. Defs are numbered sequentially. No hash-map iteration order or pointer addresses.
- **JSON diff, not string diff** — `assert-json-diff` reports the exact structural path (`.root.items[2].fn.body.block.tail`), not line numbers in a pretty-printed string.
