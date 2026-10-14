// Source: crates/sage-host/Cargo.toml
crate sage_host {
    """Shared production workspace host. The CLI, Inspector, and inline test runner use the same Cargo loading, pinned rustc metadata provider, and persistent Sage database.
    The trace adapter projects recorded work using the Inspector's trace model.
    """
    depends $sage_ir;
    depends $sage_stash;
    depends $sage_rustc_bridge;
    depends $sage_inspector;
    module driver;
}
