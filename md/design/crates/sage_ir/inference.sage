module crate::inference;

// Sources: check/infer_ctx.rs, check/infer/, check/obligation*.
context InferCtx {
    """One body owns variables, versioned equality/bound state, obligations, tasks, diagnostics, and its working stash."""
    state state: crate::types::InferenceState;
    fn relate() -> crate::types::Unit {
        """Require equality, subtyping, or coercion. Preserve numeric domain intersections and defer constraints whose information is not available. Alias relations may register normalization obligations."""
        envelope {
            reads self.state;
            // Coercion and subtyping delegate recursively to narrower relations.
            invokes self.relate();
            invokes self.obligation();
        }
    }
    fn obligation() -> crate::types::Answer {
        """Canonicalize a proof or normalization question with its parameter environment, invoke the solver, and apply its canonical response to caller variables. Ambiguity does not prove the goal."""
        envelope {
            reads self.state;
            invokes crate::solver::Solver::solve();
        }
    }
    fn speculate() -> crate::types::Unit {
        """Fork an isolated inference version; try constraints and obligations; commit a selected branch or discard its equalities, wakes, and obligations."""
        envelope {
            reads self.state;
            invokes self.relate();
            invokes self.obligation();
        }
    }
    fn finalize() -> crate::types::Unit {
        """Drive fallback and terminal obligation discharge. Emit diagnostics for remaining uncertainty; do not publish pending inference as success."""
        envelope {
            reads self.state;
            invokes self.obligation();
        }
    }
    fn finish() -> crate::types::Body {
        """Assert quiescence and copy the working tree into a fresh output stash while resolving type edges. Preserve diagnostics and error recovery; the result has no live inference tasks."""
        envelope {
            reads self.state;
            invokes $sage_stash::storage::Stash::alloc();
        }
    }
}
