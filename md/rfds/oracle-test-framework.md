# RFD: Oracle Test Framework

**Status:** Proposed

## Problem

Sage needs a testing strategy that validates correctness against rustc — the canonical Rust implementation. Unit tests against internal APIs are brittle (they break on every refactor) and don't answer the question that matters: "does sage produce the same answer as rustc?"

We want a framework that:

1. Takes arbitrary `.rs` files as input (no fixture DSL, no test harness coupling).
2. Produces a normalized, diffable output format from both rustc and sage.
3. Can eventually be run against the `rust-lang/rust` test suite (`tests/ui/`) to measure coverage at scale.

## Design

### Shared data model: `rust-ref` crate

A new crate (`crates/rust-ref`) defines the canonical data structures representing lifetime-erased typed Rust programs. Both the rustc oracle driver and sage produce values of these types. Comparison is structural equality — not string diffing.

The data model is serializable (serde) so outputs can be written to JSON files for debugging, but the primary comparison path is in-process.

Key properties of the data model:

- **Fully resolved paths** — every name reference carries its canonical path (e.g., `std::option::Option`, not `Option`). Local variables carry their binding index.
- **Lifetime-erased types** — references are `Ref { mutable: bool, ty: Box<Type> }` with no lifetime. This avoids lifetime inference differences while still exercising the type checker.
- **Structured, not textual** — the output is a tree of typed enums, not formatted strings. This lets the test harness do precise structural diff ("sage resolved this path differently") rather than line-level diff.

Lifetime correctness is validated separately: both sides should produce errors in the same locations. We don't try to match the error messages, just the set of spans that error.

### Example

For input:
```rust
fn foo(x: &u32) -> u32 {
    *x + 1
}
```

The shared data structure (shown as JSON for illustration):
```json
{
  "items": [
    {
      "kind": "fn",
      "name": "foo",
      "params": [
        {
          "name": "x",
          "type": { "ref": { "mutable": false, "ty": { "primitive": "u32" } } }
        }
      ],
      "return_type": { "primitive": "u32" },
      "body": {
        "kind": "call",
        "target": { "path": ["core", "ops", "Add", "add"] },
        "args": [
          {
            "kind": "deref",
            "expr": { "kind": "local", "index": 0, "name": "x" },
            "type": { "primitive": "u32" }
          },
          {
            "kind": "literal",
            "value": "1",
            "type": { "primitive": "u32" }
          }
        ],
        "type": { "primitive": "u32" }
      }
    }
  ]
}
```

The exact schema will evolve — the key property is that both drivers produce structurally identical values for correct programs.

### Architecture

```
                        ┌──────────────────┐
                        │   rust-ref crate  │  (data model + serde)
                        └────────┬─────────┘
                    ┌────────────┼────────────┐
                    ▼                         ▼
┌──────────────────────────┐   ┌──────────────────────────┐
│  sage-oracle             │   │  sage                    │
│  (rustc custom driver)   │   │  (sage pipeline)         │
│  .rs → rust-ref value    │   │  .rs → rust-ref value    │
└──────────────────────────┘   └──────────────────────────┘
                    │                         │
                    ▼                         ▼
              ┌──────────────────────────────────┐
              │  test harness: assert_eq + diff  │
              └──────────────────────────────────┘
```

#### `rust-ref` (new crate: `crates/rust-ref`)

A plain Rust library — no compiler dependencies. Defines the shared data model:

- `Type` enum (primitives, references, paths with generic args, tuples, slices, fn pointers, etc.)
- `Item` enum (fn, struct, enum, trait, impl, type alias, const, static)
- `Expr` enum (local, literal, call, method call, field access, binary op, if, match, block, etc.)
- `Pattern` enum (binding, struct, tuple, literal, wildcard, etc.)

All types derive `Serialize`, `Deserialize`, `PartialEq`, `Debug`. The crate has no dependency on rustc or sage internals — it's the neutral meeting point.

#### `sage-oracle` (new crate: `crates/sage-oracle`)

A rustc custom driver (using `rustc_driver` + `rustc_interface`) that:

1. Compiles the input file through full type checking.
2. Walks the typechecked HIR (`rustc_hir` + `TypeckResults`).
3. Builds `rust-ref` values, erasing lifetimes, resolving all paths to their `DefPath`.
4. Serializes to JSON on stdout (or returns values in-process for the test harness).

This is the ground truth. It runs the real rustc pipeline — no shortcuts.

#### Sage output

Sage's equivalent output path. Given the same `.rs` file, sage parses, resolves, type-checks, and produces `rust-ref` values. Differences are bugs in sage.

#### Test harness

A test crate (or integration tests in the workspace root) that:

1. Discovers `.rs` test files from a directory (e.g., `test-fixtures/oracle/`).
2. Runs the oracle on each file → `rust-ref` value.
3. Runs sage on each file → `rust-ref` value.
4. Compares using `assert-json-diff` (`assert_json_eq!`), which serializes both sides via serde and reports the exact JSON path where they diverge (e.g., "json atoms at path `.items[2].fn.body.call.target` are not equal").

For error cases, both sides emit a sorted list of error spans. Same structural comparison.

### Test file discovery

The harness supports two input shapes:

1. **Single `.rs` file** — a standalone program (e.g., `test-fixtures/oracle/basics/hello.rs`). The oracle compiles it directly.

2. **Directory with `src/`** — a multi-file crate (e.g., `test-fixtures/oracle/cross-module/`). The harness finds `src/lib.rs` or `src/main.rs` as the entry point and includes all `.rs` files under `src/`. Either works — lib crates and binary crates are both supported.

The harness recursively walks `test-fixtures/oracle/`. For each entry:
- If it's a `.rs` file, treat it as a single-file test.
- If it's a directory containing `src/lib.rs` or `src/main.rs`, treat it as a multi-file crate test.
- Otherwise, skip it.

### Initial test corpus

Three tests to get passing first (`test-fixtures/oracle/`):

#### `basics/hello.rs` — simple functions and structs

Exercises: fn signatures, parameter types, return types, struct definitions, struct literals, field access. No imports, no macros, no generics. The minimum viable test.

#### `basics/macro_rules.rs` — zero-arg macro_rules expansion

Exercises: `macro_rules!` definition, invocation with no arguments, name resolution of the macro-generated function. Validates that sage's macro expansion produces the same visible items as rustc's.

#### `cross-module/` — imports across modules

A two-file crate: `src/lib.rs` declares `mod types;` and uses `types::Wrapper`. Exercises: module resolution, `use` imports, cross-module path resolution, struct field access on a type defined elsewhere.

### Phased rollout

**Phase 1: Oracle driver only.** Build `sage-oracle` and get it printing clean `rust-ref` JSON for the three initial tests. No sage comparison yet — just establish the format on real code.

**Phase 2: Sage comparison.** Wire up sage to produce `rust-ref` values for the same inputs. Get the outputs matching for the initial corpus.

**Phase 3: Expand corpus.** Add generics, trait impls, closures, enums, pattern matching. Each new language feature gets a test file.

**Phase 4: Scale to rust test suite.** Point the harness at `tests/ui/` from rustc. Track pass-rate as a metric. Accept that many files will fail (unsupported features) — the goal is a ratchet, not 100% from day one.

### Expected outputs for initial tests

Working through the three test files to derive the minimum `rust-ref` data model.

#### `basics/hello.rs`

```rust
fn identity(x: u32) -> u32 { x }
fn add(a: u32, b: u32) -> u32 { a + b }
struct Point { x: u32, y: u32 }
fn origin() -> Point { Point { x: 0, y: 0 } }
fn get_x(p: Point) -> u32 { p.x }
```

Expected `rust-ref` output (JSON):
```json
{
  "root": {
    "name": "",
    "items": [
      {
        "fn": {
          "name": "identity",
          "params": [{ "name": "x", "ty": "u32" }],
          "return_ty": "u32",
          "body": { "local": { "name": "x", "index": 0 } }
        }
      },
      {
        "fn": {
          "name": "add",
          "params": [
            { "name": "a", "ty": "u32" },
            { "name": "b", "ty": "u32" }
          ],
          "return_ty": "u32",
          "body": {
            "binary_op": {
              "op": "add",
              "lhs": { "local": { "name": "a", "index": 0 } },
              "rhs": { "local": { "name": "b", "index": 1 } },
              "ty": "u32"
            }
          }
        }
      },
      {
        "struct": {
          "name": "Point",
          "fields": [
            { "name": "x", "ty": "u32" },
            { "name": "y", "ty": "u32" }
          ]
        }
      },
      {
        "fn": {
          "name": "origin",
          "params": [],
          "return_ty": { "path": ["Point"] },
          "body": {
            "struct_lit": {
              "path": ["Point"],
              "fields": [
                { "name": "x", "value": { "literal": { "kind": "int", "value": "0" } } },
                { "name": "y", "value": { "literal": { "kind": "int", "value": "0" } } }
              ],
              "ty": { "path": ["Point"] }
            }
          }
        }
      },
      {
        "fn": {
          "name": "get_x",
          "params": [{ "name": "p", "ty": { "path": ["Point"] } }],
          "return_ty": "u32",
          "body": {
            "field": {
              "expr": { "local": { "name": "p", "index": 0 } },
              "field_name": "x",
              "ty": "u32"
            }
          }
        }
      }
    ]
  }
}
```

#### `basics/macro_rules.rs`

```rust
macro_rules! make_getter {
    () => { fn get_value() -> u32 { 42 } };
}
make_getter!();
fn use_getter() -> u32 { get_value() }
```

The `rust-ref` model represents the **post-expansion** program. The macro definition and invocation are not represented — only their visible output. So this looks like:

```json
{
  "root": {
    "name": "",
    "items": [
      {
        "fn": {
          "name": "get_value",
          "params": [],
          "return_ty": "u32",
          "body": { "literal": { "kind": "int", "value": "42" } }
        }
      },
      {
        "fn": {
          "name": "use_getter",
          "params": [],
          "return_ty": "u32",
          "body": {
            "call": {
              "target": { "path": ["get_value"] },
              "args": [],
              "ty": "u32"
            }
          }
        }
      }
    ]
  }
}
```

#### `cross-module/`

```rust
// src/lib.rs
mod types;
use types::Wrapper;
fn wrap(x: u32) -> Wrapper { Wrapper { value: x } }
fn unwrap(w: Wrapper) -> u32 { w.value }

// src/types.rs
pub struct Wrapper { pub value: u32 }
```

Submodules are items, just like in Rust:

```json
{
  "root": {
    "name": "",
    "items": [
      {
        "mod": {
          "name": "types",
          "items": [
            {
              "struct": {
                "name": "Wrapper",
                "fields": [
                  { "name": "value", "ty": "u32" }
                ]
              }
            }
          ]
        }
      },
      {
        "fn": {
          "name": "wrap",
          "params": [{ "name": "x", "ty": "u32" }],
          "return_ty": { "path": ["types", "Wrapper"] },
          "body": {
            "struct_lit": {
              "path": ["types", "Wrapper"],
              "fields": [
                { "name": "value", "value": { "local": { "name": "x", "index": 0 } } }
              ],
              "ty": { "path": ["types", "Wrapper"] }
            }
          }
        }
      },
      {
        "fn": {
          "name": "unwrap",
          "params": [{ "name": "w", "ty": { "path": ["types", "Wrapper"] } }],
          "return_ty": "u32",
          "body": {
            "field": {
              "expr": { "local": { "name": "w", "index": 0 } },
              "field_name": "value",
              "ty": "u32"
            }
          }
        }
      }
    ]
  }
}
```

### Derived `rust-ref` data structures

From the above examples, the minimum types needed. All data structures are
**generic over a `Def` type parameter** representing resolved definitions.

Callers construct the tree directly, embedding their native ID type (`DefId`
for rustc, `Symbol<'db>` for sage) as the `Def` parameter. `Def` represents
**item-level definitions** (fns, structs, modules, traits, impls, etc.) — not
local variables or parameters, which use `Expr::Local { index }` instead.

A `map` operation walks the tree, replacing every `Def` with a normalized
form suitable for comparison.

```rust
// ═══════════════════════════════════════════════════════════════════════
// Core data structures — generic over Def
// ═══════════════════════════════════════════════════════════════════════

pub struct Crate<Def> {
    pub root: Module<Def>,
}

pub struct Module<Def> {
    pub def: Def,
    pub name: String,
    pub items: Vec<Item<Def>>,
}

pub enum Item<Def> {
    Fn(FnItem<Def>),
    Struct(StructItem<Def>),
    Mod(Module<Def>),
    // Future: Enum, Trait, Impl, Const, Static, TypeAlias
}

pub struct FnItem<Def> {
    pub def: Def,
    pub name: String,
    pub params: Vec<Param<Def>>,
    pub return_ty: Type<Def>,
    pub body: Expr<Def>,
}

pub struct Param<Def> {
    pub name: String,
    pub ty: Type<Def>,
}

pub struct StructItem<Def> {
    pub def: Def,
    pub name: String,
    pub fields: Vec<FieldDef<Def>>,
}

pub struct FieldDef<Def> {
    pub name: String,
    pub ty: Type<Def>,
}

/// Lifetime-erased type representation.
pub enum Type<Def> {
    /// Primitive types: u8, u16, u32, u64, i8, ..., bool, char, str
    Primitive(String),
    /// Named type — references a definition.
    Def { target: Def, type_args: Vec<Type<Def>> },
    /// &T or &mut T (lifetime erased)
    Ref { mutable: bool, ty: Box<Type<Def>> },
    /// ()
    Unit,
    /// (A, B, ...)
    Tuple(Vec<Type<Def>>),
    // Future: Slice, Array, FnPtr, Dyn, Never, Infer
}

/// Body expression — typed, resolved. Fully explicit: autoref, autoderef,
/// and coercions are represented as nodes (not implicit).
pub enum Expr<Def> {
    /// Reference to a local variable/parameter.
    Local { name: String, index: u32 },
    /// Integer/float/bool/char/string literal.
    Literal { kind: LiteralKind, value: String },
    /// Built-in operator on primitives (no trait dispatch).
    BinaryOp { op: BinOp, lhs: Box<Expr<Def>>, rhs: Box<Expr<Def>>, ty: Type<Def> },
    /// Overloaded operator or regular function/method call (resolved to a def).
    Call { target: Def, args: Vec<Expr<Def>>, ty: Type<Def> },
    /// Struct literal construction.
    StructLit { target: Def, fields: Vec<FieldExpr<Def>>, ty: Type<Def> },
    /// Field access (expr.field).
    Field { expr: Box<Expr<Def>>, field_name: String, ty: Type<Def> },
    /// Block expression { stmts; tail }
    Block { stmts: Vec<Stmt<Def>>, tail: Option<Box<Expr<Def>>>, ty: Type<Def> },
    /// Explicit dereference (includes compiler-inserted autoderef).
    Deref { expr: Box<Expr<Def>>, ty: Type<Def> },
    /// Explicit reference (includes compiler-inserted autoref).
    Ref { mutable: bool, expr: Box<Expr<Def>>, ty: Type<Def> },
    // Future: If, Match, Loop, Closure, Index, Assign, Return, Cast
}

pub enum Stmt<Def> {
    Let { name: String, index: u32, ty: Type<Def>, init: Option<Expr<Def>> },
    Expr(Expr<Def>),
}

pub struct FieldExpr<Def> {
    pub name: String,
    pub value: Expr<Def>,
}

pub enum LiteralKind {
    Int,
    Float,
    Bool,
    Char,
    Str,
}

pub enum BinOp {
    Add, Sub, Mul, Div, Rem,
    Eq, Ne, Lt, Le, Gt, Ge,
    And, Or,
    BitAnd, BitOr, BitXor, Shl, Shr,
}

// ═══════════════════════════════════════════════════════════════════════
// Normalization — map Def to a comparable form
// ═══════════════════════════════════════════════════════════════════════

/// For comparison, both sides normalize to `Crate<NormalizedDef>`.
/// Local items get sequential IDs assigned in depth-first source order;
/// external items use a DefPath.
pub enum NormalizedDef {
    Local(u32),
    External(DefPath),
}

/// A stable identifier for an external definition — crate name plus
/// the path segments from the crate root to the item.
pub struct DefPath {
    pub krate: String,
    pub segments: Vec<DefPathSegment>,
}

pub struct DefPathSegment {
    /// The kind of definition at this level (module, type, fn, impl, etc.)
    pub kind: DefKind,
    /// The name (or disambiguator for anonymous items like impls).
    pub name: String,
}

pub enum DefKind {
    Mod,
    Fn,
    Struct,
    Enum,
    Trait,
    Impl,
    TypeAlias,
    Const,
    Static,
    // ...
}

/// The library provides a generic `map` over the tree that replaces
/// every `Def` via a caller-supplied closure. Normalization is just
/// one use — you could also map to debug strings, etc.
impl<Def> Crate<Def> {
    pub fn map<Def2>(self, f: impl FnMut(Def) -> Def2) -> Crate<Def2> { ... }
}
```

This is enough to represent all three initial tests. The model grows by adding variants to `Item`, `Expr`, `Type`, and `Stmt` as we expand the corpus.

### Data model design principles

- **Deterministic.** Same input → same value, always. Vectors are ordered by source position. No hash-map iteration order, no pointer addresses.
- **Minimal.** Only represent information sage is expected to produce. No MIR, no region variables, no obligation causes, no compiler-internal IDs.
- **Stable across rustc versions.** Local defs are identified by tree position (not DefIndex). External defs use `(crate_name, path)` strings. This survives rustc metadata layout changes.
- **Human-debuggable.** The JSON serialization should be readable enough to understand what the program does. Structured diff tooling can highlight exactly which node diverges.
- **Incrementally growable.** Start with fn signatures only. Add body expressions later. Add more item kinds later. The `rust-ref` types can grow without breaking existing tests — new fields are `Option` or new enum variants.

### Open questions

1. **Body detail level for phase 1?** Start with signatures only (params, return type, generic bounds) or include bodies from the start? Signatures are safer to stabilize; bodies catch more bugs.

2. **How to handle macro expansion?** Probably compare the post-expansion typed tree (both sides expand macros first). The `rust-ref` model represents the expanded program, not the surface syntax.

3. **How to handle trait resolution in bodies?** Overloaded operators (e.g., `x + y` on non-primitives) are represented as `Call` with the resolved impl method as target. Built-in operators on primitives use `BinaryOp` (no def). Method calls also use `Call` with the resolved impl path. This exercises the trait solver.

4. **Multi-file crates?** Start with single-file inputs. Multi-file support (mod declarations) can come later by pointing at a directory rather than a single file.

5. **Generics and monomorphization?** The `rust-ref` model should represent generic signatures as-written (with type parameters), not monomorphized. Call sites show the generic args used at that call.
