# WIP: Name resolution and derive expansion

## First spike goal

Given a module path like `mini_redis::cmd::get`:

1. Resolve the path to its `SourceFile` via the module tree.
2. Resolve all `use` imports in that module.
3. Resolve derive attribute names to their definitions (builtin or
   proc-macro).
4. Expand builtin derives into `impl` blocks. Mark proc-macro derives
   as opaque.
5. Pretty-print the module with expanded impls.

All of this must be **on-demand** — only the salsa queries needed for
the requested module should fire.

### Integration tests

Each test case produces **two snapshots** (checked with `expect-test`):

- **Expanded output** — the pretty-printed module with derive impls.
- **Query log** — the list of salsa queries that fired. This is a
  regression test for demand-driven behavior: if unrelated modules
  appear in the log, something is wrong.

Test cases to cover:

- Module with builtin derives (e.g. `cmd::get` — `Debug`, `Clone`)
- Module with proc-macro derives (e.g. clap's `Parser` — logged as
  opaque)
- Module with no derives (just use resolution)
- Leaf module vs module that re-exports children

## Design decisions

### TyCtxt lifetime and TcxDb trait

The entire `cargo sage` workflow runs inside the `after_expansion`
callback. The salsa database and `TyCtxt` coexist for the duration.

External crate metadata is accessed through a `TcxDb` trait that
speaks sage IR types, keeping `sage-ir` rustc-free:

```rust
trait TcxDb {
    fn extern_crate(&self, name: &str) -> Option<CrateNum>;
    fn module_children<'db>(&self, db: &'db dyn Db, crate_num: CrateNum, def_index: DefIndex)
        -> Vec<(Name<'db>, Symbol<'db>, Namespace)>;
    fn is_builtin_derive(&self, crate_num: CrateNum, def_index: DefIndex) -> bool;
}
```

The `Database` struct holds a `Box<dyn TcxDb + 'tcx>`, accessible via
a method on the `Db` trait. `Database<'tcx>` carries the lifetime —
no erasure. The impl lives in the `sage` binary crate and translates
`TyCtxt` queries into sage types.

- `extern_crate(name)` → returns `CrateNum`. The caller constructs
  `Module` with `ModuleSource::External(crate_num, CRATE_DEF_INDEX)`.
- `module_children(crate_num, def_index)` → returns sage
  `(Name, Symbol, Namespace)` triples. Called by `definition()` for
  external modules. Items that live in multiple namespaces (e.g. tuple
  structs: type + value) produce multiple entries with distinct
  `Symbol`s. Only `pub` items are returned — `TcxDb` is never used
  for workspace-local crates. Asserts that `crate_num` is not
  `LOCAL_CRATE` (0).
- `is_builtin_derive(crate_num, def_index)` → used after resolution to
  decide builtin vs proc-macro expansion. Checks both
  `DefKind::Macro(MacroKind::Derive)` and `has_attr(rustc_builtin_macro)`
  to avoid false positives on builtin bang macros. Asserts that
  `crate_num` is not `LOCAL_CRATE` (0).

### `module_children` implementation details

Uses `child.res.opt_def_id()` — if `None` (primitives, non-macro
attrs), skip. If `Some(def_id)`, map to `Symbol::External`. Re-exports
are transparent: the `DefId` points at the original definition, and
we don't track the re-export chain.

### Module tree is on-demand

Resolving `mini_redis::cmd::get` walks segment by segment:
`definition(root, "cmd")` → resolve file → `definition(cmd, "get")`
→ resolve file. Each step parses only the file it needs via
`file_item_tree`. Sibling modules are not parsed.

We cannot assume file paths match module names (`#[path = "..."]`
exists), so we must parse each file on the way down to find its `mod`
declarations.

### Module vs ModItem

`ModItem` is the syntactic node from lowering (`mod foo;` or
`mod foo { ... }`). `Module` is the resolved concept — a module you
can query for children, imports, etc. A tracked function maps
`ModItem` → `Module`, resolving file-based modules to `SourceFile`s.
The crate root is a `Module` constructed directly from the root
`SourceFile` (no `ModItem`).

### Symbol stays flat

No sub-kinds in the interned struct. Namespace membership is derived
from the underlying `Item`/`DefKind`, not stored on `Symbol`. Tracked
methods (`sig`, `fields`, etc.) provide kind-specific data on demand.

### Use desugaring is eager

Use declarations are flattened into atomic `UseImport` entries during
`file_item_tree` lowering. The CST walk is ~40 lines (recursive prefix
accumulation) with no external dependencies, so there's no benefit to
a separate lazy query. `UseItem` is replaced by `Vec<UseImport>` in
the item tree.

### Derive expansion result

Derive resolution produces a uniform result:

```rust
enum DeriveResult<'db> {
    Builtin { impls: Vec<ImplItem<'db>> },
    ProcMacro { symbol: Symbol<'db> },  // opaque for now
}
```

Both go through the same name resolution and expansion path; they
diverge at the expansion step. Builtins generate impl blocks directly.
Proc-macros go through the same interface but return empty tokens for
now (dylib invocation added later).

### Query logging

Override `salsa::Database::salsa_event()` on the database to capture
query names with key arguments. Integration tests snapshot the log
to verify demand-driven behavior. No dependency edges — just names.

## New types

### Module

```rust
#[salsa::interned]
struct Module<'db> {
    source: ModuleSource<'db>,
}

enum ModuleSource<'db> {
    Local { file: SourceFile, parent: Option<Module<'db>> },
    External(CrateNum, DefIndex),
}
```

### Symbol

```rust
#[salsa::interned]
struct Symbol<'db> {
    source: SymbolSource<'db>,
}

enum SymbolSource<'db> {
    Local(Item<'db>),
    External(CrateNum, DefIndex),
}
```

### ExternPrelude

```rust
/// Stored as a side table on the database, not a salsa input.
struct ExternPrelude {
    crates: HashMap<String, CrateNum>,
}
```

## Module tree

The `definition(db, module, name)` query scans a module's
`file_item_tree` for an item with the given name. It does not recurse
into child modules.

For external crates, the module tree is implicit — `module_children`
on `TyCtxt` gives children on demand.

## Name resolution algorithm

```
resolve(name, namespace, scope) -> Result<Symbol, ResolutionError>

1. Walk outward: block → function → module → crate root
2. At each scope:
   a. Check declared items + explicit use imports in target namespace
   b. Exactly one → Ok(symbol)
   c. Multiple → Err(Ambiguity)
   d. Zero → check glob imports, search their targets
   e. Glob: one match → Ok. Multiple → Err(Ambiguity).
   f. Glob + outer-scope candidate → Err(Ambiguity)
3. After crate root: check extern prelude
4. Then: check std prelude (known set of names)
5. Nothing → Err(Unresolved)
```

### Use imports

Two prerequisite fixes in the lowering pass:

1. **Path segments** — `lower_path()` currently produces single-segment
   paths with the full text (e.g. `"std::collections::HashMap"` as one
   segment). Fix to walk `scoped_identifier` nodes and produce proper
   multi-segment `Vec<Name>`. This affects all paths (type refs, trait
   paths, etc.), not just use statements.

2. **Use desugaring** — replace the current `UseItem` lowering (which
   produces a single `Path` + optional alias) with a recursive CST walk
   that produces flat `UseImport` entries. The `UseItem` type is
   removed; `file_item_tree` produces `Vec<UseImport>` directly.

The relevant tree-sitter node kinds for use declarations:

- `scoped_identifier` — `foo::Bar`
- `scoped_use_list` — `foo::{A, B}`, prefix + `use_list`
- `use_as_clause` — `foo::Bar as Baz`
- `use_wildcard` — `foo::*`
- `use_list` — `{A, B, C}` (can nest)
- `self` / `crate` / `super` — distinct node kinds (not `identifier`)

A `use` declaration desugars into one or more `UseImport` entries
(purely syntactic — no path resolution yet):

```rust
/// A single flattened use import (syntactic, not resolved).
#[salsa::tracked]
struct UseImport<'db> {
    /// The full path as written, e.g. [foo, bar].
    path: Path<'db>,
    kind: UseKind<'db>,
}

enum UseKind<'db> {
    Named(Name<'db>),  // use foo::bar / use foo::bar as baz
    Glob,              // use foo::bar::*
    Unnamed,           // use foo::Bar as _
}
```

`use foo::bar` is `{ path: [foo, bar], kind: Named("bar") }` — the
alias always defaults to the last segment. `use foo::{self, Bar}`
normalizes `self` to `{ path: [foo], kind: Named("foo") }` — trailing
`self` never appears in the output.

Resolution of the path happens on demand during name resolution — if
we're resolving name `X`, we walk the `UseImport` whose alias is `X`
but don't touch unrelated imports.

Desugaring examples:

```
use bytes::Bytes;
  → { path: [bytes, Bytes], alias: Bytes }

use crate::{Connection, Frame};
  → { path: [crate, Connection], alias: Connection }
  → { path: [crate, Frame], alias: Frame }

use crate::cmd::{Get, Ping};
  → { path: [crate, cmd, Get], alias: Get }
  → { path: [crate, cmd, Ping], alias: Ping }

use tokio::time::{self, Duration};
  → { path: [tokio, time], alias: time }
  → { path: [tokio, time, Duration], alias: Duration }

use std::io::{self, Cursor};
  → { path: [std, io], alias: io }
  → { path: [std, io, Cursor], alias: Cursor }

use foo::Bar as Baz;
  → { path: [foo, Bar], alias: Baz }

use std::collections::*;
  → { path: [std, collections], is_glob: true }

use self::inner::Thing;
  → { path: [self, inner, Thing], alias: Thing }

use super::other::Stuff;
  → { path: [super, other, Stuff], alias: Stuff }
```

#### First-segment resolution (edition 2018+)

In `use` paths, the first segment resolves against:

- `crate` → workspace crate root module
- `self` → current module
- `super` → parent module
- bare identifier → checked against the **current module's items first**,
  then the **extern prelude**. Local items shadow the extern prelude.
  (Use `::foo` to force extern prelude lookup.)

So `use foo::Bar` inside `mod outer` resolves `foo` as a child of
`outer`, not the crate root. This is the current module's items, not
the crate root's items. To reach the crate root, use `use crate::...`.

#### `self` in brace syntax

`use foo::{self, Bar}` imports the module `foo` itself (as `foo`) plus
`foo::Bar`. Note: `self` in this position only imports from the **type
namespace** — it won't import a function with the same name.

#### Resolution precedence tests

These should be included in the test suite to verify first-segment
resolution behavior.

```rust
// TEST: local module shadows extern crate
// Expected: resolves to local mod serde, not extern crate serde
mod serde {
    pub fn hello() {}
}
use serde::hello; // OK — resolves to local mod
```

```rust
// TEST: use resolves against current module, not crate root
// Expected: `use inner::Thing` works in outer (inner is a child)
mod outer {
    pub mod inner {
        pub struct Thing;
    }
    use inner::Thing; // OK — inner is a child of outer
}
```

```rust
// TEST: use does NOT resolve siblings
// Expected: error — inner is not a child of sibling
mod outer {
    pub mod inner {
        pub struct Thing;
    }
    mod sibling {
        use inner::Thing; // ERROR: unresolved import
    }
}
```

```rust
// TEST: glob import shadows extern prelude
// Expected: resolves to glob-imported serde, not extern crate
mod stuff {
    pub mod serde {
        pub fn hello() {}
    }
}
use stuff::*;
use serde::hello; // OK — glob-imported serde shadows extern crate
```

```rust
// TEST: explicit use import shadows extern prelude
// Expected: resolves to use-imported serde, not extern crate
mod stuff {
    pub mod serde {
        pub fn hello() {}
    }
}
use stuff::serde;
use serde::hello; // OK — use-imported serde shadows extern crate
```

```rust
// TEST: ::prefix forces extern prelude
// Expected: resolves to extern crate std, not local mod std
mod std {
    pub mod collections {
        pub struct MyHashMap;
    }
}
use ::std::collections::HashMap; // OK — extern crate std
use std::collections::MyHashMap; // OK — local mod std
```

```rust
// TEST: macro_rules! and derive macro coexist with same name
// Expected: #[derive(Debug)] resolves to the derive macro,
//           Debug!() resolves to the macro_rules! macro
use std::prelude::v1::Debug;

macro_rules! Debug { () => { 0 } }

#[derive(Debug)]
struct Foo { }

fn main() {
    println!("{}", Debug!());
}
```

### Namespaces

Three namespaces: type, value, macro. The macro namespace is further
subdivided by `MacroKind` (bang, attr, derive) — these are effectively
separate sub-namespaces. A `macro_rules! Debug` and `#[derive(Debug)]`
can coexist with the same name; Rust disambiguates by usage context.

```rust
enum Namespace {
    Type,
    Value,
    Macro(MacroKind),
}

enum MacroKind {
    Bang,
    Attr,
    Derive,
}
```

`module_children` can return multiple entries for the same name in
different macro sub-namespaces. Resolution filters by the specific
`MacroKind` needed: `#[derive(X)]` resolves with
`Namespace::Macro(MacroKind::Derive)`, `X!()` resolves with
`Namespace::Macro(MacroKind::Bang)`.

A single `use` can import into all namespaces (including all macro
sub-namespaces). For derive resolution we need
`Macro(MacroKind::Derive)`, but the infrastructure tracks all of them.

### Prelude

The std prelude is a known set of names. We inject them as if there's
an implicit `use std::prelude::v1::*` at the crate root. Since this is
a glob from an external crate, it's resolved via the dep snapshot —
query `module_children` on `std::prelude::v1`.

### Subsetting restrictions (unchanged)

- `macro_rules!` definitions are registered in the module scope for
  name resolution (`Namespace::Macro(MacroKind::Bang)`) but expansion
  is not implemented — attempting to expand one is an error
- `macro_rules!` scoping is treated as module-scoped (visible
  throughout the module) rather than textual (visible only after the
  definition point). Known gap — incorrect for edge cases but fine
  for mini-redis
- No workspace glob imports
- No proc-macro crates defined in workspace
- No `#[path = "..."]` attributes on modules
- Derive helper attributes not resolved (noted as gap)
- Time-traveling ambiguities from macro-introduced names ignored
- Name resolution cycles panic (default salsa behavior); fixpoint
  resolution deferred

## Derive resolution

Given `#[derive(Foo)]` on an item:

1. Resolve `Foo` in the macro namespace using the algorithm above
2. Result is a `Symbol` pointing to either:
   - A builtin derive (`Debug`, `Clone`, `Default`, etc.)
   - A proc-macro derive from an external crate (`Parser`, `Subcommand`)

### Builtin derives

`Debug`, `Clone`, `Default`, etc. are resolved through the std prelude
like any other name — query `module_children` on `std::prelude::v1`
to discover what's available. The `Symbol` for these will have
`SymbolSource::External` pointing at their `DefId` in the dep snapshot.
We detect them as builtins by checking the `DefId` (e.g.,
`tcx.is_builtin_derive(def_id)` or similar).

### Proc-macro derives

`Parser`, `Subcommand` (clap) — the `Symbol` points at a proc-macro
`DefId`. Expansion means calling the compiled dylib.

## Expansion

### Builtin derives

Generate the impl directly in our IR. We know the struct fields, we
know what `Debug`/`Clone`/`Default` impls look like. Hardcoded for
the spike; follows the same `DeriveResult` interface as proc-macros.

### Proc-macro derives

Full plumbing in place: resolution identifies the proc-macro `Symbol`,
expansion is called through the same interface as builtins. For the
initial spike the proc-macro expander returns empty tokens — the dylib
invocation is easy to add on top once builtins are working.

mini-redis uses `Parser` and `Subcommand` from clap in
`src/bin/cli.rs` and `src/bin/server.rs`, which exercises this path.

## Implementation plan

### Step 1: Fix path lowering

**Files:** `crates/sage-ir/src/lower.rs`

Both `lower_path` functions (item-level at L454 and body-level at L526)
currently stuff the full node text into a single `Name`. Fix to walk
`scoped_identifier` / `scoped_type_identifier` nodes and produce
multi-segment `Vec<Name>`.

```rust
// crates/sage-ir/src/lower.rs — replace both lower_path impls

impl<'db> LowerCtx<'db> {
    fn lower_path(&self, node: Node<'_>) -> Path<'db> {
        let mut segments = Vec::new();
        self.collect_path_segments(node, &mut segments);
        Path::new(self.db, segments, self.span(node))
    }

    fn collect_path_segments(&self, node: Node<'_>, out: &mut Vec<Name<'db>>) {
        // Recurse into scoped_identifier / scoped_type_identifier:
        //   node.child_by_field_name("path") → prefix
        //   node.child_by_field_name("name") → last segment
        // Base cases: identifier, type_identifier, self, crate, super,
        //   primitive_type, metavariable
        // generic_type: recurse into the type child, ignore type_arguments
        ...
    }
}
```

**Test:** Update `mini_redis_signatures.txt` snapshot. Paths like
`crate::Result<Get>` should now show as `[crate, Result]` (with
generics handled separately) instead of a single blob.

---

### Step 2: Use flattening

**Files:** `crates/sage-ir/src/lower.rs`, `crates/sage-ir/src/item.rs`,
`crates/sage-ir/src/types.rs`, `crates/sage-ir/src/display.rs`

Define `UseImport` and `UseKind` in `types.rs`:

```rust
#[salsa::tracked]
pub struct UseImport<'db> {
    pub path: Path<'db>,
    pub kind: UseKind<'db>,
    pub span: SpanIndices,
}

#[derive(Copy, Clone, PartialEq, Eq, Hash, salsa::Update)]
pub enum UseKind<'db> {
    Named(Name<'db>),
    Glob,
    Unnamed,
}
```

Replace `UseItem` in `item.rs`. Change `Item::Use(UseItem)` to carry
the flattened imports — either `Item::Use(Vec<UseImport>)` or remove
`Use` from `Item` and have `file_item_tree` return uses separately.

Replace `lower_use` with a recursive walk:

```rust
impl<'db> LowerCtx<'db> {
    fn lower_use(&mut self, node: Node<'_>) -> Vec<UseImport<'db>> {
        let mut imports = Vec::new();
        let mut prefix = Vec::new();
        // node's child (after visibility) is the use tree root
        self.flatten_use_tree(node.child(...)?, &mut prefix, &mut imports);
        imports
    }

    fn flatten_use_tree(
        &self,
        node: Node<'_>,
        prefix: &mut Vec<Name<'db>>,
        out: &mut Vec<UseImport<'db>>,
    ) {
        // Match on node.kind():
        //   "identifier" | "self" | "crate" | "super" →
        //       leaf: push Named(name), path = prefix + [name]
        //   "scoped_identifier" →
        //       leaf: collect_path_segments into prefix, emit Named(last)
        //   "use_as_clause" →
        //       path from child "path", alias from child "alias"
        //       if alias is "_" → Unnamed, else Named(alias)
        //   "use_wildcard" →
        //       path = prefix (from scoped part), kind = Glob
        //   "scoped_use_list" →
        //       extend prefix from child "path", recurse into "list"
        //   "use_list" →
        //       iterate children, recurse each with current prefix
        //   "self" as trailing in use_list →
        //       emit Named(last segment of prefix), path = prefix
        ...
    }
}
```

**Test:** Update `mini_redis_signatures.txt`. Use statements should
display as atomic imports, e.g.:
```
use crate::Connection
use crate::Db
use bytes::Bytes
```

---

### Step 3: New types — `Module`, `Symbol`, `CrateNum`, `DefIndex`

**Files:** new `crates/sage-ir/src/module.rs`,
new `crates/sage-ir/src/symbol.rs`, update `lib.rs`

```rust
// crates/sage-ir/src/module.rs

/// Opaque crate number (matches rustc's CrateNum).
#[derive(Copy, Clone, PartialEq, Eq, Hash, salsa::Update)]
pub struct CrateNum(pub u32);

/// Opaque definition index within a crate.
#[derive(Copy, Clone, PartialEq, Eq, Hash, salsa::Update)]
pub struct DefIndex(pub u32);

#[salsa::interned]
pub struct Module<'db> {
    pub source: ModuleSource<'db>,
}

#[derive(Clone, PartialEq, Eq, Hash, salsa::Update)]
pub enum ModuleSource<'db> {
    /// Workspace module backed by a source file.
    Local {
        file: SourceFile,
        parent: Option<Module<'db>>,
    },
    /// External crate module, queryable via TcxDb.
    External(CrateNum, DefIndex),
}
```

```rust
// crates/sage-ir/src/symbol.rs

#[salsa::interned]
pub struct Symbol<'db> {
    pub source: SymbolSource<'db>,
}

#[derive(Copy, Clone, PartialEq, Eq, Hash, salsa::Update)]
pub enum SymbolSource<'db> {
    Local(Item<'db>),
    External(CrateNum, DefIndex),
}
```

**Test:** Types compile. No behavioral tests yet.

---

### Step 4: `TcxDb` trait

**Files:** new `crates/sage-ir/src/tcx.rs`, update `db.rs`

```rust
// crates/sage-ir/src/tcx.rs

pub trait TcxDb {
    /// Look up an external crate by name.
    fn extern_crate(&self, name: &str) -> Option<CrateNum>;

    /// List the children of an external module.
    /// Items in multiple namespaces (e.g. tuple structs) produce
    /// multiple entries with distinct Symbols.
    /// Only returns pub items. Asserts crate_num != LOCAL_CRATE.
    fn module_children<'db>(
        &self,
        db: &'db dyn Db,
        crate_num: CrateNum,
        def_index: DefIndex,
    ) -> Vec<(Name<'db>, Symbol<'db>, Namespace)>;

    /// Is this external def a builtin derive macro?
    /// Checks DefKind::Macro(MacroKind::Derive) AND
    /// has_attr(rustc_builtin_macro). Asserts crate_num != LOCAL_CRATE.
    fn is_builtin_derive(
        &self,
        crate_num: CrateNum,
        def_index: DefIndex,
    ) -> bool;
}
```

Add `TcxDb` access to the database:

```rust
// crates/sage-ir/src/db.rs

#[salsa::db]
pub trait Db: salsa::Database {
    fn tcx(&self) -> &dyn TcxDb;
}

pub struct Database<'tcx> {
    storage: salsa::Storage<Self>,
    tcx: Box<dyn TcxDb + 'tcx>,
}
```

For tests without rustc, provide a `NoopTcxDb` that returns empty
results for everything.

**Test:** Existing tests still pass with `NoopTcxDb`.

---

### Step 5: Module resolution queries

**Files:** new `crates/sage-ir/src/resolve.rs`

```rust
// crates/sage-ir/src/resolve.rs

/// Items declared in a module (from file_item_tree for local,
/// from TcxDb for external).
#[salsa::tracked(returns(ref))]
pub fn module_items<'db>(
    db: &'db dyn Db,
    module: Module<'db>,
) -> Vec<Item<'db>>;

/// Use imports in a module (from file_item_tree for local,
/// empty for external — external modules don't have use statements).
#[salsa::tracked(returns(ref))]
pub fn module_use_imports<'db>(
    db: &'db dyn Db,
    module: Module<'db>,
) -> Vec<UseImport<'db>>;

/// Find a direct child definition by name.
/// For local modules: scan module_items.
/// For external modules: call tcx.module_children.
#[salsa::tracked]
pub fn definition<'db>(
    db: &'db dyn Db,
    module: Module<'db>,
    name: Name<'db>,
) -> Option<Symbol<'db>>;

/// Resolve a ModItem to its Module — find the SourceFile for
/// `mod foo;` declarations.
#[salsa::tracked]
pub fn resolve_mod<'db>(
    db: &'db dyn Db,
    parent: Module<'db>,
    mod_item: ModItem<'db>,
) -> Module<'db>;
// Looks for foo.rs or foo/mod.rs relative to parent's file.

/// Resolve a module path like ["mini_redis", "cmd", "get"] to a Module.
/// Walks segment by segment using definition() + resolve_mod().
pub fn resolve_module_path<'db>(
    db: &'db dyn Db,
    root: Module<'db>,
    path: &[&str],
) -> Result<Module<'db>, ResolutionError>;
```

**Test:** `resolve_module_path(root, ["cmd", "get"])` on mini-redis
returns a `Module` whose `SourceFile` is `src/cmd/get.rs`.

---

### Step 6: Name resolution

**Files:** `crates/sage-ir/src/resolve.rs` (extend)

```rust
#[derive(Copy, Clone, PartialEq, Eq)]
pub enum Namespace {
    Type,
    Value,
    Macro,
}

#[derive(Debug)]
pub enum ResolutionError {
    Unresolved,
    Ambiguous,
}

/// Resolve a name in a module's scope.
/// 1. Check declared items matching namespace
/// 2. Check named use imports — resolve matching import's path on demand
/// 3. Check glob imports — resolve each glob's target module, search children
/// 4. Check parent module (if any)
/// 5. Check extern prelude (via tcx.extern_crate)
/// 6. Check std prelude (implicit glob of std::prelude::v1)
#[salsa::tracked]
pub fn resolve_name<'db>(
    db: &'db dyn Db,
    module: Module<'db>,
    name: Name<'db>,
    namespace: Namespace,
) -> Result<Symbol<'db>, ResolutionError>;

/// Resolve a use import's path segment by segment.
/// First segment: crate → root, self → current, super → parent,
///   bare → current module items then extern prelude.
/// Remaining segments: definition(module, segment) for each.
#[salsa::tracked]
pub fn resolve_use_path<'db>(
    db: &'db dyn Db,
    module: Module<'db>,
    import: UseImport<'db>,
) -> Result<Symbol<'db>, ResolutionError>;
```

**Test:** In `cmd/get.rs`, resolve `Connection` → follows
`use crate::{Connection, ...}` → finds `Connection` in crate root.
Resolve `Bytes` → follows `use bytes::Bytes` → extern prelude →
`bytes` crate → `Bytes` symbol.

---

### Step 7: Derive resolution

**Files:** new `crates/sage-ir/src/derive.rs`

```rust
// crates/sage-ir/src/derive.rs

pub enum DeriveResult<'db> {
    Builtin { impls: Vec<ImplItem<'db>> },
    ProcMacro { symbol: Symbol<'db> },
}

/// Resolve and expand all derives on an item.
#[salsa::tracked(returns(ref))]
pub fn expand_derives<'db>(
    db: &'db dyn Db,
    module: Module<'db>,
    item: Item<'db>,
) -> Vec<DeriveResult<'db>>;
// 1. Read item's attrs, find #[derive(...)] attributes
// 2. Parse the token tree to extract individual derive names
// 3. For each name: resolve_name(module, name, Namespace::Macro)
// 4. Check tcx.is_builtin_derive(symbol) → expand or return ProcMacro

/// Extract individual derive names from a #[derive(A, B, C)] attribute.
fn parse_derive_args<'db>(
    db: &'db dyn Db,
    attr: Attr<'db>,
) -> Vec<Name<'db>>;
// Split the TokenTree text on commas, trim whitespace, intern each name.
```

**Test:** On `Get` struct in `cmd/get.rs`, `expand_derives` returns
`[DeriveResult::Builtin { impls: [impl Debug for Get { ... }] }]`.

---

### Step 8: Builtin derive expansion

**Files:** new `crates/sage-ir/src/derive/builtins.rs`

```rust
// crates/sage-ir/src/derive/builtins.rs

/// Generate an impl block for a builtin derive.
pub fn expand_builtin<'db>(
    db: &'db dyn Db,
    derive_name: Name<'db>,
    item: Item<'db>,
) -> ImplItem<'db>;
// Match on derive_name text:
//   "Debug"   → generate impl Debug with fmt method
//   "Clone"   → generate impl Clone with clone method
//   "Default" → generate impl Default with default method
//   "Copy", "Eq", "Hash", "PartialEq" → marker or delegate impls
// Each reads the item's fields/variants to produce the body.

/// Proc-macro stub — returns empty impl.
pub fn expand_proc_macro_stub<'db>(
    db: &'db dyn Db,
    symbol: Symbol<'db>,
    item: Item<'db>,
) -> Vec<ImplItem<'db>>;
// Returns empty vec for now. Interface is ready for dylib call.
```

**Test:** Expand `Debug` on `Get { key: String }` → produces
`impl Debug for Get` with a `fmt` method that writes `"Get"` and
formats `key`.

---

### Step 9: Query logging

**Files:** `crates/sage-ir/src/db.rs`

```rust
// crates/sage-ir/src/db.rs

impl Database<'_> {
    pub fn take_query_log(&self) -> String {
        // Drain the log, format as one query name per line.
        ...
    }
}

#[salsa::db]
impl salsa::Database for Database<'_> {
    fn salsa_event(&self, event: &dyn Fn() -> salsa::Event) {
        let event = event();
        // Filter for WillExecute events (actual query executions).
        // Format: "query_name(key_debug_string)"
        // Push to an internal RefCell<Vec<String>>.
        ...
    }
}
```

**Test:** Run `resolve_module_path(root, ["cmd", "get"])`, check that
the query log contains `file_item_tree` for `lib.rs`, `cmd/mod.rs`,
`cmd/get.rs` and does NOT contain `file_item_tree` for `cmd/set.rs`.

---

### Step 10: Integration tests

**Files:** `crates/sage-ir/tests/expand_tests.rs`

```rust
#[test]
fn expand_cmd_get() {
    let db = setup_mini_redis();  // creates Database with NoopTcxDb,
                                  // loads all source files as SourceFile inputs
    let root_module = ...;        // Module::Local for lib.rs
    let module = resolve_module_path(&db, root_module, &["cmd", "get"]).unwrap();

    // Expanded output
    let output = pretty_print_expanded(&db, module);
    expect_file!["./snapshots/expand_cmd_get.txt"].assert_eq(&output);

    // Query log
    let log = db.take_query_log();
    expect_file!["./snapshots/expand_cmd_get_queries.txt"].assert_eq(&log);
}
```

Note: tests using `NoopTcxDb` can only resolve workspace-local names.
For full resolution (extern crates), we need the rustc-backed impl.
The first integration tests will verify module tree walking and
builtin derive expansion on items that don't depend on external type
resolution for their derives.

---

### Step 11: CLI wiring

**Files:** `src/main.rs`

```rust
#[derive(clap::Subcommand)]
enum CargoCmd {
    Sage {
        #[arg(short, long = "package", value_name = "CRATE")]
        p: Vec<String>,
        /// Expand a specific module and print the result.
        #[arg(long, value_name = "PATH")]
        module: Option<String>,
    },
}
```

Inside `after_expansion`: if `--module` is set, construct the salsa
`Database` with a real `TcxDb` impl backed by `TyCtxt`, resolve the
module path, expand derives, pretty-print, and print the query log.

## Open questions

- Proc-macro dylib invocation: reuse rustc's loaded dylibs from
  `after_expansion`, or load ourselves? (plumbing in place, actual
  call is next after builtins work)
- Exact `salsa_event` filtering — which event kinds to log, how to
  format key arguments for stable snapshots

## Implementation status

Steps 1–10 and Steps A–C are implemented. The full pipeline is working:
module tree, use desugaring, name resolution (including std prelude),
derive resolution, builtin expansion, query logging, and real TcxDb
backed by `TyCtxt`.

### Completed

- **Steps 1–10**: sage-ir crate with module tree, use desugaring,
  name resolution, derive expansion, builtin expansion, query logging.
  Integration tests verify demand-driven module resolution on mini-redis
  with `NoopTcxDb`.

- **Step A**: Database lifetime refactor. Salsa requires `Database: 'static`
  (via `Any` bound), so `Database<'tcx>` is not possible. Instead, used
  unsafe lifetime erasure via `Database::with_tcx()`. Safety enforced by
  the `run_sage_with` callback pattern (Database dropped before `'tcx`
  ends).

- **Step B**: `TcxDb::module_children` returns `Vec<(Name, Symbol, Namespace)>`.
  Added `Send + Sync` supertrait bounds to `TcxDb`.

- **Step C**: `RustcTcxDb<'tcx>` in `src/tcx_impl.rs`. Implements
  `extern_crate`, `module_children`, `is_builtin_derive`. Uses
  `MacroKinds` bitflags (not `MacroKind` enum) for `DefKind::Macro`
  matching. `DefKind::Const` and `AssocConst` are struct variants in
  nightly-2026-03-15.

- **Driver**: `run_sage_with(project_dir, packages, |ctx| { ... })` in
  `src/driver.rs`. Handles full pipeline: load workspace, build deps,
  run_compiler, create Database with RustcTcxDb, construct root Module.
  CLI and tests both use it.

- **Integration tests**: 5 tests in `tests/expand_tests.rs` using real
  TcxDb pointed at `test-fixtures/mini-redis`.

### Deviations from plan

1. **Database lifetime**: Plan said `Database<'tcx>` with no erasure.
   Reality: salsa's `Any` bound requires `'static`. Used unsafe
   `transmute` to erase the lifetime, with safety enforced by the
   closure-scoped borrow in `run_sage_with`.

2. **`is_builtin_derive`**: Plan said `has_attr(sym::rustc_builtin_macro)`.
   Reality: in nightly-2026-03-15, `rustc_builtin_macro` is a parsed
   attribute (`AttributeKind::RustcBuiltinMacro`), not a raw symbol.
   Used `find_attr!(RustcBuiltinMacro { .. })` instead.

3. **Std prelude resolution**: Plan said "resolve through the std prelude
   like any other name". Reality: needed namespace-aware filtering because
   `Debug` exists in both type namespace (the trait) and macro namespace
   (the derive macro). Added `resolve_in_std_prelude(db, name, ns)` that
   walks `std::prelude::v1` and filters by namespace.

4. **Derive expansion tests**: Plan said snapshot expanded impls. Reality:
   `expand_builtin` creates tracked structs (`Path`, `ImplItem`, etc.)
   outside tracked functions, which panics in salsa. Integration tests
   verify derive resolution (name → symbol → is_builtin_derive) without
   full expansion. Full expansion needs `expand_derives` to be a tracked
   function.

5. **Macro sub-namespaces**: Plan said `Namespace::Macro(MacroKind)` with
   separate sub-namespaces. Reality: kept `Namespace::Macro` as a single
   variant for simplicity. The namespace filtering in `resolve_in_std_prelude`
   and `module_children` is sufficient to disambiguate.

### Known gaps

- `expand_builtin` creates tracked structs outside tracked functions.
  Needs to be wrapped in a tracked function for full derive expansion
  to work end-to-end.
- Macro sub-namespaces (bang vs attr vs derive) are collapsed into a
  single `Namespace::Macro`. Works for mini-redis but may need
  refinement for crates with name collisions.
- `macro_rules!` scoping is module-scoped, not textual.
- No workspace glob imports.
- No `#[path = "..."]` attributes on modules.
- Derive helper attributes not resolved.

## Next: Real TcxDb implementation

### Step A: Refactor `Database` to carry `'tcx` lifetime

**Files:** `crates/sage-ir/src/db.rs`, `crates/sage-ir/src/tcx.rs`,
all files that construct or reference `Database`

The current `Database` uses `Arc<dyn TcxDb + Send + Sync>` which
requires `'static`. `RustcTcxDb<'tcx>` holds `TyCtxt<'tcx>` (arena-
allocated, can't outlive the `after_expansion` callback), so `Database`
must carry the lifetime.

Change:
- `Database` → `Database<'tcx>`
- `TcxDb` trait object storage: `Arc<dyn TcxDb + Send + Sync>` →
  `Box<dyn TcxDb + 'tcx>` (no need for `Arc`/`Send`/`Sync` — single-
  threaded within the callback)
- Update `Db` trait impl, all test helpers that create `Database`
  (they pass `NoopTcxDb` which is `'static`, so `Database<'static>`
  works fine for tests)

This is a mechanical refactor — no behavioral changes, just lifetime
propagation. All existing tests should pass unchanged.

---

### Step B: Update `TcxDb` trait to return `Namespace`

**Files:** `crates/sage-ir/src/tcx.rs`, `crates/sage-ir/src/db.rs`

Update `module_children` signature from `Vec<(Name, Symbol)>` to
`Vec<(Name, Symbol, Namespace)>`. Update `NoopTcxDb` accordingly.

---

### Step C: Build `RustcTcxDb`

### Goal

Build the `TcxDb` impl in the `sage` binary crate (`src/`) so that
derive expansion tests produce actual output. This unblocks the
expand-and-snapshot tests from the original plan.

### Where it lives

`src/tcx_impl.rs` — implements `sage_ir::tcx::TcxDb` backed by
`TyCtxt<'tcx>`. Lives in the binary crate because it depends on
rustc internals (behind `rustc_private`).

### What it needs to do

```rust
struct RustcTcxDb<'tcx> {
    tcx: TyCtxt<'tcx>,
}

impl TcxDb for RustcTcxDb<'_> {
    fn extern_crate(&self, name: &str) -> Option<CrateNum> {
        // Walk tcx.crates(()) to find a crate whose name matches.
        // Map rustc's CrateNum to our CrateNum(u32) via .as_u32().
    }

    fn module_children(&self, db, crate_num, def_index)
        -> Vec<(Name, Symbol, Namespace)> {
        assert!(crate_num.0 != 0, "TcxDb must not be called with LOCAL_CRATE");
        // Use tcx.module_children(DefId { krate, index })
        // For each child:
        //   child.res.opt_def_id() — if None, skip (primitives etc.)
        //   Filter to pub visibility only.
        //   Re-exports are transparent: use the resolved DefId directly.
        //   Items in multiple namespaces (e.g. tuple structs) emit
        //   multiple entries with distinct Symbols.
        //   Map each child's DefId → sage Symbol::External.
        //   Map each child's name → sage Name.
        //   Derive namespace from DefKind.
    }

    fn is_builtin_derive(&self, crate_num, def_index) -> bool {
        assert!(crate_num.0 != 0, "TcxDb must not be called with LOCAL_CRATE");
        let def_id = DefId { krate: crate_num.into(), index: def_index.into() };
        matches!(self.tcx.def_kind(def_id), DefKind::Macro(MacroKind::Derive))
            && self.tcx.has_attr(def_id, sym::rustc_builtin_macro)
    }
}
```

### How it gets wired in

The core entry point is `run_sage_with`, which does all setup and
hands a live context to a callback:

```rust
// src/driver.rs (or src/lib.rs)

/// Everything needed to query sage inside after_expansion.
pub struct SageContext<'db> {
    pub db: &'db Database<'tcx>,
    pub root: Module<'db>,
    // ...
}

/// Set up the full sage pipeline for a project and call f with
/// a live SageContext. Handles: load_workspace, build rustc args,
/// run_compiler, create RustcTcxDb + Database + root Module.
pub fn run_sage_with<F, R>(project_dir: &Path, f: F) -> R
where
    F: FnOnce(&SageContext) -> R,
{
    let ws = metadata::load_workspace(project_dir, &[]);
    // build args, run_compiler, inside after_expansion:
    //   create RustcTcxDb { tcx }, Database::new(tcx_db)
    //   construct root Module from workspace source files
    //   call f(&sage_ctx)
}
```

CLI commands are thin wrappers:

```rust
// src/main.rs

fn main() {
    let args = Cargo::parse();
    run_sage_with(&cwd, |sage| {
        // dispatch on subcommand: check, expand --module, etc.
    });
}
```

Tests call `run_sage_with` directly:

```rust
// tests/expand_tests.rs

#[test]
fn expand_cmd_get() {
    run_sage_with(Path::new("test-fixtures/mini-redis"), |sage| {
        let module = sage.resolve_module_path(&["cmd", "get"]).unwrap();
        let output = sage.pretty_print_expanded(module);
        expect_file!["./snapshots/expand_cmd_get.txt"].assert_eq(&output);
    });
}
```

### Integration test plan

Tests live as normal integration tests in the `sage` binary crate
(`tests/expand_tests.rs`). Each `#[test]` calls `run_sage_with`
pointed at `test-fixtures/mini-redis`, which does the full setup
(load workspace, build deps, run_compiler, create Database with
real RustcTcxDb) and hands a `SageContext` to the test closure.

Prerequisite: mini-redis deps must already be built (`cargo check`
with the pinned nightly). `load_workspace` handles this automatically
via `cargo build --message-format=json`.

```rust
#[test]
fn expand_cmd_get() {
    run_sage_with(Path::new("test-fixtures/mini-redis"), |sage| {
        let module = sage.resolve_module_path(&["cmd", "get"]).unwrap();
        let output = sage.pretty_print_expanded(module);
        expect_file!["./snapshots/expand_cmd_get.txt"].assert_eq(&output);
        let log = sage.take_query_log();
        expect_file!["./snapshots/expand_cmd_get_queries.txt"].assert_eq(&log);
    });
}
```

Test cases:

1. **`expand_cmd_get`** — resolve `cmd::get`, expand `#[derive(Debug)]`
   on `Get`. Snapshot the expanded impl and the query log. Verify
   `file_item_tree` only fires for `lib.rs`, `cmd/mod.rs`, `cmd/get.rs`.

2. **`expand_cmd_set`** — `Set` has `#[derive(Debug)]` too. Verify
   independent expansion (no cross-module queries from step 1).

3. **`expand_bin_cli`** — `cli.rs` uses `#[derive(Parser, Subcommand)]`
   from clap. These should resolve as `DeriveResult::ProcMacro`.
   Verify the symbol points at clap's proc-macro DefId.

4. **`expand_no_derives`** — a module with no derives (e.g. `parse.rs`).
   Verify `expand_derives` returns empty and no macro-namespace
   resolution queries fire.

### Key rustc APIs to use

- `tcx.crates(())` — iterator over all loaded crates
- `tcx.crate_name(crate_num)` — get crate name as `Symbol`
- `tcx.module_children(def_id)` — children of a module
- `ModChild { ident, res, vis, .. }` — each child entry
- `child.res.opt_def_id()` — get DefId, skip None (primitives etc.)
- `tcx.def_kind(def_id)` — get DefKind for namespace derivation
- `Res::Def(DefKind::Macro(MacroKind::Derive), def_id)` — derive macros
- `tcx.has_attr(def_id, sym::rustc_builtin_macro)` — builtin check

### Design decisions log (TcxDb implementation)

Decided during design review 2026-04-30, updated during implementation:

1. **Lifetime**: Database stays `'static` (salsa `Any` bound). Unsafe
   lifetime erasure via `Database::with_tcx()`. Safety enforced by
   `run_sage_with` callback pattern.
2. **Re-exports**: transparent — `opt_def_id()` gives resolved DefId,
   no re-export chain tracking
3. **`is_builtin_derive`**: checks `DefKind::Macro(DERIVE)` AND
   `find_attr!(RustcBuiltinMacro { .. })`. The `has_attr` approach
   doesn't work because attrs are parsed in nightly-2026-03-15.
4. **`LOCAL_CRATE` guard**: assert in `module_children` and
   `is_builtin_derive` — hitting it means a bug in resolution
5. **Integration tests**: normal `cargo test` with `run_compiler()`
   inside each `#[test]`
6. **Test args**: reuse `load_workspace()` pointed at
   `test-fixtures/mini-redis`, shared `build_rustc_args()` helper
7. **Visibility**: `module_children` returns only `pub` items
8. **Namespace in return**: `(Name, Symbol, Namespace)` triples
9. **Multi-namespace items**: multiple entries with distinct Symbols
   (e.g. tuple struct → one Type entry, one Value entry)
10. **Macro namespace**: single `Namespace::Macro` (no sub-namespaces
    for bang/attr/derive yet). Sufficient for current use cases.
11. **`macro_rules!`**: registered in module scope for name resolution
    (`Macro`), expansion not implemented (errors)
12. **`macro_rules!` scoping**: treated as module-scoped, not textual
    (known gap)
13. **Entry point**: `run_sage_with(project_dir, packages, |sage| { ... })`
    is the core — does all setup, hands a `SageContext` to the
    callback. CLI commands and tests both use it.
14. **Send+Sync on TcxDb**: `TcxDb: Send + Sync` supertrait. `RustcTcxDb`
    uses unsafe `Send + Sync` impls because `TyCtxt` is `!Send` but
    we only use single-threaded within `after_expansion`.
15. **Std prelude**: `resolve_in_std_prelude(db, name, ns)` walks
    `std::prelude::v1` via TcxDb with namespace filtering.
