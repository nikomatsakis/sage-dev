module crate::storage;

// Source: crates/sage-stash/src/.
type Stored """Owned immutable typed tree or pointer within a stash.""";
context Stash {
    """Append-only storage and hash-consing boundary."""
    fn alloc() -> crate::storage::Stored {
        """Allocate typed values/slices with deterministic content hashes; canonicalize equal supported values and retain typed ownership. Copying between stashes preserves semantic content."""
        envelope {
        }
    }
}
