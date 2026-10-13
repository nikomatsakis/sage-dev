module crate::inference;

// Sources: check/infer_ctx.rs, check/infer/, check/obligation*.
context InferCtx {
    """
    One body owns variables, versioned equality/bound state, obligations, tasks, diagnostics, and its working stash.

    {anchor}`body_local_quiescence`
    **One body owns inference through quiescence.** An `InferCtx` and all of its variables, versions, tasks, and obligations belong to one body query. A successful `CheckedBody` is published only after every type in its tree is resolved and no task, obligation, or speculative branch remains live.
    """
    state state: crate::types::InferenceState;
    fn relate() -> crate::types::Unit {
        """
        Require equality, subtyping, or coercion. Preserve numeric domain intersections and defer constraints whose information is not available. Alias relations may register normalization obligations.

        {anchor}`numeric_domains`
        **Inference domains narrow transactionally without selecting a default.** An effective domain follows its complete equality class, narrows only by intersection, and obeys the versioned egraph's commit, rollback, revision, and wake rules. Only proof-derived constraints make a numeric class concrete in this slice; overloadable operator spelling does not narrow a domain, and scheduler quiescence does not authorize defaulting.

        {anchor}`subtyping_and_coercion`
        **Subtyping preserves representation; coercion is an explicit conversion.** A successful subtype relation requires source and target to have the same memory layout and introduces no runtime conversion. A coercion is selected only at a coercion site, may change layout, and is materialized in the completed Typed IR. Never-to-any belongs only to the coercion relation.
        """
        envelope {
            reads self.state;
            // Coercion and subtyping delegate recursively to narrower relations.
            invokes self.relate();
            invokes self.obligation();
        }
    }
    fn obligation() -> crate::types::Answer {
        """
        Canonicalize a proof or normalization question with its parameter environment, invoke the solver, and apply its canonical response to caller variables. Ambiguity does not prove the goal.

        {anchor}`canonical_boundary`
        **Solver knowledge crosses one canonical, transactional boundary.** Goals and their local environment are canonicalized together; rigid inputs, flexible inputs, universes, substitutions, residuals, and goal-specific outputs all round-trip without capture or lost sharing. A normalization output is imported before it is related to the caller's expected type. This is the inference consequence of [Solver operations return goal-specific semantic outputs](../../../rfds/architectural-foundations/README.md#solver-operations-return-goal-specific-semantic-outputs).
        """
        envelope {
            reads self.state;
            invokes crate::solver::Solver::solve();
        }
    }
    fn speculate() -> crate::types::Unit {
        """
        Fork an isolated inference version; try constraints and obligations; commit a selected branch or discard its equalities, wakes, and obligations.

        {anchor}`transaction_isolation`
        **Speculation is an all-or-nothing inference transaction.** Every operation which may partially constrain inference executes in an isolated child version. Complete success may atomically collapse an exclusive leaf; failure or cancellation discards all of its equalities, bounds, universe changes, wakes, tasks, and staged obligations. This is the inference consequence of [Versioned egraph children are inference transactions](../../../rfds/architectural-foundations/README.md#versioned-egraph-children-are-inference-transactions) and [Inference-variable identities are unique across egraph versions](../../../rfds/architectural-foundations/README.md#inference-variable-identities-are-unique-across-egraph-versions).

        {anchor}`atomic_checking`
        **Speculative checking changes commit atomically.** A candidate or multi-part compatibility check stages its inference changes and obligations in an isolated child version. The whole operation either commits to its direct parent or is discarded; no equality, wake, or obligation from a rejected alternative reaches the requester. The version invariants are defined by [Versioned egraph children are inference transactions](../../../rfds/architectural-foundations/README.md#versioned-egraph-children-are-inference-transactions).
        """
        envelope {
            reads self.state;
            invokes self.relate();
            invokes self.obligation();
        }
    }
    fn finalize() -> crate::types::Unit {
        """
        Drive fallback and terminal obligation discharge. Emit diagnostics for remaining uncertainty; do not publish pending inference as success.

        {anchor}`terminal_obligations`
        **A checked body cannot publish residual inference work.** Solver responses may add substitutions and residual goals during checking, but fallback and a mandatory terminal pass must discharge them or emit a diagnostic before `CheckedBody` is created. Incomplete solver results are terminal outcomes under [Incompleteness is an explicit terminal outcome](../../../rfds/architectural-foundations/README.md#incompleteness-is-an-explicit-terminal-outcome), not work silently left running after return.
        """
        envelope {
            reads self.state;
            invokes self.obligation();
        }
    }
    fn finish() -> crate::types::Body {
        """
        Assert quiescence and copy the working tree into a fresh output stash while resolving type edges. Preserve diagnostics and error recovery; the result has no live inference tasks.

        {anchor}`completed_body_boundary`
        **The completed body is the incremental and scheduling boundary.** Mutable inference steps are not separately memoized or externally visible. Reuse is determined by the completed `CheckedBody` and its semantic query dependencies, and internal branch or polling order cannot change that body. This is the inference consequence of [Cross-item dependencies stop at semantic interfaces](../../../rfds/architectural-foundations/README.md#cross-item-dependencies-stop-at-semantic-interfaces).
        """
        envelope {
            reads self.state;
            invokes $sage_stash::storage::Stash::alloc();
        }
    }
}
