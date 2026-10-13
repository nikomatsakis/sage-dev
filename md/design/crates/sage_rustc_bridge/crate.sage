// Source: crates/sage-rustc-bridge/Cargo.toml
crate sage_rustc_bridge {
    """Own the rustc session on its service thread and provide owned external definition facts through the Tcx database boundary. Does not solve Sage-local goals.
    Dependencies below describe workspace crates, including development dependencies; third-party libraries remain outside this model.
    """
    depends $sage_ir;
    module provider;
}
