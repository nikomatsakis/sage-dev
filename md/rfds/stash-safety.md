# RFD: Stash safety — cross-stash identity checking and value-based equality

**Status:** Draft (partially begun)

**Depends on:**
- [Module sym tree](./module-sym-tree.md) — stash architecture, `Stashed<T>`

## Goal

Catch cross-stash pointer misuse at runtime and fix `Stashed<T>` equality to compare by value rather than by byte identity. These are two sides of the same coin: today a `Ptr` from stash A can be silently indexed into stash B (reading wrong data), and `Stashed::PartialEq` compares raw byte buffers (a shortcut that breaks as soon as handle metadata is embedded in the buffer).

## Problem

### Silent cross-stash misuse

```rust
let mut stash_a = Stash::new();
let mut stash_b = Stash::new();
let ptr = stash_a.alloc(Point { x: 1, y: 2 });
let _ = stash_b.alloc(Point { x: 99, y: 100 });
stash_b[ptr] // silently reads Point { x: 99, y: 100 } — no panic, no warning
```

This happened in practice during the type-signatures work: `SigLowerCtx` received a `Ty` whose `Slice` args pointed into a caller's stash, stored it directly in the destination stash, and downstream code read wrong data. The fix was to deep-copy via `Identity` folder, but nothing caught the bug — it was found by inspecting test output.

### Byte-level `Stashed` equality is fragile

`Stash::PartialEq` compares `self.buf == other.buf`. This works only because today the byte layout is deterministic for identical allocation sequences. But it would break if:
- Handle metadata (like a stash ID) is embedded in `Ptr`/`Slice`
- Allocation order or padding changes between otherwise-equivalent stashes
- Any non-content bytes end up in the buffer

The existing `StashEq` trait already solves value comparison within a single stash. `Stashed` equality needs the cross-stash version.

## Design

### Part 1: Cross-stash `StashEq` for `Stashed`

Generalize `StashEq` to support two different stashes:

```rust
pub trait CrossStashEq<'db> {
    fn cross_stash_eq(&self, self_stash: &Stash, other: &Self, other_stash: &Stash) -> bool;
}
```

Blanket impl from `StashEq` for leaf types (scalars, `StashDirect` types) that ignore stash context. `Ptr<T>` and `Slice<T>` get implementations that dereference through their respective stashes and compare recursively.

`Stashed::PartialEq` delegates to this:

```rust
impl<T: CrossStashEq<'db>> PartialEq for Stashed<T> {
    fn eq(&self, other: &Self) -> bool {
        self.root.cross_stash_eq(self.stash(), other.root(), other.stash())
    }
}
```

Similarly, `CrossStashHash`:

```rust
pub trait CrossStashHash<'db> {
    fn cross_stash_hash<H: Hasher>(&self, stash: &Stash, state: &mut H);
}
```

This is the same signature as the existing `StashHash` — the hash only needs one stash (the value's own stash). So `Stashed::Hash` can delegate to `StashHash` directly; no new trait needed for hashing.

**Derive macro changes.** The `AllocStashData` derive should also generate `CrossStashEq` impls, following the same field-by-field pattern as the existing `StashEq` derive but threading two stashes. Alternatively, a blanket impl from `StashEq` covers types with no cross-stash fields (leaf types), and `Ptr`/`Slice` get manual impls.

**Migration path.** `Stash::PartialEq` (raw byte comparison) remains as a fast path or is removed entirely. The byte comparison was never semantically correct — it was a coincidence that it worked. The `salsa::Update` impl for `Stashed` also uses `PartialEq` and benefits from the same fix.

### Part 2: Debug-mode stash identity tagging

Each `Stash` gets a unique ID (debug-only, via atomic counter). Each `Ptr<T>` and `Slice<T>` carries the allocating stash's ID as a debug-only field. On `stash[ptr]`, assert that the Ptr's stash ID matches the Stash's ID.

```rust
pub struct Ptr<T> {
    index: u32,
    #[cfg(debug_assertions)]
    stash_id: u32,
    _marker: PhantomData<T>,
}
```

**Why Part 1 must come first.** Embedding `stash_id` in `Ptr`/`Slice` changes their byte representation. Since `Ptr`/`Slice` are stored inside stash-allocated compound types (e.g., `FnSigAstData { params: Slice<ParamAst> }`), the stash byte buffer includes these fields. Two independently-constructed stashes with identical logical content will have different stash IDs, so their byte buffers will differ. The current `Stash::PartialEq` (byte comparison) breaks. Part 1's value-based comparison resolves this — it compares through the `CrossStashEq` trait, which ignores stash IDs.

**Scope of the check.** The assertion fires on `stash[ptr]` and `stash[slice]` — the two `Index` impls. It does not fire on `Ptr::PartialEq` or `Ptr::Hash` (those only compare `index`, as before). The stash ID is a diagnostic, not a semantic property.

**Zero cost in release.** The `stash_id` field, the atomic counter, and the assertion are all `#[cfg(debug_assertions)]`. Release builds are unchanged in layout and performance.

### Part 3: Convenience API for cross-stash copying

Today, consuming a signature from another stash requires manually setting up a `TyFolder`:

```rust
let sig = fn_signature(db, fn_ast, module, source_root);
let stash = sig.stash();
let binder = sig.root();
// caller must know to use Identity folder to copy types into their own stash
```

Add convenience methods on `Stashed<Binder<FnSig>>` etc.:

```rust
impl Stashed<Binder<'db, FnSig<'db>>> {
    /// Copy the signature into `target`, returning a Binder with all
    /// Ptr/Slice references valid in `target`.
    pub fn copy_into(&self, target: &mut Stash) -> Binder<'db, FnSig<'db>>;

    /// Instantiate the binder with concrete type args into `target`.
    pub fn instantiate_into(
        &self,
        target: &mut Stash,
        args: &[Ty<'db>],
    ) -> FnSig<'db>;
}
```

The caller never sees raw `Ptr`/`Slice` values from the source stash — the API encapsulates the cross-stash copy. This makes the identity check (Part 2) a safety net rather than a routine failure mode.

## Current state (partially begun)

The identity tagging (Part 2) was partially implemented but reverted after discovering that it requires Part 1 as a prerequisite. Specifically:

- Adding `stash_id` to `Ptr`/`Slice` broke `Stashed::PartialEq` (byte-level comparison of stash buffers).
- Two independently-built stashes with identical logical content fail equality because the embedded stash IDs differ in the byte buffer.
- The existing test `cross_stash_ptr_same_type_reads_wrong_data` documents the current bad behavior (silently reading wrong data from the wrong stash).

The `StashEq`/`StashHash` traits (single-stash) already exist and are derived for all stash-allocated types. The cross-stash generalization is incremental.

## Implementation plan

### Step 1: `CrossStashEq` trait and `Stashed::PartialEq`

Add `CrossStashEq` trait. Implement for `Ptr<T>`, `Slice<T>`, `Option<T>`, and `StashDirect` types. Update the `AllocStashData` derive macro to generate `CrossStashEq` impls. Change `Stashed::PartialEq` to use `CrossStashEq`. Keep `Stash::PartialEq` (byte comparison) for now as a separate method if needed, but `Stashed` no longer uses it.

Verify all existing tests pass — the new equality should produce the same results for all current uses.

### Step 2: Debug-mode stash identity tagging

Add stash ID to `Stash`, `Ptr`, `Slice` (debug-only). Add assertions to `Index` impls. Update the test `cross_stash_ptr_same_type_reads_wrong_data` to `#[should_panic]`.

### Step 3: Convenience `copy_into` / `instantiate_into` methods

Add methods to `Stashed<Binder<FnSig>>`, `Stashed<Binder<StructSig>>`, etc. Migrate callers from manual `TyFolder` usage.

## Scope and non-goals

**In scope:**
- `CrossStashEq` trait and impls for all stash types
- `Stashed::PartialEq` rewrite
- Debug-mode stash identity tagging on `Ptr`/`Slice`
- Convenience methods for cross-stash signature consumption

**Out of scope:**
- Phantom-lifetime or type-system approaches to prevent cross-stash misuse at compile time (too costly for the benefit)
- Changing `Stash::PartialEq` semantics (it remains byte-level; only `Stashed` changes)
- `CrossStashOrd` (no current use case)

## Open questions

1. **Derive macro scope.** Should the `AllocStashData` macro also generate `CrossStashEq`, or should it be a separate derive? Bundling is simpler; separating avoids bloating the common derive for types that never appear in `Stashed` roots.

2. **`StashHash` for `Stashed`.** The current `Stash::hash` uses byte-level hashing. Should `Stashed::Hash` switch to `StashHash`-based hashing for consistency with the equality change? This would be more expensive but semantically correct. The current byte hash might produce different hashes for logically-equal stashes if the stash ID is embedded in the buffer. This must be fixed if Part 2 is implemented.
