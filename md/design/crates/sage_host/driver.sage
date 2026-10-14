module crate::driver;

context AnalysisHost {
    """Owns workspace source inputs and the external metadata provider. Installs tracked target cfg and Cargo-feature inputs and registers package sources with paths relative to the selected source root. Opening a host does not demand local semantic products or require successful rustc checking of local source."""
    fn analyze() -> $sage_ir::modules::LocalModSym {
        """Establish the selected local crate and give a caller access to its production database and root module. The caller chooses semantic demands."""
        envelope {}
    }
}
