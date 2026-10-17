// Source: crates/sage-test-harness/Cargo.toml
crate sage_test_harness {
    """Construct source-backed test crates, demand semantic products, record diagnostics, snapshots, and incremental query evidence.
    The [Inline Compilation Tests](compile-tests.md) specification adds
    ordinary Cargo tests with inline Rust fixtures, source-line type and symbol
    observations, exhaustive diagnostic markers checked independently against Sage
    and its pinned rustc, and optional Sage query snapshots.
    It complements the callback-based helpers using the shared production host.

    Dependencies below describe workspace crates, including development dependencies; third-party libraries remain outside this model.
    """
    depends $sage_host;
    depends $sage_ir;
    depends $sage_stash;
}
