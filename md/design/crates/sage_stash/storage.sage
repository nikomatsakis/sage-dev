module crate::storage;

// Source: crates/sage-stash/src/.
type Stored """Owned immutable typed tree or pointer within a stash.""";
context Stash {
    """Append-only storage and hash-consing boundary.

{anchor}`handle_ownership`
Handles never cross stash ownership implicitly. A `Ptr<T>` or
`Slice<T>` is interpreted only by its allocating `Stash`. Retaining a value
across a stash boundary requires a structural copy into the destination; a
checked output tree must not contain handles back into its source CST or an
imported query result.

{anchor}`typed_interning`
Allocation is content-addressed within one stash. Every value
and slice allocation is hash-consed by its typed structural content. Equal
content of the same stash data type yields the same handle, while hash
collisions are resolved by actual content comparison rather than sharing.

{anchor}`immutable_entries`
Allocated entries are immutable. `Stash` is append-only:
`alloc` and `alloc_slice` may add entries, but safe code cannot mutate a
`Ptr<T>` or `Slice<T>` allocation after it has been interned. Transient
inference state is finalized by resolving and copying the completed value
into a fresh output stash.

{anchor}`rooted_equality`
Query-result equality follows rooted semantic content. The
equality, ordering, hashing, and Salsa update behavior of `Stashed<T>` use a
deterministic structural fingerprint of the reachable root, not stash
addresses, allocation order, padding, or unreachable allocations.

{anchor}`query_value`
A stash-owned tree is one incremental value. Salsa compares
and backdates a `Stashed<T>` root as a unit; nodes below that root are not
independent tracked identities. Finer incremental boundaries are introduced
as semantic queries outside the tree, not by exposing its internal handles.
"""
    fn alloc() -> crate::storage::Stored {
        """Allocate typed values/slices with deterministic content hashes; canonicalize equal supported values and retain typed ownership. Copying between stashes preserves semantic content."""
        envelope {
        }
    }
}
