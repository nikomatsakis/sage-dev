module crate::metadata;

// Sources: external_syms.rs and tcx/. External provider is injected through Db::tcx.
#[rust(none, """Groups the keyed lowering functions in external_syms.rs.""")]
class ExternalFacts {
    """Keyed lowering boundary over owned Tcx facts. The concrete rustc service lives in sage-rustc-bridge; sage-ir does not depend on that implementation."""
    input provider: crate::types::Metadata;
    query signature() -> crate::types::Signature {
        """Request one definition-keyed signature and lower it into Sage types and generic identities."""
        envelope {
            reads self.provider;
        }
    }
    query items() -> crate::types::Items {
        """Request module or associated children, preserving supported kinds and unavailable metadata."""
        envelope {
            reads self.provider;
        }
    }
    query value() -> crate::types::Value {
        """Request an associated value, field, or declared type fact; never ask rustc to solve a Sage goal."""
        envelope {
            reads self.provider;
        }
    }
    query candidates() -> crate::types::Candidates {
        """Request relevant external impl headers and preserve provider completeness."""
        envelope {
            reads self.provider;
        }
    }
}

