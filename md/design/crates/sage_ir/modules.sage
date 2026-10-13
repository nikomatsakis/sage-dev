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
        """
        Parse a file-backed module and create stable symbols from individual CSTs. Inline module items are supplied by the containing parse.

        {anchor}`expand.direct_members`
        **Expansion returns direct module members, not a visible-name environment.** The result preserves deterministic order and provenance for source and generated direct items. Child-module members remain in the child, while imports and namespace lookup are interpreted later by resolution.

        {anchor}`expand.completeness`
        **Exhaustive absence requires complete expansion.** Published symbols are safe to use even when expansion is incomplete, but an omitted symbol supports a definite negative answer only when expansion is complete for that semantic question. This is the expansion consequence of [Incompleteness is an explicit terminal outcome](../../../rfds/architectural-foundations/README.md#incompleteness-is-an-explicit-terminal-outcome).

        {anchor}`expand.dependencies`
        **Expansion observes expansion inputs, never checked semantic detail.** A module expansion may depend on parsed item identities, macro resolution and definitions, generated output, and completeness facts. It must not depend on item signatures, trait solutions, or function bodies.
        """
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
        """
        Parse source with tree-sitter; copy self-contained CSTs and create per-kind symbols in their scope. Generated source reuses the same representation.

        {anchor}`parse.syntax_identities`
        **Parsing publishes syntax-backed identities, not semantic conclusions.** A represented item has stable local identity, self-contained CST, ordering, ownership, and provenance. Resolution and checking remain downstream operations, and malformed or unsupported top-level syntax is represented explicitly rather than accepted as a definition. Tree-sitter is temporary parser machinery under [Tree-sitter for parsing](../../../rfds/architectural-foundations/README.md#tree-sitter-for-parsing), not the published identity or syntax representation.

        {anchor}`parse.detail_firewall`
        **Item identity is separate from tracked syntax detail.** Rebuilding a module may update an item's CST or absolute location without minting a new symbol when its semantic identity is unchanged. Every represented definition owns a separate CST stash so an equal sibling can be backdated independently. This applies [Symbols form the uniform semantic identity family](../../../rfds/architectural-foundations/README.md#symbols-form-the-uniform-semantic-identity-family) and [Stash for type-level interning](../../../rfds/architectural-foundations/README.md#stash-for-type-level-interning) at the parse boundary.

        {anchor}`parse.recovery_completeness`
        **Parse recovery never makes absence conclusive.** An error item or error CST node is terminal for the current parse, but any consumer asking an exhaustive question must treat the surrounding represented set as incomplete unless it can prove the error irrelevant to that question. This is the parsing consequence of [Incompleteness is an explicit terminal outcome](../../../rfds/architectural-foundations/README.md#incompleteness-is-an-explicit-terminal-outcome).

        {anchor}`parse.dependencies`
        **The parse query depends only on parse inputs.** Parsing one module may read its source, scope, edition, and generated parse source, but never resolved names, checked interfaces, bodies of other items, or trait results.
        """
        envelope {
            invokes $sage_stash::storage::Stash::alloc();
        }
    }
}

context Expansion {
    """Macro expansion orchestration."""
    fn expand() -> crate::types::Items {
        """
        Resolve the macro, expand supported invocation/derive forms, and parse generated tokens with expansion provenance. Respect expansion depth limits.

        {anchor}`expand.recursion_and_fixpoint`
        **Recursive expansion and same-module fixed points are distinct.** Recursive processing expands invocations emitted directly by another invocation. Salsa fixed-point recovery is used only when resolving a macro requires the provisional expanded members of that same module; convergence does not imply completeness.
        """
        envelope {
            invokes crate::resolution::Resolver::resolve();
            invokes crate::modules::Parser::parse();
            invokes self.expand();
        }
    }
}
