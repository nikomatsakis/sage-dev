// Source: Cargo.toml
crate sage {
    """Command-line compiler host and persistent analysis database; loads sources and external metadata and serves semantic inspection.
    Dependencies below describe workspace crates, including development dependencies; third-party libraries remain outside this model.
    """
    depends $sage_ir;
    depends $sage_inspector;
    depends $sage_reflect;
    depends $sage_rustc_bridge;
    depends $sage_stash;
    depends $sage_test_harness;
    module driver;
}
