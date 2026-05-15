# Stash (`sage-stash`)

The **Stash** is a flat, type-erased byte buffer for `Copy`-only
data. It stores function bodies (expressions, statements, patterns)
outside of salsa, avoiding per-node tracked-struct overhead while
still integrating with salsa's incremental invalidation via
byte-level equality.

## Why not salsa tracked structs for body nodes?

A function body can contain hundreds of `Expr`, `Stmt`, and `Pat`
nodes. Making each one a salsa tracked struct would mean hundreds
of salsa IDs per function, with per-field change detection overhead
that isn't needed — body resolution reads the *entire* body, not
individual nodes. The Stash gives O(1) bulk equality: if the bytes
didn't change, the body didn't change.

## Core types

### `Stash`

```rust
pub struct Stash {
    buf: Vec<u8>,
    entries: Vec<Entry>,
    intern_map: HashMap<InternKey, u32>,
}
```

A growable byte buffer with an entry table. Each entry records a
`TypeId`, byte offset, and element count. The `intern_map` is only
used for deduplicated (`intern`) operations.

Two equality semantics: `Stash: PartialEq` compares `buf` bytes
directly, and `Stash: Hash` hashes the `buf`. This is what makes
`Stashed<T>` cheap to compare.

### `Ptr<T>` and `Slice<T>`

```rust
pub struct Ptr<T> { index: u32, .. }
pub struct Slice<T> { index: u32, .. }
```

Thin handles (4 bytes each, `Copy`) that index into the entry
table. `Ptr<T>` points to one value, `Slice<T>` to a contiguous
run. Accessed via `stash[ptr]` / `stash[slice]` (`Index` impls).

Handles are only valid within the `Stash` that created them.
Cross-stash indexing panics at the type-id check.

### `Stashed<T>`

```rust
pub struct Stashed<T> {
    stash: Stash,
    root: T,
}
```

A self-contained bundle: a `Stash` plus a root handle into it.
This is what salsa stores as a tracked field value.

`PartialEq` compares the stash bytes first (O(n) over the buffer),
then the root value. If the bytes match, handle indices are
equivalent so the root comparison is O(1). This gives salsa its
incremental firewall: a tracked field of type `Stashed<Ptr<Body>>`
only invalidates downstream queries when the body's bytes actually
changed.

The type alias `FunctionBody<'db> = Stashed<Ptr<Body<'db>>>` is
the concrete instance used for function bodies.

## Alloc vs Intern

- **`stash.alloc(value)`** — append, return a fresh `Ptr`. Two
  calls with the same value produce distinct handles.
- **`stash.intern(value)`** — deduplicate. Returns the same `Ptr`
  for equal values (`T: Hash + Eq`).
- Same distinction for `alloc_slice` / `intern_slice`.

Body lowering uses `alloc` exclusively (dedup isn't needed for
tree-shaped data). Interning is available for cases where
structural sharing matters.

## Derive macros

- **`#[derive(AllocStashData)]`** — generates `StashData` +
  `AllocStashData` impls. Used on body node types (`Expr`, `Stmt`,
  `Pat`, `MatchArm`, `FieldInit`, etc.).
- **`#[derive(InternStashData)]`** — generates `StashData` +
  `InternStashData` impls. Used when dedup is desired.

Types that are self-contained scalars (no `Ptr`/`Slice` fields)
can implement `StashDirect` for blanket `StashEq`/`StashHash`/
`StashOrd` impls that ignore the stash context.

## Stash-contextual comparison

Handles compare by index (O(1)), but that only works within the
same stash. For cross-stash comparison (e.g. diffing two versions
of a body), the `StashEq`, `StashHash`, and `StashOrd` traits
provide stash-contextual deep comparison:

```rust
pub trait StashEq<'db> {
    fn stash_eq(&self, other: &Self, stash: &Stash) -> bool;
}
```

`Ptr<T>` and `Slice<T>` implement these by quick-checking the
index, then falling through to element-wise deep comparison.

## Salsa integration

`Ptr<T>`, `Slice<T>`, and `Stashed<T>` all implement
`salsa::Update` (behind a `salsa` feature flag). The `Update`
impls use value equality — salsa calls `maybe_update` and gets
told whether the new value differs from the old, enabling
incremental skip.

## Usage pattern

Lowering builds a body like this:

```rust
let mut stash = Stash::new();
let root_expr = lower_expr(&mut stash, node);
let body = stash.alloc(Body { root: root_expr, span });
let function_body: FunctionBody = Stashed::new(stash, body);
```

The `FunctionBody` is stored in `FnAst::body` (a salsa tracked
field). When salsa re-executes lowering and the new stash bytes
match the old ones, `Stashed::eq` returns true and downstream
queries like `resolve_body` are skipped.
