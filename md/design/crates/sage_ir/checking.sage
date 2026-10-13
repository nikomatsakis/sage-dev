module crate::checking;

// Sources: check/mod.rs, cst/* Check implementations, and check/method*.
context Check {
    """
    Signature-checking context with resolver and source/target stashes.

    {anchor}`symbol_keyed_interfaces`
    **Public checking queries are symbol-keyed semantic boundaries.** A caller supplies the definition symbol and the query derives scope and environment from it. Signatures are the primary cross-item interface; field, member, and other language-required interfaces may be separate narrow products. Bodies and checking temporaries are not interfaces. This is the shared query design required by [Cross-item dependencies stop at semantic interfaces](../../../rfds/architectural-foundations/README.md#cross-item-dependencies-stop-at-semantic-interfaces).
    """
    fn signature() -> crate::types::Signature {
        """
        Resolve type paths and generic parameters, check declared types and where clauses, and assemble a bound checked interface. Detailed Rust type rules remain outside this architectural model.

        {anchor}`identity_minting`
        **Semantic identities have exactly one minting boundary.** An owning query creates each generic parameter or associated-item symbol once. Every dependent query reopens or references that identity instead of reconstructing it from its name or source position. The identity family is defined by [Symbols form the uniform semantic identity family](../../../rfds/architectural-foundations/README.md#symbols-form-the-uniform-semantic-identity-family).

        {anchor}`owned_products`
        **Checked products own their semantic storage.** CST is immutable input; each checked signature or body allocates into a fresh output stash and returns a self-contained `Stashed<T>`. Its deterministic content fingerprint, rather than allocation address or source-tree lifetime, is the value used for backdating. [Stash for type-level interning](../../../rfds/architectural-foundations/README.md#stash-for-type-level-interning) owns this representation choice.

        {anchor}`signature.body_independence`
        **A checked signature is the body-independent cross-item interface.** Consumers obtain the types, generic binder, and parameter environment needed to use a definition without checking that definition's body. No signature query reads any function body. This is the signature-phase consequence of [Cross-item dependencies stop at semantic interfaces](../../../rfds/architectural-foundations/README.md#cross-item-dependencies-stop-at-semantic-interfaces).

        {anchor}`signature.owner_binder`
        **Generic identities are minted once by their owner and reused.** An owner signature creates its parameters under one binder. Associated-item, field, and body queries reopen that binder and add only genuinely new item-level parameters; they never reconstruct an existing `GenericParam` or owner `Self` identity.

        {anchor}`signature.storage`
        **Checked interfaces are self-contained semantic values.** Signature lowering reads CST from one stash and writes all checked types, binders, predicates, and retained alias applications into a separate result stash. No checked value points into tree-sitter state or the source CST stash. This is the signature-level ownership boundary established by [Stash for type-level interning](../../../rfds/architectural-foundations/README.md#stash-for-type-level-interning).

        {anchor}`signature.narrow_detail`
        **Interface detail is split at semantic reuse boundaries.** Fields and associated-item enumeration remain independently keyed from the owner signature, and selecting one associated item reads only that item's signature. Enumeration never checks sibling signatures or bodies eagerly.
        """
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
        """
        Walk expressions and patterns, resolve references, instantiate interfaces, select methods using isolated inference branches, and elaborate typed nodes. Signature permissions apply to any encountered symbol of the stated kind.

        {anchor}`body.elaboration`
        **A completed body is an elaborated semantic tree.** Successful output contains final types and semantic identities and materializes all type-directed operations used by the source. Method syntax, unresolved names, inference variables, pending obligations, and adjustment recipes do not survive in `CheckedBody`; [Completed bodies are elaborated typed trees](../../../rfds/architectural-foundations/README.md#completed-bodies-are-elaborated-typed-trees) owns the cross-cutting representation decision.

        {anchor}`body.interface_dependencies`
        **Body checking depends on interfaces, never other bodies.** The body query reads only its own CST and signature plus the name-resolution, interface, metadata, candidate, and solver facts demanded by that body. A call may read its callee's signature but cannot read the callee body. This is the body-phase consequence of [Cross-item dependencies stop at semantic interfaces](../../../rfds/architectural-foundations/README.md#cross-item-dependencies-stop-at-semantic-interfaces).

        {anchor}`body.isolated_completion`
        **Speculation is isolated and completion is quiescent.** Candidate matching and other speculative relations occur in child inference versions; failure discards their equalities, wakeups, and staged obligations, while a successful operation commits them atomically. The public result is frozen only after fallback and terminal obligation processing leave no live work. [Versioned egraph children are inference transactions](../../../rfds/architectural-foundations/README.md#versioned-egraph-children-are-inference-transactions) defines the shared transaction rules.

        {anchor}`body.expected_types`
        **Expected types flow into expression checking without selecting semantic candidates.** A caller supplies an available expected type before checking the expression beneath that boundary. Expression forms may use it to guide local inference and explicit coercion, but trait proof, method provider discovery, and alias normalization retain their independent candidate-selection rules.

        {anchor}`body.operator_calls`
        **Overloadable operators remain resolved calls in Typed IR.** Every non-short-circuit binary operator retains its trait operation and method result through inference; its token alone neither narrows nor equates operand types. Every proven operation completes as a resolved trait-method call, including primitive and generic cases. Ambiguity and unsupported operands cannot be represented as success, and primitive specialization occurs only at the future codegen boundary defined by [the operator representation decision](../../../rfds/architectural-foundations/README.md#overloadable-operators-remain-trait-calls-through-typed-ir).
        """
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
