#![feature(register_tool)]
#![register_tool(sage)]
//! Explicit gaps in representation guarantees. Passing these placeholders does
//! not establish the behavior; associated production tests show partial evidence.

mod todo {
    #[test]
    #[sage::test(crates.sage_ir.symbols.definition_identity)]
    fn symbols_definition_identity() {
        // TODO: Add the distinct-path/same-symbol and equal-spelling/different-scope
        // resolution matrix; current output examples do not establish it.
    }

    #[test]
    #[sage::test(crates.sage_ir.symbols.kind_wrappers)]
    fn symbols_kind_wrappers() {
        // TODO: Complete compile-time/API conversion and local/external dispatch coverage
        // for every supported symbol kind.
    }

    #[test]
    #[sage::test(crates.sage_ir.symbols.local_identity)]
    fn symbols_local_identity() {
        // TODO: Extend the per-kind detail matrix to sibling insertion/reordering and
        // signature/member/body reuse for every local kind.
    }

    #[test]
    #[sage::test(crates.sage_ir.symbols.external_identity)]
    fn symbols_external_identity() {
        // TODO: SymExt also interns kind; add identity tests across metadata updates and
        // reject inconsistent provider kinds.
    }

    #[test]
    #[sage::test(crates.sage_ir.symbols.external_fact_queries)]
    fn symbols_external_fact_queries() {
        // TODO: Complete definition-keyed signature/body/proof/normalization traces
        // including rejection of unrelated metadata reads.
    }

    #[test]
    #[sage::test(crates.sage_ir.symbols.associated_interfaces)]
    fn symbols_associated_interfaces() {
        // TODO: Add a trace assertion excluding sibling-body reads for trait and impl
        // functions, types, and constants.
    }

    #[test]
    #[sage::test(crates.sage_ir.symbols.body_interfaces)]
    fn symbols_body_interfaces() {
        // TODO: Cold/warm clone traces establish one case; add the full callee-body edit
        // matrix and sibling-body trace cases.
    }

    #[test]
    #[sage::test(crates.sage_ir.types.binder.owner_parameters)]
    fn types_binder_owner_parameters() {
        // TODO: Add copy/open identity checks and owner-plus-method generic coverage for all
        // associated item kinds.
    }

    #[test]
    #[sage::test(crates.sage_ir.types.spans.relative_composition)]
    fn types_spans_relative_composition() {
        // TODO: Complete attributes/signatures/bodies/associated/error/zero-width/generated-
        // text coordinate fixtures before and after item and sibling movement.
    }

    #[test]
    #[sage::test(crates.sage_ir.types.spans.offset_reuse)]
    fn types_spans_offset_reuse() {
        // TODO: Extend plain/generated item insertion/removal edit tests to compare resolved
        // positions, semantic values, and downstream query reuse.
    }

    #[test]
    #[sage::test(crates.sage_ir.types.spans.generated_identity)]
    fn types_spans_generated_identity() {
        // TODO: Current tests cover derive movement and duplicate occurrences; add nested
        // expansion identity/text/origin/coordinate comparisons.
    }

    #[test]
    #[sage::test(crates.sage_ir.types.aliases.identity)]
    fn types_aliases_identity() {
        // TODO: Copy/fold/display and canonical round trips are covered; complete inference
        // and oracle identity-preservation fixtures.
    }

    // {anchor}`crates.sage_ir.types.aliases.revelation`
    // TODO: Named reveal, opaque reveal boundaries, and the complete unrevealed-bound proof matrix remain pending.
    #[test]
    #[sage::test(crates.sage_ir.types.aliases.revelation)]
    fn types_aliases_revelation() {
        // TODO: Named reveal, opaque reveal boundaries, and the complete unrevealed-bound
        // proof matrix remain pending.
    }

    #[test]
    #[sage::test(crates.sage_ir.types.lifetimes.dummy)]
    fn types_lifetimes_dummy() {
        // TODO: Local explicit/elided/static and synthesized references are covered;
        // complete imported/bound/inferred-origin tests.
    }

    #[test]
    #[sage::test(crates.sage_ir.types.body.typed_provenance)]
    fn types_body_typed_provenance() {
        // TODO: Add a whole-tree completion validator covering every expression/pattern
        // shape and synthesized provenance.
    }

    #[test]
    #[sage::test(crates.sage_ir.types.body.resolved_references)]
    fn types_body_resolved_references() {
        // TODO: Add a whole-tree validator rejecting every unresolved semantic reference and
        // unsupported placeholder.
    }

    #[test]
    #[sage::test(crates.sage_ir.types.body.no_inference)]
    fn types_body_no_inference() {
        // TODO: Collect completion fixtures traversing the entire returned tree and
        // rejecting live inference or unresolved choices.
    }

    // {anchor}`crates.sage_ir.types.body.elaborated`
    // TODO: Source-shaped TyExprData variants remain for unsupported constructs; complete elaboration and whole-tree rejection tests.
    #[test]
    #[sage::test(crates.sage_ir.types.body.elaborated)]
    fn types_body_elaborated() {
        // TODO: Source-shaped TyExprData variants remain for unsupported constructs;
        // complete elaboration and whole-tree rejection tests.
    }

    #[test]
    #[sage::test(crates.sage_ir.types.body.diagnosed_recovery)]
    fn types_body_diagnosed_recovery() {
        // TODO: Add full recovery/completion tests pairing error nodes with emitted
        // diagnostics across unsupported constructs.
    }

    #[test]
    #[sage::test(crates.sage_ir.types.calls.selected_semantics)]
    fn types_calls_selected_semantics() {
        // TODO: Complete direct/static-trait substitution cases and generic trait calls;
        // current clone fixture covers static dispatch and receiver elaboration.
    }

    // {anchor}`crates.sage_ir.types.calls.dispatch_families`
    // TODO: Dynamic, function-pointer, and callable-trait dispatch and the complete substitution/adjustment matrix remain pending.
    #[test]
    #[sage::test(crates.sage_ir.types.calls.dispatch_families)]
    fn types_calls_dispatch_families() {
        // TODO: Dynamic, function-pointer, and callable-trait dispatch and the complete
        // substitution/adjustment matrix remain pending.
    }

    #[test]
    #[sage::test(crates.sage_ir.types.operators.trait_calls)]
    fn types_operators_trait_calls() {
        // TODO: Ground primitive and user-defined operators are covered; generic operators
        // and the full oracle matrix remain pending.
    }

    #[test]
    #[sage::test(crates.sage_ir.types.operators.lazy_control_flow)]
    fn types_operators_lazy_control_flow() {
        // TODO: Add test bodies that directly assert both lazy node kinds and conditional
        // RHS semantics; existing helpers only check Binary shape and no trait lookup.
    }

    // {anchor}`crates.sage_ir.types.body.closures`
    // TODO: Closures and capture elaboration remain pending.
    #[test]
    #[sage::test(crates.sage_ir.types.body.closures)]
    fn types_body_closures() {
        // TODO: Closures and capture elaboration remain pending.
    }

    // {anchor}`crates.sage_ir.types.body.coroutines`
    // TODO: Async/coroutine body representation remains pending.
    #[test]
    #[sage::test(crates.sage_ir.types.body.coroutines)]
    fn types_body_coroutines() {
        // TODO: Async/coroutine body representation remains pending.
    }

    // {anchor}`crates.sage_ir.types.body.indexing`
    // TODO: Index trait-call elaboration remains pending.
    #[test]
    #[sage::test(crates.sage_ir.types.body.indexing)]
    fn types_body_indexing() {
        // TODO: Index trait-call elaboration remains pending.
    }

    // {anchor}`crates.sage_ir.types.body.conditional_patterns`
    // TODO: Complete conditional-pattern elaboration and tree fixtures remain pending.
    #[test]
    #[sage::test(crates.sage_ir.types.body.conditional_patterns)]
    fn types_body_conditional_patterns() {
        // TODO: Complete conditional-pattern elaboration and tree fixtures remain pending.
    }

    // {anchor}`crates.sage_ir.types.body.for_iteration`
    // TODO: For-loop iterator elaboration remains pending.
    #[test]
    #[sage::test(crates.sage_ir.types.body.for_iteration)]
    fn types_body_for_iteration() {
        // TODO: For-loop iterator elaboration remains pending.
    }

    // {anchor}`crates.sage_ir.types.body.try_residual`
    // TODO: Try and residual elaboration remain pending.
    #[test]
    #[sage::test(crates.sage_ir.types.body.try_residual)]
    fn types_body_try_residual() {
        // TODO: Try and residual elaboration remain pending.
    }

    // {anchor}`crates.sage_ir.types.body.await`
    // TODO: Await conversion/output elaboration remains pending.
    #[test]
    #[sage::test(crates.sage_ir.types.body.await)]
    fn types_body_await() {
        // TODO: Await conversion/output elaboration remains pending.
    }

    // {anchor}`crates.sage_ir.types.body.macro_expansion`
    // TODO: Expression macro expansion and whole-tree rejection of unexpanded macro nodes remain pending.
    #[test]
    #[sage::test(crates.sage_ir.types.body.macro_expansion)]
    fn types_body_macro_expansion() {
        // TODO: Expression macro expansion and whole-tree rejection of unexpanded macro
        // nodes remain pending.
    }
    #[test]
    #[sage::test(crates.sage_ir.types.spans.item_origin)]
    fn types_spans_item_origin() {
        // TODO: Add focused parser fixtures comparing attribute and nested associated-item
        // byte coordinates, including empty and error nodes.
    }
    #[test]
    #[sage::test(crates.sage_ir.types.scoped_identities)]
    fn types_scoped_identities() {
        // TODO: Add tests distinguishing generic, field-owner/index, and body-local
        // identities across unrelated owners.
    }
}
