module crate::oracle;

// Sources: crates/sage-oracle/src/lib.rs and emit/.
#[rust(none, """Groups analyze_crate and the rustc callbacks; there is no Oracle type.""")]
class Oracle {
    """Independent rustc adapter."""
    fn analyze_crate() -> $rust_ref::Representation {
        """Run rustc callbacks for the input crate, inspect compiler results, and independently emit the shared schema. Report missing output and unsupported shapes explicitly."""
        envelope {
        }
    }
}

