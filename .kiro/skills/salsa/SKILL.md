---
name: salsa
description: "Guide to salsa 0.26 incremental computation framework. Use when designing or implementing Salsa-based databases, tracked structs, interned types, tracked functions, or accumulators. Covers the full API: #[salsa::input], #[salsa::tracked], #[salsa::interned], #[salsa::db], #[salsa::accumulator], field tracking, the Update trait, lifecycle of tracked structs, and performance tuning. Use whenever the user is working with salsa or designing an incremental IR."
---

# Salsa 0.26 — Incremental Computation Framework

Version: 0.26.1. Used by rust-analyzer. Crate: `salsa` on crates.io.

## Core concept

Salsa memoizes function results and tracks dependencies. When inputs change,
only functions whose dependencies changed are re-executed. Everything else
returns cached results.

```
Input changes → Salsa checks which tracked functions depend on it
             → Re-executes only those → Compares results
             → If result unchanged, downstream stays cached
```

## Database

Every Salsa program needs a database struct. It stores all inputs, tracked
structs, interned values, and memoized results.

```rust
#[salsa::db]
#[derive(Clone, Default)]
pub struct Database {
    storage: salsa::Storage<Self>,
}

#[salsa::db]
impl salsa::Database for Database {}
```

All Salsa structs are just newtyped integer IDs (`salsa::Id`). The actual data
lives in the database. You need `&db` to read fields, `&mut db` to modify inputs.

## Input structs — `#[salsa::input]`

Root mutable data. The starting point of all computation.

```rust
#[salsa::input]
struct SourceFile {
    #[returns(ref)]
    path: String,
    #[returns(ref)]
    contents: String,
}
```

- **Create:** `SourceFile::new(&db, path, contents)` — takes `&db` (not `&mut`)
- **Read:** `file.path(db)` returns `&String` (because `#[returns(ref)]`)
- **Write:** `file.set_contents(&mut db).to(new_value)` — requires `&mut db`, bumps revision
- **Durability:** `file.set_contents(&mut db).with_durability(Durability::LOW).to(value)`

Inputs are the ONLY things that can be mutated. Everything else is derived.

## Tracked functions — `#[salsa::tracked]`

Memoized pure functions. The core of incremental computation.

```rust
#[salsa::tracked]
fn parse(db: &dyn salsa::Database, file: SourceFile) -> ItemTree<'_> {
    let contents = file.contents(db);  // creates dependency on file.contents
    // ... parse ...
    ItemTree::new(db, items)
}
```

**Rules:**
- First arg must be `&dyn SomeDatabase` (read-only — cannot mutate inputs)
- Second arg should be a Salsa struct (input, tracked, or interned)
- Additional args are allowed but discouraged (less efficient caching)
- Return type must implement `Clone` (or use `#[returns(ref)]`)
- Must be deterministic — same inputs → same output

**Options:**
- `#[salsa::tracked(returns(ref))]` — return `&T` from cache instead of cloning
- `#[salsa::tracked(lru = 128)]` — LRU cache eviction
- `#[salsa::tracked(specify)]` — allow externally specifying the result for a key

**Re-execution logic:** When a dependency changes, Salsa re-executes the function.
If the new result equals the old result (via `Update` trait / `PartialEq`), downstream
functions that depend on this result are NOT re-executed. This is "backdating."

## Tracked structs — `#[salsa::tracked]`

Intermediate computed data created inside tracked functions.

```rust
#[salsa::tracked]
struct ItemTree<'db> {
    #[returns(ref)]
    items: Vec<Item>,
}
```

- **Create:** `ItemTree::new(db, items)` — only inside tracked functions
- **Read:** `tree.items(db)` — returns `&Vec<Item>` (because `#[returns(ref)]`)
- **Immutable** — no setters. Values are fixed for the revision.
- **Lifetime:** `'db` ties the struct to the database lifetime

### Field tracking with `#[tracked]`

By default, all fields of a tracked struct are compared as a unit when checking
for changes. You can opt individual fields into independent tracking:

```rust
#[salsa::tracked]
struct Function<'db> {
    #[id]
    name: Name<'db>,

    #[tracked]
    #[returns(ref)]
    signature: Signature,

    #[tracked]
    #[returns(ref)]
    body: Body,
}
```

With `#[tracked]` on individual fields:
- Each field is independently monitored for changes
- If only `body` changes but `signature` stays the same, functions that only
  read `signature` are NOT re-executed
- This is the **incremental firewall** — editing a function body doesn't
  invalidate callers that only depend on the signature

Without `#[tracked]`, all fields are compared together — any field change
invalidates all readers.

### `#[id]` fields for stable matching

When a tracked function is re-executed, tracked structs from the new execution
are matched against those from the old execution. By default, matching is by
creation order (first created → first matched).

`#[id]` fields override this — structs are matched by their `#[id]` field values:

```rust
#[salsa::tracked]
struct Item<'db> {
    #[id]
    name: Name<'db>,  // matched by name across revisions
    #[tracked]
    #[returns(ref)]
    kind: ItemKind,
}
```

If items are reordered in the source, `#[id]` ensures the old `foo` matches the
new `foo` (not whatever happens to be created first). This avoids spurious
invalidation.

**Use `#[id]` when:** the tracked struct represents a named entity (function,
struct, module) that can be reordered without semantic change.

### Lifecycle across revisions

1. Tracked function re-executes
2. New tracked structs are created and matched against old ones (by order or `#[id]`)
3. Fields are compared — unchanged fields don't invalidate downstream
4. Old tracked structs with no matching new struct are **deleted**
5. Deletion cascades: any tracked function that took the deleted struct as input
   has its memoized result discarded

## Interned structs — `#[salsa::interned]`

Canonical deduplicated values. Same fields → same ID. Fast equality (integer comparison).

```rust
#[salsa::interned]
struct Name<'db> {
    #[returns(ref)]
    text: String,
}
```

- **Create:** `Name::new(db, "foo".to_string())` — returns existing ID if same value exists
- **Equality:** `name1 == name2` is just integer comparison (O(1))
- **Read:** `name.text(db)` returns `&String`
- **Use for:** identifiers, paths, type names — anything compared frequently

Interned structs don't need `#[id]` — they ARE their own identity.

## Accumulators — `#[salsa::accumulator]`

Side-channel data collection (diagnostics, warnings).

```rust
#[salsa::accumulator]
pub struct Diagnostic(String);
```

**Push during tracked function:**
```rust
#[salsa::tracked]
fn check(db: &dyn Db, item: Item) {
    // ...
    Diagnostic("type mismatch".to_string()).accumulate(db);
}
```

**Collect from outside:**
```rust
let diagnostics: Vec<Diagnostic> = check::accumulated::<Diagnostic>(db, item);
```

Accumulators are re-collected when the tracked function re-executes. They
participate in Salsa's incremental tracking.

## The `Update` trait

Salsa uses the `Update` trait to determine if a value has changed. For most
types, this is `PartialEq`. For types containing Salsa IDs, derive it:

```rust
#[derive(Clone, Debug, PartialEq, Eq, Hash, salsa::Update)]
pub struct Signature {
    pub params: Vec<TypeRef>,
    pub return_type: Option<TypeRef>,
}
```

**Important:** Types stored in tracked struct fields or returned from tracked
functions must implement `Update`. Use `#[derive(salsa::Update)]` for types
that contain Salsa struct IDs (which have a `'db` lifetime).

## Common patterns

### The ItemTree pattern (incremental firewall)

```rust
#[salsa::input]
struct SourceFile { #[returns(ref)] text: String }

#[salsa::tracked]
struct ItemTree<'db> {
    #[returns(ref)]
    items: Vec<ItemData<'db>>,
}

#[salsa::tracked]
struct FunctionItem<'db> {
    #[id] name: Name<'db>,
    #[tracked] #[returns(ref)] params: Vec<Param<'db>>,
    #[tracked] #[returns(ref)] return_type: Option<TypeRef<'db>>,
    // NO body — parsed on demand in a separate query
}

#[salsa::tracked]
fn file_item_tree(db: &dyn Db, file: SourceFile) -> ItemTree<'_> { ... }

#[salsa::tracked]
fn function_body(db: &dyn Db, func: FunctionItem<'_>) -> Body<'_> { ... }
```

Editing a function body → `function_body` re-executes → but `FunctionItem`'s
`params` and `return_type` haven't changed → callers of those fields are cached.

### Specify pattern (for external/builtin items)

```rust
#[salsa::tracked(specify)]
fn item_type(db: &dyn Db, item: Item<'_>) -> Type<'_> {
    // default: compute from source
    ...
}

// For builtins:
let item = Item::new(db, builtin_name);
item_type::specify(db, item, known_type);
```

### Database trait pattern (for extensibility)

```rust
pub trait Db: salsa::Database {
    // custom methods if needed
}

#[salsa::db]
impl Db for MyDatabase {}
```

Tracked functions take `&dyn Db` so different database implementations can be used.

## Performance notes

- **Interned structs** are cheap to compare (integer equality) — use them for
  names, paths, anything compared frequently
- **`#[returns(ref)]`** avoids cloning — use for Vec, String, any large value
- **`#[tracked]` fields** enable fine-grained invalidation — use on fields that
  change independently (e.g., signature vs body)
- **`#[id]` fields** prevent spurious invalidation from reordering — use on
  named entities
- **LRU** (`lru = N`) prevents unbounded memory in long-running processes
- **Durability** hints (`Durability::HIGH`) help Salsa skip checking stable data
- **Don't intern solver types** (Ty, GenericArgs, etc.) — they churn too much.
  Use arena allocation for those. (Lesson from rust-analyzer PR #21295/#21307)

## Gotchas

- Tracked structs can ONLY be created inside tracked functions. Creating one
  outside panics at runtime.
- `&mut db` is needed to set inputs. Tracked functions only get `&db`.
- Tracked struct fields with `'db` lifetime types must derive `salsa::Update`.
- The `inventory` feature (default) enables automatic registration. Without it,
  you need manual registration via `salsa::plumbing`.
- Salsa structs are `Copy` (they're just integer IDs). Don't store large data
  in them directly — store it in the database via fields.
