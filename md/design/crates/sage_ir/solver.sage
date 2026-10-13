module crate::solver;

// Sources: check/solve/{canonical,prove,clauses,boundary,result}.rs.
class Solver {
    """Logical namespace for canonical solver queries; detailed proof rules are a separate semantic specification."""
    query solve() -> crate::types::Answer {
        """
        Solve a canonical positive trait or normalization goal using assumptions, applicable local/external impl clauses, and nested goals. Preserve canonical response variables, ambiguity, and explicit limits. Current lifetime handling is dummy-only. Scheduling and richer cycle semantics remain separate planned work.

        {anchor}`normalization_demand`
        **Alias normalization preserves reveal context and narrow demand.** An alias retains its identity until an operation requests its value. Revealability participates in the canonical input, and projection normalization reads only the fixed trait, associated item, applicable impl headers, selected associated values, and predicates required for that value. It never reads unrelated impls or bodies. This is the solver consequence of [Named, associated, and opaque aliases share one semantic family](../../../rfds/architectural-foundations/README.md#named-associated-and-opaque-aliases-share-one-semantic-family) and [Cross-item dependencies stop at semantic interfaces](../../../rfds/architectural-foundations/README.md#cross-item-dependencies-stop-at-semantic-interfaces).

        {anchor}`bounded_completeness`
        **Solver completeness is groundness-sensitive and explicitly bounded.** Non-ground queries terminate with only sound knowledge but may be ambiguous; ground queries return the complete logical answer unless a named, deterministic resource limit is exhausted. Incomplete candidate sources, ambiguity, and each overflow class remain distinct and none becomes `No`. This is the solver consequence of [Trait solving is groundness-sensitive and resource-bounded](../../../rfds/architectural-foundations/README.md#trait-solving-is-groundness-sensitive-and-resource-bounded) and [Incompleteness is an explicit terminal outcome](../../../rfds/architectural-foundations/README.md#incompleteness-is-an-explicit-terminal-outcome).

        {anchor}`canonical_outputs`
        **Goal-specific output is canonical solver knowledge.** `Prove(P)` returns `Proven`, while input-only `Normalize(Alias)` returns a `Type`. The output's variables participate in response binding, validation, caching, comparison, merging, and caller import; the caller's expected type is related only after complete candidate answers are merged. This is the solver consequence of [Solver operations return goal-specific semantic outputs](../../../rfds/architectural-foundations/README.md#solver-operations-return-goal-specific-semantic-outputs).

        {anchor}`alternative_isolation`
        **Alternative state is isolated and answer reduction is output-aware.** Every disjunct candidate publishes a canonical response from isolated inference state. Aggregation is order-independent, retains hard knowledge common to every live alternative, and merges value-producing answers only when their outputs agree or an explicit priority rule dominates. Conjunct cancellation is permitted only after reconciling shared hard information. This is the solver consequence of [Versioned egraph children are inference transactions](../../../rfds/architectural-foundations/README.md#versioned-egraph-children-are-inference-transactions) and [Whiteboard producers own isolated proof contexts](../../../rfds/architectural-foundations/README.md#whiteboard-producers-own-isolated-proof-contexts).

        {anchor}`shared_final_result`
        **One whiteboard frame has one final future output.** A frame owns the producer for one canonical atomic goal, and all requesters subscribe to that work. Candidate-local and frame-level progress are revisioned side information; they do not create extra future outputs or expose raw candidate state. Suspended continuations remain Rust futures, polled in bounded rounds.

        {anchor}`canonical_cycles`
        **Recursive semantics follow canonical cycles, not polling order.** Repeated canonical goals use explicit inductive, coinductive, or unknown path semantics and provisional fixpoint evaluation. Infinitely growing distinct goals are overflow governed by deterministic structural limits, not ordinary cycle lookup.

        {anchor}`schedule_independence`
        **Scheduling cannot change the canonical return value.** For fixed semantic inputs and configured limits, polling order may change discovery, cancellation timing, trace order, elapsed work, and memory use, but not the answer class, output, substitutions, residuals, hints, response numbering, ambiguity/overflow distinction, or overflow kind.
        """
        envelope {
            invokes self.candidates();
            invokes self.solve();
            invokes crate::symbols::TraitSymbol::signature();
            invokes crate::symbols::ImplSymbol::signature();
            invokes crate::symbols::LocalImplSym::associated_type_value();
            invokes crate::metadata::ExternalFacts::value();
            forbid transitive crate::symbols::LocalFnSym::body;
        }
    }
    query candidates() -> crate::types::Candidates {
        """
        Discover local impls for the requested trait and external relevant impls. Current local discovery scans expanded module items; carry completeness evidence rather than interpreting missing facts as exhaustive failure.

        {anchor}`candidate_completeness`
        **Candidate discovery is global, trait-keyed, conservative, and completeness-carrying.** A proof considers represented impls in the local crate and every reachable dependency, keyed first by the fixed trait. Any self-head refinement retains blanket and unclassifiable fallback candidates, and a missing or incomplete source prevents exhaustive `No`. This is the solver consequence of [Trait impl discovery is global and trait-keyed](../../../rfds/architectural-foundations/README.md#trait-impl-discovery-is-global-and-trait-keyed) and [Incompleteness is an explicit terminal outcome](../../../rfds/architectural-foundations/README.md#incompleteness-is-an-explicit-terminal-outcome).

        {anchor}`local_index_firewall`
        **The local impl index is an incremental firewall.** Its stable crate-owned identity contains a private tracked map, and keyed tracked lookup is the only reader. An unrelated edit may rebuild the map and reexecute a cheap lookup, but an equal trait bucket is backdated before impl-header lowering, solving, normalization, or body checking; impl-body edits do not alter signature-level buckets.
        """
        envelope {
            invokes crate::modules::LocalModSym::expanded_items();
            invokes crate::symbols::ImplSymbol::signature();
            invokes crate::metadata::ExternalFacts::candidates();
        }
    }
}
