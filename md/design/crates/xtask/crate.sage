// Source: xtask/Cargo.toml
crate xtask {
    """Workspace maintenance commands, including development tooling and documentation support.
    Dependencies below describe workspace crates, including development dependencies; third-party libraries remain outside this model.
    """
    depends $sage_meta;
}
