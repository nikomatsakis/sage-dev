// Source: crates/sage-test-harness/Cargo.toml
crate sage_test_harness {
    """Construct source-backed test crates, demand semantic products, record diagnostics, snapshots, and incremental query evidence.
    The draft [Inline Compilation Tests](compile-tests.md) specification adds
    ordinary Cargo tests with inline Rust fixtures, source-line type
    observations, exhaustive diagnostic markers, and optional query snapshots.
    It describes a planned runner, not the current callback-based helpers.

    Dependencies below describe workspace crates, including development dependencies; third-party libraries remain outside this model.
    """
    depends $sage_ir;
    depends $sage_stash;
}
