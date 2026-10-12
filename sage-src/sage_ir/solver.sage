module crate::solver;

// Sources: check/solve/{canonical,prove,clauses,boundary,result}.rs.
class Solver {
    """Logical namespace for canonical solver queries; detailed proof rules are a separate semantic specification."""
    query solve() -> crate::types::Answer {
        """Solve a canonical positive trait or normalization goal using assumptions, applicable local/external impl clauses, and nested goals. Preserve canonical response variables, ambiguity, and explicit limits. Current lifetime handling is dummy-only. Scheduling and richer cycle semantics remain separate planned work."""
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
        """Discover local impls for the requested trait and external relevant impls. Current local discovery scans expanded module items; carry completeness evidence rather than interpreting missing facts as exhaustive failure."""
        envelope {
            invokes crate::modules::LocalModSym::expanded_items();
            invokes crate::symbols::ImplSymbol::signature();
            invokes crate::metadata::ExternalFacts::candidates();
        }
    }
}
