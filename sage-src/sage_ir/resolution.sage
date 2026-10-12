module crate::resolution;

// Sources: check/resolve/ and check/infer_ctx.rs::Scope.
context Resolver {
    """Lexical/module environment with generic ribs; ordinary mutable helper, not a public query."""
    state environment: crate::types::Environment;
    fn resolve() -> crate::types::Resolution {
        """Search local ribs, defining module, imports, and external module metadata in the requested namespace. Retain ambiguity and unavailable results; follow module paths and macro namespaces as needed."""
        envelope {
            reads self.environment;
            invokes crate::modules::LocalModSym::expanded_items();
            invokes crate::metadata::ExternalFacts::items();
        }
    }
}
