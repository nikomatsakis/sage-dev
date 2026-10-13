// Source: crates/sage-reflect/Cargo.toml
crate sage_reflect {
    """Structural reflection of semantic values into bounded inspectable trees, preserving symbol references.
    Dependencies below describe workspace crates, including development dependencies; third-party libraries remain outside this model.
    """
    depends $sage_reflect_derive;
    depends $sage_stash;
    module reflection;
}
