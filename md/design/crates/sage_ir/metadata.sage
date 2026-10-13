module crate::metadata;

// Sources: external_syms.rs and tcx/. External provider is injected through Db::tcx.
class ExternalFacts {
    """
    Keyed lowering boundary over owned Tcx facts. The concrete rustc service lives in sage-rustc-bridge; sage-ir does not depend on that implementation.

    {anchor}`provider_authority`
    **External metadata supplies owned facts, never Sage decisions.** The provider may state facts authored by a reachable dependency or the compiler, but it cannot resolve local syntax, select a Sage candidate, prove a Sage goal, normalize an alias, or type-check a Sage body. This is the typed metadata boundary established by [External providers supply facts, not Sage semantic answers](../../../rfds/architectural-foundations/README.md#external-providers-supply-facts-not-sage-semantic-answers).

    {anchor}`explicit_incompleteness`
    **Missing metadata remains explicit incompleteness.** A missing, unsupported, or conservatively incomplete external fact is never replaced by an inferred default, a successful interface, or exhaustive logical failure. This is the metadata consequence of [Incompleteness is an explicit terminal outcome](../../../rfds/architectural-foundations/README.md#incompleteness-is-an-explicit-terminal-outcome).

    {anchor}`product_boundaries`
    **External demand is decomposed by semantic product.** Identity enumeration, signatures, impl headers, and individual associated values are separate tracked products. Consumers enumerate identities before requesting selected details, and no external function body is available to checking. This is the metadata consequence of [Cross-item dependencies stop at semantic interfaces](../../../rfds/architectural-foundations/README.md#cross-item-dependencies-stop-at-semantic-interfaces).

    {anchor}`keyed_reuse`
    **Raw and lowered metadata share the same narrow reuse key.** Each lowered semantic fact observes only its corresponding owned provider fact. Repeating an unchanged request reuses the lowered value without another raw request, and changing an unrelated provider fact does not invalidate it.
    """
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
