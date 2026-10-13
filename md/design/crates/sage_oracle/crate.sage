// Source: crates/sage-oracle/Cargo.toml
crate sage_oracle {
    """Run rustc and independently emit its semantic results into rust-ref.
    Dependencies below describe workspace crates, including development dependencies; third-party libraries remain outside this model.
    """
    depends $rust_ref;
    module oracle;
}
