// Source: crates/sage-inspector/Cargo.toml
crate sage_inspector {
    """Persistent inspection actor, protocol, HTTP service, and browser assets. The host supplies semantic computation through InspectionProvider.
    Dependencies below describe workspace crates, including development dependencies; third-party libraries remain outside this model.
    """
    depends $sage_reflect;
    module inspection;
}
