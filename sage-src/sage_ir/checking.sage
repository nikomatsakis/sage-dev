module crate::checking;

// Sources: check/mod.rs, cst/* Check implementations, and check/method*.
context Check {
    """Signature-checking context with resolver and source/target stashes."""
    fn signature() -> crate::types::Signature {
        """Resolve type paths and generic parameters, check declared types and where clauses, and assemble a bound checked interface. Detailed Rust type rules remain outside this architectural model."""
        envelope {
            // Nested type syntax is checked in the same Check context.
            invokes self.signature();
            creates resolver: crate::resolution::Resolver;
            invokes crate::resolution::Resolver::resolve();
            invokes crate::symbols::TraitSymbol::signature();
            invokes crate::symbols::StructSymbol::signature();
            invokes crate::symbols::EnumSymbol::signature();
            invokes $sage_stash::storage::Stash::alloc();
        }
    }
}

context BodyCheck {
    """Logical grouping of expression/pattern CheckWith and method lookup operations; not a Rust class."""
    fn check() -> crate::types::Body {
        """Walk expressions and patterns, resolve references, instantiate interfaces, select methods using isolated inference branches, and elaborate typed nodes. Signature permissions apply to any encountered symbol of the stated kind."""
        envelope {
            // Expressions and patterns recursively enter the same body context.
            invokes self.check();
            invokes crate::resolution::Resolver::resolve();
            invokes crate::symbols::FnSymbol::signature();
            invokes crate::symbols::TraitSymbol::signature();
            invokes crate::symbols::ImplSymbol::signature();
            invokes crate::symbols::StructSymbol::signature();
            invokes crate::symbols::EnumSymbol::signature();
            invokes crate::symbols::TraitSymbol::items();
            invokes crate::symbols::ImplSymbol::items();
            invokes crate::inference::InferCtx::relate();
            invokes crate::inference::InferCtx::obligation();
            invokes crate::inference::InferCtx::speculate();
            forbid transitive crate::symbols::LocalFnSym::body;
        }
    }
}
