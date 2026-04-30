---
name: salsa
description: "Guide to salsa 0.26 incremental computation framework. Use when designing or implementing Salsa-based databases, tracked structs, interned types, tracked functions, or accumulators. Covers the full API: #[salsa::input], #[salsa::tracked], #[salsa::interned], #[salsa::db], #[salsa::accumulator], field tracking, the Update trait, lifecycle of tracked structs, and performance tuning. Use whenever the user is working with salsa or designing an incremental IR."
---

# Salsa 0.26 — Incremental Computation

## Database Setup

**DO:** Always start with this pattern
```rust
#[salsa::db]
#[derive(Clone, Default)]
pub struct Database {
    storage: salsa::Storage<Self>,
}

#[salsa::db]
impl salsa::Database for Database {}
```

**DON'T:** Forget `Clone` — salsa requires it for snapshots.

## Input Structs — Root Data

**DO:** Use for mutable root data only.
```rust
#[salsa::input]
struct SourceFile {
    #[returns(ref)]  // avoid cloning large values
    contents: String,
}
```

**DON'T:** Try to mutate inputs inside tracked functions — use `&mut db` only outside.

- **Create:** `SourceFile::new(&db, contents)` — takes `&db`, not `&mut`
- **Read:** `file.contents(db)` — returns `&String` with `#[returns(ref)]`
- **Write:** `file.set_contents(&mut db).to(new_value)` — requires `&mut db`

## Tracked Functions — Memoized Computation

**DO:** Follow this signature pattern.
```rust
#[salsa::tracked]
fn parse(db: &dyn Database, file: SourceFile) -> ItemTree<'_> {
    let text = file.contents(db);  // creates dependency on file.contents
    ItemTree::new(db, parse_items(text))
}
```

**DON'T:**
- Take `&mut db` — tracked functions are read-only
- Add extra parameters beyond the salsa struct — hurts caching
- Return non-Clone types without `#[returns(ref)]`

**Options:**
- `#[salsa::tracked(returns(ref))]` — return `&T` instead of cloning
- `#[salsa::tracked(lru = 128)]` — LRU cache eviction for long-running processes
- `#[salsa::tracked(specify)]` — allow external specification for builtins

## Tracked Methods — Methods on Salsa Structs

**DO:** Use `#[salsa::tracked]` on both the impl block and the method.
```rust
// On a salsa::input (no 'db lifetime) — elide the db lifetime:
#[salsa::tracked]
impl MyInput {
    #[salsa::tracked]
    fn tracked_fn(self, db: &dyn Db) -> u32 {
        self.field(db) * 2
    }
}

// On a tracked/interned struct (has 'db) — tie db lifetime to 'db:
#[salsa::tracked]
impl<'db> Symbol<'db> {
    #[salsa::tracked]
    pub fn sig(self, db: &'db dyn Db) -> FnSig<'db> {
        resolve_sig(db, self)
    }
}
```

Also works on trait impls:
```rust
#[salsa::tracked]
impl<'db> MyTrait<'db> for MyStruct<'db> {
    #[salsa::tracked]
    fn my_method(db: &'db dyn Db, input: MyInput) -> Self::Output {
        // ...
    }
}
```

**DON'T:** Forget `#[salsa::tracked]` on the impl block itself — both the impl and the method need the annotation.

## Tracked Structs — Intermediate Data

**DO:** Create inside tracked functions only.
```rust
#[salsa::tracked]
struct Function<'db> {
    #[id]       // stable identity across revisions
    name: Name<'db>,

    #[tracked]  // independent change tracking
    #[returns(ref)]
    signature: Signature,

    #[tracked]
    #[returns(ref)]
    body: Body,
}
```

**DON'T:** Create tracked structs outside tracked functions — runtime panic.

**Field annotations:**
- **`#[tracked]`:** Each field tracked independently. Signature change doesn't invalidate body readers. This is the **incremental firewall**.
- **Without `#[tracked]`:** All fields compared as a unit — any change invalidates all readers.
- **`#[id]`:** Match structs by this field across revisions instead of creation order. Use for named entities (functions, structs, modules) that can be reordered.
- **`#[returns(ref)]`:** Return `&T` from getter instead of cloning.

## Interned Structs — Fast Equality

**DO:** Use for frequently compared values.
```rust
#[salsa::interned]
struct Name<'db> {
    #[returns(ref)]
    text: String,
}
```

Same content → same ID. Equality is O(1) integer comparison.
Use for: identifiers, paths, type names.

## Accumulators — Side Channel Data

**DO:** Use for diagnostics and warnings.
```rust
#[salsa::accumulator]
struct Diagnostic(String);

// Push inside tracked function:
Diagnostic("type error".to_string()).accumulate(db);

// Collect later:
let errors = check::accumulated::<Diagnostic>(db, item);
```

## The Update Trait

**DO:** Derive `salsa::Update` for types containing salsa IDs (`'db` lifetime).
```rust
#[derive(Clone, Debug, PartialEq, Eq, salsa::Update)]
struct Signature<'db> {
    params: Vec<TypeRef<'db>>,
    return_type: Option<TypeRef<'db>>,
}
```

**DON'T:** Forget `salsa::Update` on types stored in tracked struct fields — compilation error.

## Common Gotchas

**Runtime panics:**
- Creating tracked structs outside tracked functions
- Accessing tracked structs deleted in a previous revision

**Compilation errors:**
- Missing `salsa::Update` on field types with `'db` lifetime
- Missing `Clone` on tracked function return types (unless `returns(ref)`)

**Subtle bugs:**
- Not using `#[id]` on named entities → reordering causes spurious recomputation
- Not using `#[tracked]` on independent fields → body edits invalidate signature readers

## Incremental Firewall Pattern

Separate item skeletons from bodies so body edits don't cascade:

```rust
#[salsa::tracked]
fn file_items(db: &dyn Db, file: SourceFile) -> ItemTree<'_> { ... }

#[salsa::tracked]
struct Function<'db> {
    #[id] name: Name<'db>,
    #[tracked] #[returns(ref)] signature: Signature<'db>,
    // NO body — parsed separately
}

#[salsa::tracked]
fn function_body(db: &dyn Db, func: Function<'_>) -> Body<'_> { ... }
```

Editing a function body → `function_body` re-executes → `Function`'s signature unchanged → callers of signature stay cached.

## Lifecycle Across Revisions

1. Tracked function re-executes
2. New tracked structs matched against old (by `#[id]` or creation order)
3. Field values compared — unchanged `#[tracked]` fields don't invalidate downstream
4. Unmatched old structs are **deleted** — dependent cached results discarded

## Performance Tips

- **Intern** frequently compared values (names, paths)
- **`#[returns(ref)]`** for Vec, String, any large value
- **`#[tracked]` fields** when changes are independent (signature vs body)
- **`#[id]` fields** for named entities to prevent reorder invalidation
- **`Durability::HIGH`** for inputs that rarely change (dep metadata)
- **Don't intern solver types** that churn (Ty, GenericArgs) — use arena allocation
