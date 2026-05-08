# WIP: Macro-expansion-aware name resolution

## Status

*Under discussion.* Do not edit code until we've finalized the plan.

## Tenets

* **On demand**: do the least work needed to complete the given task.
* **Opinionated**: we don't have to accept all of Rust; we can require it be used in particular ways.
* **Sound**: any program that works on sage would also work on rustc (potentially after subsetting).

## Overview

The core idea is computing a **Minimal Expanded Members map (MEM-map)** for each module. The MEM-map contains exactly the macro expansions needed to resolve all names — no more. Builtin derives that only produce `impl` blocks (no new named entities) are recorded but not expanded. In the future, annotations on macros could declare their exported names, further reducing expansion work.

`module_memmap` replaces `module_items` as the single source of truth for "what's in this module."

## MEM-map structure

The MEM-map is a tree with four kinds of entries:

* **Named members**: any Rust item with a name (struct, enum, trait, fn, mod, const, etc.). Includes *redirects* from `use` statements (e.g., `use foo::bar` creates a named member `bar` that redirects to `foo::bar`). Also includes `macro_rules!` definitions, treated as module-scoped (no textual ordering — see FAQ).

* **Macro uses**: result from macro invocations like `foo::bar!()`. Two varieties:
  * *Unexpanded*: resolved to a macro definition but not expanded (we already know it produces no new names — e.g., builtin derives).
  * *Expanded*: the macro was expanded and its output forms a **subtree** of additional members underneath this node. The tree structure is critical for detecting ambiguities (see "Time-travel detection").

* **Glob stems**: from `use foo::bar::*` or the implicit prelude. Record the source module; its MEM-map is consulted lazily during resolution.

* **Anonymous items**: `impl` blocks, `extern` blocks, and any other items that don't introduce a name into the module namespace. Not relevant for name resolution, but tracked here so the MEM-map is the single source of truth for "everything in this module." Downstream queries (method resolution, trait solving) consume these.

### Concrete data model

```rust
#[salsa::tracked(debug)]
pub struct ModuleMemmap<'db> {
    #[returns(ref)]
    pub entries: Vec<MemmapEntry<'db>>,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash, salsa::Update)]
pub enum MemmapEntry<'db> {
    Named(NamedMember<'db>),
    MacroUse(MacroUse<'db>),
    Glob(GlobStem<'db>),
    Anon(Item<'db>),
}

#[derive(Clone, Debug, PartialEq, Eq, Hash, salsa::Update)]
pub struct NamedMember<'db> {
    pub name: Name<'db>,
    pub ns: Namespace,
    pub kind: NamedMemberKind<'db>,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash, salsa::Update)]
pub enum NamedMemberKind<'db> {
    Item(Item<'db>),              // struct, enum, fn, mod, const, etc.
    Redirect { target: Path<'db> }, // from `use foo::bar` or `use foo::bar as baz`
    MacroDef(MacroDef<'db>),      // macro_rules! definition
}

#[derive(Clone, Debug, PartialEq, Eq, Hash, salsa::Update)]
pub struct MacroUse<'db> {
    pub path: Path<'db>,
    pub state: MacroUseState<'db>,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash, salsa::Update)]
pub enum MacroUseState<'db> {
    Unresolved,
    Unexpanded(MacroDef<'db>),           // resolved but known to produce no names
    Expanded(Vec<MemmapEntry<'db>>),     // subtree of expanded items (recursive)
    Error,                                // path resolved to non-macro
}

#[derive(Clone, Debug, PartialEq, Eq, Hash, salsa::Update)]
pub struct GlobStem<'db> {
    pub source_module: Module<'db>,      // lazily consulted during resolution
}

#[salsa::tracked(debug)]
pub struct MacroDef<'db> {
    pub name: Name<'db>,
    #[returns(ref)]
    pub body_tokens: String,             // raw token text of `() => { ... }` body
    pub span: SpanIndices,
}
```

The tree structure is recursive: `MacroUseState::Expanded` contains a `Vec<MemmapEntry>` which can itself contain further `MacroUse` entries (for macros produced by expansion). This naturally represents arbitrary nesting depth.

Entries are ordered deterministically: CST order for root-level items, expansion output order for subtrees. Since expansion is deterministic (same macro + same input = same output), the vec is stable across fixpoint iterations, and `Eq` comparison works directly.

### How `resolve_name` maps onto the MEM-map (post-convergence)

The current `resolve_name` has 5 steps: declared items → named imports → globs → extern prelude → std prelude. In the new design:

1. **Non-glob lookup**: walk the MEM-map tree recursively, collecting all `NamedMember` entries with matching name+namespace (from root and all expanded subtrees).
2. **Glob lookup**: for each `GlobStem`, compute `module_memmap(source_module)` and search for the name there.
3. **Priority**: non-glob beats glob. Multiple non-globs → `Ambiguous`. Zero non-globs + multiple globs → `Ambiguous`. Exactly one match → return it.
4. **Extern prelude**: if nothing found, check `tcx.extern_crate(name)`.
5. **Std prelude**: if still nothing, check `std::prelude::v1::*`.

Extern prelude beats std prelude (matching rustc). This is a simplification of the current code — steps 1+2 (declared items + named imports) collapse into a single MEM-map walk, since both explicit items and use-redirects are `NamedMember` entries.

### How `resolve_memmap_path` differs (during fixpoint)

`resolve_memmap_path` is used inside `module_memmap` for resolving macro paths. Key differences from `resolve_name`:
- Returns `Vec<Symbol>` (multiple results), not `Result<Symbol, Error>`.
- Within the **same parent** (same subtree level), non-glob shadows glob (just like post-convergence). This means a CST-level named item always shadows a CST-level glob for the same name.
- Across **different subtrees** (e.g., a root-level glob vs a macro-expanded non-glob from a sibling expansion), items coexist — no shadowing. All candidates are returned.
- This is why time-travel is only an error when the non-glob comes from a *different* macro expansion. If the non-glob was always in the CST (same parent as the glob), it shadows normally and there's no ambiguity.

### Glob laziness

Glob stems are *recorded* in the MEM-map but their contents are NOT inlined. The provisional MEM-map contains `GlobStem { source_module }` entries. When `resolve_memmap_path` searches for a name:
1. First check all `NamedMember` entries (walking the full tree).
2. If not found (or to collect all candidates), for each `GlobStem`, call `module_memmap(source_module)` and search *that* MEM-map.

Glob contents are never copied into the current module's MEM-map — they're resolved on-the-fly during lookup. The MEM-map itself only grows by adding named members and macro expansions.

### CST lowering changes

Currently `lower_item` maps `"macro_invocation"` and `"macro_rules_definition"` (tree-sitter node kinds) to `Item::Error`. For the MEM-map:

- `module_memmap` reads the CST directly (via tree-sitter, same as `file_item_tree`) and produces `MemmapEntry` values. It does NOT go through the `Item` enum for macro-related nodes.
- `"macro_rules_definition"` → `MemmapEntry::Named(NamedMember { kind: MacroDef(...) })`
- `"macro_invocation"` at item level → `MemmapEntry::MacroUse(MacroUse { state: Unresolved })`
- All other items → lowered via existing `lower_item` logic, wrapped as `MemmapEntry::Named` or `MemmapEntry::Anon`

The existing `Item` enum and `file_item_tree` can remain for body resolution (which doesn't need macro expansion). `module_memmap` is the new entry point for module-level queries.

## Key queries

### `module_memmap(module)` — the fixpoint

A `#[salsa::fixpoint]` tracked function. Computes the full MEM-map for a module.

**Initial/fallback value**: the empty set (required by salsa for cycle recovery).

**Algorithm**:
1. Seed from the CST: named items, use-redirects, glob stems, and unresolved macro uses.
2. For each unresolved macro use, call `resolve_memmap_path` to resolve the macro path:
   - If it resolves to one or more macro definitions → expand against all of them (see "Multiple candidates" below). Add expanded items as a subtree under this macro use node.
   - If it resolves to something that isn't a macro → mark as error.
   - If it resolves to nothing → leave unresolved (may resolve on next iteration).
3. Salsa re-executes until the result stabilizes.

Cross-module cycles (A globs from B, B globs from A) are handled automatically by salsa's cycle recovery — each module's MEM-map grows monotonically until convergence.

**Depth limit**: 128 per expansion chain (same as rustc's recursion limit). If `outer!()` → `inner!()` → `deeper!()`, that's depth 3. If any single chain exceeds 128, emit an error and stop expanding that chain.

### `expand_macro(macro_def, macro_input)` — leaf query

A tracked function that runs macro expansion. Deterministic, does not read any MEM-map, does not participate in the fixpoint. `MacroDef` is a tracked struct created during CST lowering (stable across fixpoint iterations).

**Initial implementation**: only supports `macro_rules!` with a single no-argument arm:
```rust
macro_rules! $name {
    () => { $tokens }
}
```
Expansion = take `$tokens` and parse as items. No metavariables, no repetition.

### `resolve_name(module, name)` — public API

Used *after* the MEM-map has converged. Reads `module_memmap`, flattens the tree, and applies final priority rules:

1. Non-glob (explicit items, use-redirects, macro-expanded items) — highest
2. Glob imports
3. Extern prelude
4. Std prelude — lowest

Two non-globs with the same name in the same namespace = conflict (reported by validation).

### `memmap_errors(module)` — validation

A tracked function that inspects the converged MEM-map and returns `Vec<MemmapError>`:
- **Time-travel violations**: a glob-resolved name now has a non-glob entry from a sibling expansion that would have taken priority.
- **Duplicate non-glob names**: two non-glob items with the same name in the same namespace.
- **Ambiguous macro resolution**: a macro invocation resolved to multiple candidates (multiple expansions stored).
- **Unresolved macros**: macro paths that never resolved after convergence.

`module_memmap` itself is pure — it just grows the set. All error logic lives here.

## Monotonicity invariant

The MEM-map only grows, never shrinks. This is what makes `#[salsa::fixpoint]` sound.

If a macro path resolves via a glob on iteration N, but iteration N+1 introduces a non-glob with the same name (from a sibling expansion), we do NOT replace the glob resolution. We keep *both* expansions and report an ambiguity error in validation. The set always grows.

## Multiple candidates

When `resolve_memmap_path` returns multiple results for a macro's path, we create **multiple `MacroUse` entries** for the same invocation — one per candidate, each expanded independently. Validation reports an error if a macro invocation site has more than one corresponding `MacroUse` entry. This is analogous to how multiple items with the same name coexist and produce a conflict — we record everything, validate after convergence.

## Time-travel detection

A macro path might resolve via a glob early in the fixpoint, but then a later expansion introduces a non-glob name that *should* have taken priority. Example: `baz::n!()` resolves `baz` via `use foo::*`, but `bar::m!()` expands to produce `mod baz` (non-glob).

The tree structure (expanded items as subnodes of their macro invocation) enables detecting these after convergence: check whether any glob-resolved name now has a non-glob entry from a sibling/later expansion.

**Resolution**: hard error (matching rustc's E0659). Silently letting non-glob win would violate soundness — the program's meaning would depend on expansion order, and rustc rejects it.

## What's NOT in scope

- **Complex `macro_rules!` patterns** — metavariables, repetition, multiple arms
- **Proc-macro bang expansion** — needs TcxDb integration
- **Attribute macros** (`#[tokio::main]`) — transforms items, different problem
- **Hygiene** — macro hygiene / span contexts
- **Body-level macro expansion** — `vec![]`, `format!()` inside function bodies
- **`macro_rules!` textual scoping** — sage treats all `macro_rules!` as module-scoped (see FAQ)

## Test cases

Tests are grouped by category. Each test uses only no-arg `macro_rules!` (the initial supported form). Cross-crate tests note where external crate behavior is assumed.

### Precedence / shadowing

#### Test P1: Named import beats glob

```rust
mod a { pub struct Foo; }
mod b { pub struct Foo; }
use a::*;
use b::Foo;  // named import
fn main() { let _ = Foo; }
```
**Expected**: OK — `Foo` resolves to `b::Foo`. Named import (non-glob) beats glob.

#### Test P2: Macro-expanded item beats glob

```rust
mod foo {
    pub mod bar {
        macro_rules! m { () => { mod baz { pub(crate) struct Test(pub bool); } } }
        pub(crate) use m;
    }
    pub mod baz { pub(crate) struct Test(pub u32); }
}
use foo::*;
bar::m!();
fn main() { baz::Test(true); }
```
**Expected**: OK — `baz::Test` resolves to the macro-expanded `Test(bool)`. Non-glob beats glob.

#### Test P3: Glob beats extern prelude

```rust
// Assume extern crate `log` exists with a module `log::Level`
mod mylog { pub struct Level; }
use mylog::*;
fn main() { let _ = Level; }
```
**Expected**: OK — `Level` resolves to `mylog::Level` via glob, not to anything from extern prelude. Glob has higher priority than extern prelude.

#### Test P4: Glob beats std prelude

```rust
mod custom { pub struct Option; }
use custom::*;
fn main() { let _ = Option; }
```
**Expected**: OK — `Option` resolves to `custom::Option` via glob, not `std::option::Option` from the std prelude.

#### Test P5: Two globs, same name — ambiguity

```rust
mod a { pub struct Foo; }
mod b { pub struct Foo; }
use a::*;
use b::*;
fn main() { let _ = Foo; }
```
**Expected**: ERROR — ambiguous, two glob-imported `Foo`.

#### Test P6: Named import + macro-expanded item, same name — error

```rust
mod other { pub struct Foo; }
use other::Foo;
macro_rules! m { () => { struct Foo; } }
m!();
```
**Expected**: ERROR — two non-glob items with the same name (`Foo`). Named import and macro-expanded item are both non-glob.

#### Test P7: Explicit item + named import, same name — error

```rust
mod other { pub struct Foo; }
use other::Foo;
struct Foo;
```
**Expected**: ERROR — two non-glob items with the same name.

### Path resolution variants

#### Test R1: `self::` path for macro resolution

```rust
macro_rules! m { () => { struct Foo; } }
self::m!();
fn main() { let _ = Foo; }
```
**Expected**: OK — `self::m` resolves to `m` in the current module.

#### Test R2: `crate::` path for macro resolution

```rust
mod inner {
    macro_rules! m { () => { struct Foo; } }
    pub(crate) use m;
    crate::inner::m!();
}
```
**Expected**: OK — `crate::inner::m` resolves via absolute path from crate root.

#### Test R3: `super::` path for macro resolution

```rust
macro_rules! m { () => { struct Foo; } }
pub(crate) use m;
mod child {
    super::m!();
    fn test() { let _ = Foo; }
}
```
**Expected**: OK — `super::m` resolves to `m` in the parent module. `Foo` is visible in `child`'s MEM-map.

#### Test R4: Bare identifier resolves to local module before extern crate

```rust
// Assume extern crate `foo` exists
mod foo {
    macro_rules! m { () => { struct Local; } }
    pub(crate) use m;
}
foo::m!();
fn main() { let _ = Local; }
```
**Expected**: OK — `foo` resolves to the local module, not the extern crate. Flexi-lookup checks local MEM-map first.

#### Test R5: Bare identifier falls through to extern prelude

```rust
// Assume extern crate `serde` exists with `serde::m` macro
// (cross-crate, resolved via TcxDb)
serde::m!();
```
**Expected**: OK — `serde` not found locally, falls through to extern prelude, resolves to the extern crate.

#### Test R6: Multi-segment path through nested modules

```rust
mod a {
    pub mod b {
        macro_rules! m { () => { struct Deep; } }
        pub(crate) use m;
    }
}
a::b::m!();
fn main() { let _ = Deep; }
```
**Expected**: OK — path traverses `a` → `b` → `m`, each step computing `module_memmap` on the intermediate module.

#### Test R7: Path through a use-redirect

```rust
mod inner {
    macro_rules! m { () => { struct Foo; } }
    pub(crate) use m;
}
use inner::m as make_foo;
make_foo!();
fn main() { let _ = Foo; }
```
**Expected**: OK — `make_foo` is a named member (redirect) that points to `inner::m`. Resolves to the `MacroDef`.

#### Test R8: `self::x` vs `x` when extern crate `x` exists

```rust
// Assume extern crate `x` exists
mod x {
    macro_rules! m { () => { struct Local; } }
    pub(crate) use m;
}
self::x::m!();  // unambiguously local
fn main() { let _ = Local; }
```
**Expected**: OK — `self::x` always refers to the local module, never the extern crate. Contrast with bare `x::m!()` which would also prefer local but *could* fall through to extern prelude if no local `x` existed.

### Macro expansion interactions

#### Test M1: Macro expands to a `use` redirect

```rust
mod source { pub struct Widget; }
macro_rules! m { () => { use source::Widget; } }
m!();
fn main() { let _ = Widget; }
```
**Expected**: OK — `m!()` expands to a use-redirect. `Widget` is a named member (redirect) in the expansion subtree, visible as non-glob.

#### Test M2: Macro expands to a glob import

```rust
mod source { pub struct Widget; pub struct Gadget; }
macro_rules! m { () => { use source::*; } }
m!();
fn main() { let _ = Widget; let _ = Gadget; }
```
**Expected**: OK — `m!()` expands to a glob stem in its subtree. `Widget` and `Gadget` are resolvable via that glob.

#### Test M3: Macro expands to a `mod` declaration

```rust
macro_rules! m { () => { mod inner { pub struct Foo; } } }
m!();
fn main() { let _ = inner::Foo; }
```
**Expected**: OK — `m!()` expands to a module. `inner` is a named member in the expansion subtree. `inner::Foo` resolves by computing `module_memmap(inner)`.

#### Test M4: Macro expands to `macro_rules!` + re-export (chained)

```rust
macro_rules! m { () => { macro_rules! n { () => { struct X; } } pub(crate) use n; } }
m!();
n!();
fn main() { let _ = X; }
```
**Expected**: OK — `m!()` defines `n` and re-exports it. `n` is visible in the MEM-map. `n!()` resolves and expands to `struct X`.

#### Test M5: Macro expansion produces only `impl` (anonymous)

```rust
struct Foo;
macro_rules! m { () => { impl Foo { fn bar() {} } } }
m!();
```
**Expected**: OK — `m!()` expands to an anonymous item (`impl`). No new names in the module namespace. The `impl` is recorded as `MemmapEntry::Anon`.

#### Test M6: Empty macro expansion

```rust
macro_rules! m { () => {} }
m!();
```
**Expected**: OK — `m!()` expands to nothing. The `MacroUse` has `Expanded(vec![])`. No error.

#### Test M7: Same macro invoked multiple times

```rust
macro_rules! m { () => { struct Foo; } }
m!();
m!();
```
**Expected**: ERROR — two non-glob items named `Foo` (one from each expansion). Same as Test 5.

#### Test M8: Macro expansion in child module, accessed from parent

```rust
mod child {
    macro_rules! m { () => { pub struct Foo; } }
    pub(crate) use m;
    m!();
}
fn main() { let _ = child::Foo; }
```
**Expected**: OK — `child::Foo` resolves by computing `module_memmap(child)`, which expands `m!()` and produces `Foo`.

### Time-travel / ambiguity

#### Test T1: Classic time-travel (E0659)

```rust
mod foo {
    pub mod bar {
        macro_rules! m { () => { mod baz { pub(crate) struct X; } } }
        pub(crate) use m;
    }
    pub mod baz { pub(crate) struct X; }
}
use foo::*;
bar::m!();
fn main() { let _ = baz::X; }
```
**Expected**: ERROR — `baz` is ambiguous. Glob-imported `foo::baz` vs macro-expanded `baz` from `bar::m!()`. Time-travel violation.

#### Test T2: Macro expansion introduces glob that would change earlier resolution

```rust
mod source { pub struct Conflict; }
macro_rules! m { () => { use source::*; } }
struct Conflict;
m!();
fn main() { let _ = Conflict; }
```
**Expected**: OK — `Conflict` resolves to the explicit `struct Conflict` (non-glob). The glob from `m!()`'s expansion doesn't shadow it. No time-travel because the explicit item was always there.

#### Test T3: Multiple macro candidates — ambiguous resolution

```rust
mod a {
    macro_rules! m { () => { struct FromA; } }
    pub(crate) use m;
}
mod b {
    macro_rules! m { () => { struct FromB; } }
    pub(crate) use m;
}
use a::*;
use b::*;
m!();
```
**Expected**: ERROR — `m` resolves to two candidates (from two globs). Both are expanded, validation reports ambiguous macro resolution.

#### Test T4: Cross-module fixpoint convergence

```rust
mod a {
    pub use super::b::*;
    macro_rules! ma { () => { pub struct FromA; } }
    pub(crate) use ma;
    ma!();
}
mod b {
    pub use super::a::*;
    macro_rules! mb { () => { pub struct FromB; } }
    pub(crate) use mb;
    mb!();
}
fn main() {
    let _ = a::FromB;  // visible via a's glob from b
    let _ = b::FromA;  // visible via b's glob from a
}
```
**Expected**: OK — mutual glob imports. `module_memmap(a)` depends on `module_memmap(b)` and vice versa. Salsa fixpoint converges both. `FromA` is visible in `b` via glob, `FromB` is visible in `a` via glob.

### Cross-crate

#### Test C1: Macro from external crate via explicit path

```rust
// Assume extern crate `helpers` has:
//   pub macro_rules! make_thing { () => { struct Thing; } }
//   (re-exported at crate root)
helpers::make_thing!();
fn main() { let _ = Thing; }
```
**Expected**: OK — `helpers` resolves via extern prelude. `make_thing` found in `helpers`'s module children (via TcxDb). Expansion produces `struct Thing`.

#### Test C2: Glob from external crate brings macro into scope

```rust
// Assume extern crate `helpers` re-exports macro `make_thing`
use helpers::*;
make_thing!();
fn main() { let _ = Thing; }
```
**Expected**: OK — glob from external crate. `make_thing` found via glob lookup into `helpers`'s module (TcxDb). Resolves and expands.

#### Test C3: Local module shadows extern crate name

```rust
// Assume extern crate `foo` exists
mod foo {
    macro_rules! m { () => { struct Local; } }
    pub(crate) use m;
}
foo::m!();
fn main() { let _ = Local; }
```
**Expected**: OK — `foo` resolves to local module (flexi-lookup finds it in MEM-map before checking extern prelude). Same as Test R4.

### Edge cases

#### Test E1: Unresolved macro path — error

```rust
nonexistent::m!();
```
**Expected**: ERROR — macro path `nonexistent::m` never resolves. After convergence, `memmap_errors` reports unresolved macro.

#### Test E2: Macro path resolves to non-macro — error

```rust
mod foo { pub struct NotAMacro; }
foo::NotAMacro!();
```
**Expected**: ERROR — `foo::NotAMacro` resolves to a struct, not a macro. `MacroUseState::Error`.

#### Test E3: Depth limit exceeded

```rust
macro_rules! m { () => { m!(); } }
m!();
```
**Expected**: ERROR — infinite recursion. Hits depth limit (128). Reports error, stops expanding.

#### Test E4: Macro expansion in different namespace — no conflict with same-name item

```rust
macro_rules! m { () => { const foo: u32 = 1; } }
m!();
mod foo {}
```
**Expected**: OK — `const foo` in ValueNS, `mod foo` in TypeNS. No conflict.

#### Test E5: Glob shadowed by named import — laziness

```rust
mod big_module { pub struct Unused1; pub struct Unused2; /* ... many items ... */ }
mod small { pub struct Foo; }
use big_module::*;
use small::Foo;
fn main() { let _ = Foo; }
```
**Expected**: OK — `Foo` resolves to `small::Foo` (named import). Ideally, `module_memmap(big_module)` is NOT computed because the glob is shadowed for this name. (Laziness optimization — may not be observable in correctness tests, but important for performance.)

#### Test E6: Use redirect chain

```rust
mod a {
    macro_rules! m { () => { struct Foo; } }
    pub(crate) use m;
}
mod b { pub use super::a::m; }
mod c { pub use super::b::m; }
c::m!();
fn main() { let _ = Foo; }
```
**Expected**: OK — `c::m` → redirect to `b::m` → redirect to `a::m` → `MacroDef`. Resolves through chain of redirects.

#### Test E7: Macro and non-macro with same name in different namespaces

```rust
macro_rules! foo { () => { struct Expanded; } }
fn foo() {}
foo!();
fn main() { let _ = Expanded; }
```
**Expected**: OK — `macro_rules! foo` is in MacroNS, `fn foo` is in ValueNS. No conflict. `foo!()` resolves to the macro.

## FAQ

**Q: Does sage handle `macro_rules!` textual scoping?**
No. All `macro_rules!` definitions are module-scoped, visible everywhere in the module. This is an opinionated simplification — programs relying on ordering would behave differently. Revisit later if needed.

**Q: Where do `impl` blocks from builtin derives go?**
In the MEM-map as anonymous items. Builtin derives are recorded as unexpanded macro uses (since they produce no new *names*), but when downstream queries need `impl` blocks, they expand those markers on demand and collect the anonymous items.

**Q: What about attribute macros (`#[tokio::main]`)?**
Out of scope. They transform items rather than producing new ones — different problem.

**Q: How deep does recursive expansion go?**
Depth limit 128 (same as rustc). Exceeded → error.

## Implementation plan

### Phase 1: MEM-map data model and basic resolution

**Goal**: Implement `module_memmap` as the single source of truth, replacing `module_items`. CST-seeded named members, use-redirects, and glob stems. Rewrite `resolve_name` to query the MEM-map. No macro expansion yet — invocations recorded but unresolved.

1. Define `ModuleMemmap` tracked struct (named members, macro uses, glob stems)
2. Implement `module_memmap` seeding from CST
3. Rewrite `resolve_name` to read from `module_memmap`
4. Remove/deprecate `module_items` as a public query
5. Existing tests pass

### Phase 2: Macro expansion in the fixpoint

**Goal**: Add `MacroDef` in CST lowering, trivial `expand_macro`, wire `module_memmap` to resolve and expand. Add `#[salsa::fixpoint]`.

1. Create `MacroDef` tracked struct during CST lowering
2. Implement `expand_macro(macro_def, macro_input)` for no-arg case
3. In `module_memmap`, resolve macro paths via `resolve_memmap_path`, call `expand_macro`, add results as subtree
4. Add `#[salsa::fixpoint]` with empty-set fallback
5. Handle depth limit

**Tests**: 2, 3, 4, 8

### Phase 3: Validation query

**Goal**: `memmap_errors(module)` inspects the converged MEM-map and returns `Vec<MemmapError>`.

1. Time-travel violations
2. Duplicate non-glob names
3. Ambiguous macro resolution (multiple expansions)
4. Unresolved macros

**Tests**: 5, 6, 11

## Implementation status

- [x] Phase 1: MEM-map data model and basic resolution
- [x] Phase 2: Macro expansion in the fixpoint
- [ ] Phase 3: Validation query

### Deviations from plan

1. **Glob stems resolved eagerly during seeding**: The WIP design says `module_memmap` stores `GlobStem { source_module }`. To resolve the glob path to a module, `module_memmap` needs `source_root` and `crate_root` parameters (not in the original design). Added these as tracked function parameters. This is compatible with Phase 2's fixpoint design since the fixpoint function will also need these for macro path resolution.

2. **`use_wildcard` glob path lowering fix**: tree-sitter-rust 0.24 doesn't use a field name for the path child of `use_wildcard` nodes. Fixed `flatten_use_tree` to find the path by node kind instead of `child_by_field_name("path")`.

3. **Namespace handling for redirects**: Use-redirects are stored with `ns: Namespace::Type` but match any namespace during resolution (since the target's namespace isn't known at seeding time). Items use `member.ns == ns` for exact namespace matching. Results are deduplicated by Symbol identity to avoid false ambiguities (e.g., a struct appearing in both Type and Value namespaces).

4. **`module_items` and `module_use_imports` retained**: These queries are still used by `definition()`, `find_method()` in tests, and the derive expansion system. They were not removed/deprecated in Phase 1 to minimize blast radius. `resolve_name` is the only function that switched to `module_memmap`.

5. **`seed_from_cst` parses tree-sitter directly for macro nodes**: Rather than extending `file_item_tree` to handle `macro_definition` and `expression_statement` (which would require changing the `Item` enum), `module_memmap` parses the CST directly for macro-related nodes and uses `file_item_tree` for regular items. This avoids touching the existing lowering pipeline.

6. **`self::` paths resolved locally without recursive `module_memmap` call**: For `self::m!()`, the macro path is resolved by searching the current module's entries directly (the snapshot), rather than calling `module_memmap(db, module, ...)` which would create a cycle. Multi-segment `self::a::b::m` paths still use `walk_path_to_macro` for intermediate modules.

7. **Snapshot-based resolution during expansion**: `resolve_and_expand_macros` clones the entries as a snapshot before mutating. The snapshot is used for macro path resolution while the original entries are mutated with expansion results. Recursive expansions use the same root snapshot.

8. **`expand_macro` uses `Item::Error` as placeholder**: Expanded items (structs, fns, etc.) are represented as `NamedMember { kind: Item(Item::Error(...)) }` since we only need the name and namespace for resolution. Full lowering of expanded items is deferred to when downstream queries need the actual IR.

### Open issues

(None.)

---

## Rejected alternatives

<details>
<summary>Older design: module_expanded_items (no fixpoint)</summary>

An earlier design used a `module_expanded_items` query that broke the cycle by restricting macro path resolution to only read raw `module_items` (not expanded items). This avoided the need for a fixpoint but meant macro-expanded items could never be used to resolve other macro paths — a limitation the MEM-map design removes.

Key differences:
- Old: macro path resolution cannot see expanded items (cycle broken by restriction)
- New: macro path resolution sees the full provisional MEM-map (cycle broken by salsa fixpoint convergence)

The old design also separated derive expansion from item-level macro expansion into different phases. The MEM-map design unifies them.
</details>
