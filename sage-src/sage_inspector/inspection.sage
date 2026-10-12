module crate::inspection;

// Sources: actor.rs, protocol.rs, server.rs.
#[rust(none, """Groups protocol Response<T>, errors, and event payloads.""")]
type Response """Revision-tagged response, typed error, reflected value, or query trace.""";
#[rust(none, """Groups actor and HTTP server operations; no single service type.""")]
context InspectionService {
    """Service boundary over an injected InspectionProvider."""
    fn request() -> crate::inspection::Response {
        """Serialize requests through the persistent actor, apply file updates, dispatch the typed provider demand, and return revision-tagged results over HTTP. The host adapter owns compilation; this crate has no sage-ir dependency."""
        envelope {
        }
    }
}

