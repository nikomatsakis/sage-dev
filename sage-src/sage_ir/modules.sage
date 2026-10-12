module crate::modules;

// Sources: local_syms/mods.rs, parse/mod.rs, derive/, local_syms/macro_invocations.rs.
class LocalModSym {
    """Module identity with lazily computed direct items."""
    identity name: crate::types::Name;
    identity edition: crate::types::Edition;
    identity parent: crate::types::OptionalScope;
    input source: crate::types::Source;
    input attrs: crate::types::Syntax;
    input span: crate::types::Span;
    query unexpanded_items() -> crate::types::Items {
        """Parse a file-backed module and create stable symbols from individual CSTs. Inline module items are supplied by the containing parse."""
        envelope {
            reads self.source;
            invokes crate::modules::Parser::parse();
        }
    }
    query expanded_items() -> crate::types::Items {
        """Expand macros and derives recursively, retaining source order and generated provenance. Same-module macro lookup uses a Salsa fixed point initially seeded with an empty list. Failed or unsupported expansions can be omitted; consumer-specific audits track completeness."""
        envelope {
            invokes self.unexpanded_items();
            creates resolver: crate::resolution::Resolver;
            invokes crate::resolution::Resolver::resolve();
            invokes crate::modules::Expansion::expand();
        }
    }
}

context Parser {
    """Logical boundary for parse_str_to_cst and ParseCx."""
    fn parse() -> crate::types::Items {
        """Parse source with tree-sitter; copy self-contained CSTs and create per-kind symbols in their scope. Generated source reuses the same representation."""
        envelope {
            invokes $sage_stash::storage::Stash::alloc();
        }
    }
}

context Expansion {
    """Macro expansion orchestration."""
    fn expand() -> crate::types::Items {
        """Resolve the macro, expand supported invocation/derive forms, and parse generated tokens with expansion provenance. Respect expansion depth limits."""
        envelope {
            invokes crate::resolution::Resolver::resolve();
            invokes crate::modules::Parser::parse();
            invokes self.expand();
        }
    }
}
