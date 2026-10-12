// Source: crates/sage-emit/Cargo.toml
crate sage_emit {
    """Translate Sage semantic products into the shared rust-ref representation, without repairing semantic mismatches.
    Dependencies below describe workspace crates, including development dependencies; third-party libraries remain outside this model.
    """
    depends $rust_ref;
    depends $sage_ir;
    depends $sage_stash;
    depends $sage_test_harness;
    module emission;
}
