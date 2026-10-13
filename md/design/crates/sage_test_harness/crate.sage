// Source: crates/sage-test-harness/Cargo.toml
crate sage_test_harness {
    """Construct source-backed test crates, demand semantic products, record diagnostics, snapshots, and incremental query evidence.
    Dependencies below describe workspace crates, including development dependencies; third-party libraries remain outside this model.
    """
    depends $sage_ir;
    depends $sage_stash;
}
