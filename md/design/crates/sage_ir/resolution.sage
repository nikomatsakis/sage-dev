module crate::resolution;

// Sources: check/resolve/ and check/infer_ctx.rs::Scope.
context Resolver {
    """Lexical/module environment with generic ribs; ordinary mutable helper, not a public query."""
    state environment: crate::types::Environment;
    fn resolve() -> crate::types::Resolution {
        """
        Search local ribs, defining module, imports, and external module metadata in the requested namespace. Retain ambiguity and unavailable results; follow module paths and macro namespaces as needed.

        {anchor}`lookup.completeness`
        **Negative reasoning requires explicit completeness.** A resolution result preserves every represented alternative separately from the fact that the searched source is exhaustive. An empty incomplete result is never interpreted as proof that the name or provider does not exist. This is the resolution consequence of [Incompleteness is an explicit terminal outcome](../../../rfds/architectural-foundations/README.md#incompleteness-is-an-explicit-terminal-outcome).

        {anchor}`lookup.precedence`
        **Lookup precedence is determined by scope, namespace, and path form.** Lexical ribs take priority only for an unqualified one-segment name; module lookup then follows the applicable Rust namespace, imports, ancestry, prelude, and external-module rules rather than combining every visible name into an unordered global set. Leaving a lexical scope restores enclosing bindings; later bindings retain their own body-local identity and type even when nested or sibling scopes have allocated other locals.

        {anchor}`lookup.expansion_import_ambiguity`
        **Expansion cannot silently change an unqualified import root.** Final import validation rejects a relative import whose first segment has a macro-generated direct or named-import binding and a glob binding to a distinct definition in the same namespace. This applies to unused imports and to roots of longer import paths. Glob targets and intermediate path segments use the type namespace; conflicts confined to another namespace do not make those roots ambiguous. Repeated routes to one definition are not ambiguous. Source-written named bindings retain precedence; explicitly anchored paths bypass this unqualified-root check. Validation reports the conflict after expansion without replacing the combined named-and-glob lookup used during expansion.

        {anchor}`lookup.identity_only`
        **Resolution stops at semantic identity.** Ordinary path and name lookup returns symbols and completeness information; it does not infer a receiver, normalize an associated type, prove a trait obligation, or select an applicable method candidate as a hidden side effect.

        {anchor}`lookup.dependencies`
        **Resolution dependencies follow the lookup actually performed.** The owning phase observes only searched lexical/module scopes and keyed external membership. Provider discovery enumerates identities before loading selected signatures, and resolution never reads a checked body. This is the resolution consequence of [Cross-item dependencies stop at semantic interfaces](../../../rfds/architectural-foundations/README.md#cross-item-dependencies-stop-at-semantic-interfaces).
        """
        envelope {
            reads self.environment;
            invokes crate::modules::LocalModSym::expanded_items();
            invokes crate::metadata::ExternalFacts::items();
        }
    }
}
