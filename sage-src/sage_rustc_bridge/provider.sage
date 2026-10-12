module crate::provider;

// Sources: crates/sage-rustc-bridge/src/.
#[rust(crate::RustcTcxDb)]
context RustcTcxDb {
    """Owned metadata adapter backed by a rustc service session."""
    fn request() -> $sage_ir::types::Metadata {
        """Send a keyed metadata request to the rustc thread, extract the requested external fact, and return an owned response. Session-bound rustc handles do not escape and Sage goals are not delegated."""
        envelope {
        }
    }
}

