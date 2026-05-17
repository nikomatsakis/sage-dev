# RFD: Stash safety — cross-stash identity checking and convenience APIs

**Status:** Draft

**Depends on:**
- [Hash-consed stash with fingerprinted equality](./stash-hash-consing.md) — hash-consing, `StashHasher`, `Fingerprint`
- [Module sym tree](./module-sym-tree.md) — stash architecture, `Stashed<T>`

## Goal

Catch cross-stash pointer misuse at runtime and provide safe APIs for consuming stash-allocated data across stash boundaries.

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

### Manual cross-stash copying is error-prone

Today, consuming a signature from another stash requires manually setting up a `TyFolder`:

```rust
let sig = fn_signature(db, fn_ast, module, source_root);
let stash = sig.stash();
let binder = sig.root();
// caller must know to use Identity folder to copy types into their own stash
```

Nothing in the API signals that `Ptr`/`Slice` values from one stash are invalid in another. The caller must know to copy, and must know the right mechanism.

## Design

### Part 1: Debug-mode stash identity tagging

Each `Stash` gets a unique ID (debug-only, via atomic counter). Each `Ptr<T>` and `Slice<T>` carries the allocating stash's ID as a debug-only field. On `stash[ptr]`, assert that the Ptr's stash ID matches the Stash's ID.

```rust
pub struct Ptr<T> {
    index: u32,
    #[cfg(debug_assertions)]
    stash_id: u32,
    _marker: PhantomData<T>,
}
```

**Prerequisite: hash-consing.** Embedding `stash_id` in `Ptr`/`Slice` changes their byte representation in debug mode. With hash-consing (from the [hash-consing RFD](./stash-hash-consing.md)), equality never touches raw bytes — `Ptr` compares by index, compounds compare field-by-field — so the debug-only `stash_id` field is invisible to the equality and hashing machinery.

**Scope of the check.** The assertion fires on `stash[ptr]` and `stash[slice]` — the two `Index` impls. It does not fire on `Ptr::PartialEq` or `Ptr::Hash` (those only compare `index`). The stash ID is a diagnostic, not a semantic property.

**Zero cost in release.** The `stash_id` field, the atomic counter, and the assertion are all `#[cfg(debug_assertions)]`. Release builds are unchanged in layout and performance.

### Part 2: Convenience API for cross-stash copying

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

The caller never sees raw `Ptr`/`Slice` values from the source stash — the API encapsulates the cross-stash copy. This makes the identity check (Part 1) a safety net rather than a routine failure mode.

## Current state

- The identity tagging was partially implemented but reverted after discovering that it requires value-based equality as a prerequisite (now addressed by the [hash-consing RFD](./stash-hash-consing.md)).
- Adding `stash_id` to `Ptr`/`Slice` broke `Stashed::PartialEq` (byte-level comparison of stash buffers). Two independently-built stashes with identical logical content failed equality because the embedded stash IDs differed in the byte buffer.
- The existing test `cross_stash_ptr_same_type_reads_wrong_data` documents the current bad behavior (silently reading wrong data from the wrong stash).

## Implementation plan

### Step 1: Debug-mode stash identity tagging

Add stash ID to `Stash`, `Ptr`, `Slice` (debug-only). Add assertions to `Index` impls. Update the test `cross_stash_ptr_same_type_reads_wrong_data` to `#[should_panic]`.

### Step 2: Convenience `copy_into` / `instantiate_into` methods

Add methods to `Stashed<Binder<FnSig>>`, `Stashed<Binder<StructSig>>`, etc. Migrate callers from manual `TyFolder` usage.

## Scope and non-goals

**In scope:**
- Debug-mode stash identity tagging on `Ptr`/`Slice`
- Convenience methods for cross-stash signature consumption

**Out of scope:**
- Phantom-lifetime or type-system approaches to prevent cross-stash misuse at compile time (too costly for the benefit)
- Hash-consing and fingerprinted equality (covered by [stash-hash-consing](./stash-hash-consing.md))
