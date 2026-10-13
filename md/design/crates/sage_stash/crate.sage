// Source: crates/sage-stash/Cargo.toml
crate sage_stash {
    """Append-only typed storage, deterministic content hashing, copying, and ownership of syntax and semantic trees across query boundaries.
    Dependencies below describe workspace crates, including development dependencies; third-party libraries remain outside this model.
    """
    depends $sage_stash_macros;
    module storage;
}
