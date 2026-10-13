module crate::inspection;

// Sources: actor.rs, protocol.rs, server.rs.
type Response """Revision-tagged response, typed error, reflected value, or query trace.""";
context InspectionService {
    """Service boundary over an injected InspectionProvider."""
    fn request() -> crate::inspection::Response {
        """Serialize requests through the persistent actor, apply file updates, dispatch the typed provider demand, and return revision-tagged results over HTTP. The host adapter owns compilation; this crate has no sage-ir dependency."""
        envelope {
        }
    }
}
