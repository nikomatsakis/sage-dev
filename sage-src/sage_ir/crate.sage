// Source: crates/sage-ir/Cargo.toml
crate sage_ir {
    """Demand-driven semantic frontend. Symbols connect source parsing, expansion, resolution, checked interfaces, inference, trait solving, and completed typed bodies.
    Dependencies below describe workspace crates, including development dependencies; third-party libraries remain outside this model.
    """
    depends $sage_macros_from_impls;
    depends $sage_reflect;
    depends $sage_stash;
    module types;
    module symbols;
    module modules;
    module checking;
    module resolution;
    module inference;
    module solver;
    module metadata;
}
