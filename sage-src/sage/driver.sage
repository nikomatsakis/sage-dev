module crate::driver;

// Sources: src/driver.rs and src/inspector.rs.
#[rust(crate::driver::SageHost)]
context AnalysisHost {
    """Persistent Salsa database, sources, and external metadata provider."""
    fn inspect() -> $sage_ir::types::Body {
        """Load or update source inputs, resolve the selected semantic demand, and compute only the requested product. Body demand is one supported choice; signatures and module outlines stop earlier."""
        envelope {
            invokes $sage_ir::modules::LocalModSym::expanded_items();
            invokes $sage_ir::symbols::FnSymbol::signature();
            invokes $sage_ir::symbols::LocalFnSym::body();
            invokes $sage_reflect::reflection::Reflect::reflect();
        }
    }
}

