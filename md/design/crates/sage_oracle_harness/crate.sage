// Source: crates/sage-oracle-harness/Cargo.toml
crate sage_oracle_harness {
    """Run conformance fixtures through the independent emitters and compare their serialized results exactly.
    Dependencies below describe workspace crates, including development dependencies; third-party libraries remain outside this model.
    """
    depends $rust_ref;
    depends $sage_emit;
    depends $sage_ir;
    depends $sage_oracle;
    depends $sage_rustc_bridge;
    depends $sage_stash;
    depends $sage_test_harness;
}
