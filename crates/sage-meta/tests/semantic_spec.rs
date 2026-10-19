#![feature(register_tool)]
#![register_tool(sage)]
//! Remaining portions of migrated semantic contracts.
//! These explicit placeholders preserve the previously documented partial status.
//! Their passing results are not behavioral evidence. Existing focused tests
//! are annotated alongside them when they establish a subset of the contract.

mod todo {
    // {anchor}`crates.sage_ir.resolution.lookup.completeness`
    // TODO: Resolution returns represented alternatives, but a uniform explicit completeness result across all lookup forms is not implemented.
    #[test]
    #[sage::test(crates.sage_ir.resolution.lookup.completeness)]
    fn resolution_lookup_completeness() {
        // TODO: Complete the claim-specific verification matrix:
        // Focused fixtures distinguish absent, uniquely resolved, ambiguous, and incomplete lookup,
        // including an incomplete empty provider set, and demonstrate that only the exhaustive empty case
        // permits a definitive negative conclusion.
    }

    #[test]
    #[sage::test(crates.sage_ir.resolution.lookup.precedence)]
    fn resolution_lookup_precedence() {
        // TODO: Complete the remaining import/path matrix: macro subnamespace distinctions and Rust 2015
        // path rules. Source fixtures now cover lexical/nested/sibling shadowing, independent
        // type/value/bang-macro namespaces, named-versus-glob precedence, renamed imports,
        // cyclic and ambiguous globs, repeated routes to one definition, local/external enum
        // globs, qualified/absolute paths, and 2018/2021/2024 preludes.
    }

    #[test]
    #[sage::test(crates.sage_ir.resolution.lookup.identity_only)]
    fn resolution_lookup_identity_only() {
        // TODO: Complete the claim-specific verification matrix:
        // A normalized query trace for representative path lookups contains only the searched scopes,
        // import edges, and symbol/metadata identity operations, with no inference, trait-solver,
        // candidate-signature, or checked-body execution.
    }

    #[test]
    #[sage::test(crates.sage_ir.resolution.lookup.dependencies)]
    fn resolution_lookup_dependencies() {
        // TODO: Complete the claim-specific verification matrix:
        // Cold and warm query traces show the exact scopes and external keys read for a lookup, exclude
        // unsearched modules, unrelated associated-item signatures, and all checked bodies, and show
        // reuse when those unrelated inputs change.
    }

    #[test]
    #[sage::test(crates.sage_ir.inference.body_local_quiescence)]
    fn inference_body_local_quiescence() {
        // TODO: Complete the claim-specific verification matrix:
        // Completion tests inspect a representative typed body for inference-free types and exercise
        // `finish` with live variables, obligations, tasks, and branches to prove that each prevents
        // successful publication; diagnostic recovery has separate expected output.
    }

    #[test]
    #[sage::test(crates.sage_ir.inference.numeric_domains)]
    fn inference_numeric_domains() {
        // TODO: Complete the claim-specific verification matrix:
        // Egraph tests cover both union orders, transitivity, incompatible concrete bindings, branch
        // collapse/discard, revisions, and wakes. Source-driven completion tests cover contextual numeric
        // inference, generic propagation, selected-impl and where-clause constraints, preservation of an
        // ordinary `Any` operand before proof selection, unrelated-variable isolation, and explicit
        // recovery for otherwise unresolved integer and float classes. Tests prove this slice neither
        // narrows from operator spelling nor silently chooses `i32` or `f64`.
    }

    #[test]
    #[sage::test(crates.sage_ir.inference.transaction_isolation)]
    fn inference_transaction_isolation() {
        // TODO: Complete the claim-specific verification matrix:
        // Failed occurs checks, universe checks, multi-entry response imports, candidate matches, and
        // cancellations leave the parent semantically identical and publish no wake; concurrent siblings
        // never observe one another's branch state.
    }

    #[test]
    #[sage::test(crates.sage_ir.inference.canonical_boundary)]
    fn inference_canonical_boundary() {
        // TODO: Complete the claim-specific verification matrix:
        // Canonical round-trip tests cover rigid and flexible inputs, nested binders, universe lowering,
        // shared response-local variables, residual obligations, and type outputs. An incompatible
        // expected type does not select among otherwise ambiguous normalization candidates.
    }

    #[test]
    #[sage::test(crates.sage_ir.inference.completed_body_boundary)]
    fn inference_completed_body_boundary() {
        // TODO: Complete the claim-specific verification matrix:
        // Query traces show that unchanged and interface-equivalent requests reuse the body without
        // replaying inference, while relevant interface edits reexecute it; scheduler-order perturbation
        // produces an identical completed body and diagnostics.
    }

    #[test]
    #[sage::test(crates.sage_ir.inference.subtyping_and_coercion)]
    fn inference_subtyping_and_coercion() {
        // TODO: Complete the claim-specific verification matrix:
        // Source-driven integration tests reject never-to-any beneath invariant type constructors, accept
        // it at a top-level coercion site, and inspect the completed Typed IR to show that the accepted
        // coercion is explicit. Focused relation tests cover every implemented subtype and coercion rule
        // and demonstrate that subtype success preserves layout.
    }

    // {anchor}`crates.sage_ir.solver.normalization_demand`
    // TODO: Named alias expansion and opaque reveal context remain unsupported.
    // Local discovery still scans unrelated impl identities; excluding those reads requires
    // the tracked private index deferred by crates.sage_ir.solver.local_index_firewall.
    #[test]
    #[sage::test(crates.sage_ir.solver.normalization_demand)]
    fn solver_normalization_demand() {
        // TODO: Complete the claim-specific verification matrix:
        // Named, associated, and opaque aliases survive folding and canonical round trips with reveal
        // context intact; inside/outside opaque tests produce the appropriate different result; query
        // traces for one projection include only relevant headers and its requested value and exclude
        // sibling associated values, unrelated impls, and every body.
    }

    // {anchor}`crates.sage_ir.solver.bounded_completeness`
    // TODO: Ground completeness and the full deterministic resource taxonomy remain planned.
    #[test]
    #[sage::test(crates.sage_ir.solver.bounded_completeness)]
    fn solver_bounded_completeness() {
        // TODO: Complete the claim-specific verification matrix:
        // A fixture matrix covers ground positive and negative answers, non-ground ambiguity despite an
        // available instantiation, incomplete candidate sources, and every configured limit class.
        // Repeated runs produce the same canonical outcome and no incomplete or exhausted case produces
        // `No`.
    }

    #[test]
    #[sage::test(crates.sage_ir.solver.canonical_outputs)]
    fn solver_canonical_outputs() {
        // TODO: Complete the claim-specific verification matrix:
        // Operation/output mismatch is rejected; type outputs containing repeated response-local
        // variables round-trip with sharing and universes intact; incompatible candidate outputs remain
        // ambiguous even when the caller expects one of them; trait proof never exposes a selected impl
        // identity.
    }

    #[test]
    #[sage::test(crates.sage_ir.solver.candidate_completeness)]
    fn solver_candidate_completeness() {
        // TODO: Complete the claim-specific verification matrix:
        // Local, upstream, blanket, and unsimplifiable fixtures show that indexed discovery loses no
        // exhaustive candidate; unrelated traits and provably disjoint rigid heads are absent; indexed
        // and exhaustive enumeration produce identical canonical responses; incomplete sources never
        // manufacture `No`; and a primitive operator proof reads its ordinary `core` impl metadata rather
        // than a synthetic operator candidate.
    }

    // {anchor}`crates.sage_ir.solver.local_index_firewall`
    // TODO: The local trait-keyed private tracked index and edit firewall are not implemented; discovery still scans module items.
    #[test]
    #[sage::test(crates.sage_ir.solver.local_index_firewall)]
    fn solver_local_index_firewall() {
        // TODO: Complete the claim-specific verification matrix:
        // Persistent edit tests distinguish index rebuild, keyed lookup, header lowering, canonical
        // solving, normalization, and body execution. Unrelated-trait and impl-body edits stop at an
        // equal lookup; relevant-trait and global-hazard edits invalidate exactly their permitted
        // consumers; identities remain stable across unrelated edits.
    }

    // {anchor}`crates.sage_ir.solver.alternative_isolation`
    // TODO: Concurrent conjunction and cancellation isolation remain planned beyond current completed-answer reduction.
    #[test]
    #[sage::test(crates.sage_ir.solver.alternative_isolation)]
    fn solver_alternative_isolation() {
        // TODO: Complete the claim-specific verification matrix:
        // Candidate-order permutations produce identical canonical responses; incompatible normalization
        // outputs remain ambiguous; failed or cancelled candidates leak no equality, universe change,
        // wake, or obligation; concurrent-conjunction tests reconcile sibling information before
        // accepting an absorbing result.
    }

    // {anchor}`crates.sage_ir.solver.shared_final_result`
    // TODO: Bounded polling rounds and monotone intermediate publication remain planned.
    #[test]
    #[sage::test(crates.sage_ir.solver.shared_final_result)]
    fn solver_shared_final_result() {
        // TODO: Complete the claim-specific verification matrix:
        // Duplicate requesters share one producer, receive an identical final response, and cannot
        // observe candidate-local state; progress snapshots strengthen monotonically; a self-waking
        // pending future is deferred to a later polling round and cannot starve sibling work.
    }

    // {anchor}`crates.sage_ir.solver.canonical_cycles`
    // TODO: The provisional inductive cutoff and depth limit do not implement path-kind-aware canonical-cycle fixpoints.
    #[test]
    #[sage::test(crates.sage_ir.solver.canonical_cycles)]
    fn solver_canonical_cycles() {
        // TODO: Complete the claim-specific verification matrix:
        // Focused recursive clause families distinguish inductive failure, coinductive success, unknown-
        // path ambiguity, converging and non-converging fixpoints, and non-repeating growth overflow;
        // ready-queue perturbation never changes any result.
    }

    // {anchor}`crates.sage_ir.solver.schedule_independence`
    // TODO: Deterministic resource accounting and scheduler perturbation guarantees remain planned.
    #[test]
    #[sage::test(crates.sage_ir.solver.schedule_independence)]
    fn solver_schedule_independence() {
        // TODO: Complete the claim-specific verification matrix:
        // Deterministically perturbed ready queues produce byte-identical stashed results across proof,
        // normalization, residual, ambiguity, and every overflow case; only explicitly non-semantic trace
        // order may differ.
    }

    #[test]
    #[sage::test(crates.sage_ir.metadata.provider_authority)]
    fn metadata_provider_authority() {
        // TODO: Complete the claim-specific verification matrix:
        // The typed provider interface has no proof, normalization, method-selection, local-resolution,
        // or checked-body operation; end-to-end tests demonstrate that Sage candidate and solver queries
        // execute after raw metadata is returned rather than accepting a provider verdict.
    }

    #[test]
    #[sage::test(crates.sage_ir.metadata.explicit_incompleteness)]
    fn metadata_explicit_incompleteness() {
        // TODO: Complete the claim-specific verification matrix:
        // Fixture providers return missing, unsupported, and incomplete values at every semantic
        // boundary; callers preserve the distinction in diagnostics and completeness, and solver tests
        // prove none of those values can produce a false `Yes` or `No`.
    }

    #[test]
    #[sage::test(crates.sage_ir.metadata.product_boundaries)]
    fn metadata_product_boundaries() {
        // TODO: Complete the claim-specific verification matrix:
        // Query traces for method lookup, trait proof, and normalization show identity enumeration
        // followed only by selected signatures, applicable headers, or the requested associated value;
        // they exclude sibling items and values, unrelated impls, and all callee/impl bodies.
    }

    #[test]
    #[sage::test(crates.sage_ir.metadata.keyed_reuse)]
    fn metadata_keyed_reuse() {
        // TODO: Complete the claim-specific verification matrix:
        // Cold/warm traces pair every lowered query with its exact raw operation; the warm trace contains
        // neither execution, relevant edits invalidate both layers, and unrelated-key edits preserve or
        // backdate the lowered result.
    }

    #[test]
    #[sage::test(crates.sage_ir.checking.symbol_keyed_interfaces)]
    fn checking_symbol_keyed_interfaces() {
        // TODO: Complete the claim-specific verification matrix:
        // Query traces show a signature request and a body request as distinct symbol-keyed roots, reject
        // ambient caller parameters and callee-body reads, and demonstrate unchanged reuse across an
        // interface-preserving edit.
    }

    #[test]
    #[sage::test(crates.sage_ir.checking.identity_minting)]
    fn checking_identity_minting() {
        // TODO: Complete the claim-specific verification matrix:
        // Cross-query and persistent-edit tests compare owner, generic-parameter, `Self`, and associated-
        // item identities across signatures, field/member details, and bodies, including shadowed equal
        // names.
    }

    #[test]
    #[sage::test(crates.sage_ir.checking.owned_products)]
    fn checking_owned_products() {
        // TODO: Complete the claim-specific verification matrix:
        // Structural traversal detects any source-stash or tree-sitter pointer in checked output, equal
        // recomputation yields equal fingerprints, and a persistent edit test shows an equal recomputed
        // product stops downstream reexecution.
    }

    #[test]
    #[sage::test(crates.sage_ir.inference.terminal_obligations)]
    fn inference_terminal_obligations() {
        // TODO: Complete the claim-specific verification matrix:
        // Tests cover obligations solved immediately, woken after inference, deduplicated after
        // canonicalization, rejected at the terminal pass, and exhausted by a resource limit; successful
        // completion asserts no live obligations, tasks, wakes, or root branches.
    }

    #[test]
    #[sage::test(crates.sage_ir.inference.atomic_checking)]
    fn inference_atomic_checking() {
        // TODO: Complete the claim-specific verification matrix:
        // Unit tests reject a candidate after partial generic matching and observe the parent unchanged,
        // accept the same shape and observe exactly one staged obligation publication, and exercise
        // isolated sibling candidates without shared inference identities.
    }

    #[test]
    #[sage::test(crates.sage_ir.modules.parse.syntax_identities)]
    fn modules_parse_syntax_identities() {
        // TODO: Complete the claim-specific verification matrix:
        // Source and generated fixtures preserve item order, ownership, CST, and provenance; malformed or
        // unsupported top-level input produces an error item; and a parse-only query trace contains no
        // resolution, signature, body, or solver work.
    }

    #[test]
    #[sage::test(crates.sage_ir.modules.parse.detail_firewall)]
    fn modules_parse_detail_firewall() {
        // TODO: Complete the claim-specific verification matrix:
        // Persistent edit tests move an item, edit one body, and insert or reorder siblings while
        // checking symbol identity, per-item CST fingerprints, and downstream reexecution separately for
        // affected and unaffected items, including generated items.
    }

    // {anchor}`crates.sage_ir.modules.parse.recovery_completeness`
    // TODO: Parse errors are retained, but a uniform question-sensitive completeness result is not implemented.
    #[test]
    #[sage::test(crates.sage_ir.modules.parse.recovery_completeness)]
    fn modules_parse_recovery_completeness() {
        // TODO: Complete the claim-specific verification matrix:
        // Malformed and unsupported-item fixtures retain safe sibling symbols while downstream exhaustive
        // queries refuse false negative conclusions attributable to the missing construct.
    }

    #[test]
    #[sage::test(crates.sage_ir.modules.parse.dependencies)]
    fn modules_parse_dependencies() {
        // TODO: Complete the claim-specific verification matrix:
        // A cold parse trace names only those inputs, an unchanged warm request executes no parse work,
        // and edits to semantic facts that do not change parse inputs leave the parse result reusable.
    }

    #[test]
    #[sage::test(crates.sage_ir.modules.expand.direct_members)]
    fn modules_expand_direct_members() {
        // TODO: Complete the claim-specific verification matrix:
        // Fixtures cover ordinary items, `use` items, inline child modules, recursively generated items,
        // and derives, asserting direct membership, ordering, ownership, and generated provenance without
        // treating imported or child names as direct parent members.
    }

    #[test]
    #[sage::test(crates.sage_ir.modules.expand.completeness)]
    fn modules_expand_completeness() {
        // TODO: Complete the claim-specific verification matrix:
        // Successful expansion permits a ground exhaustive negative query, while unresolved, malformed,
        // unsupported, and resource-limited expansion produce conservative uncertainty rather than the
        // same negative.
    }

    #[test]
    #[sage::test(crates.sage_ir.modules.expand.recursion_and_fixpoint)]
    fn modules_expand_recursion_and_fixpoint() {
        // TODO: Complete the claim-specific verification matrix:
        // One fixture exercises nested emitted invocations, another requires a macro definition emitted
        // earlier in the same module, and traces show bounded fixed-point reexecution followed by stable
        // warm reuse.
    }

    #[test]
    #[sage::test(crates.sage_ir.modules.expand.dependencies)]
    fn modules_expand_dependencies() {
        // TODO: Complete the claim-specific verification matrix:
        // Cold and warm traces identify the expansion boundary; edits to an invocation or selected macro
        // output invalidate it; body-only and unrelated-signature edits do not; and unchanged
        // reconstructed output is backdated before downstream consumers execute.
    }

    #[test]
    #[sage::test(crates.sage_ir.checking.signature.body_independence)]
    fn checking_signature_body_independence() {
        // TODO: Complete the claim-specific verification matrix:
        // Query traces for local and external callers name the selected interface facts and no bodies; a
        // body-only edit preserves the signature result and does not reexecute its consumers.
    }

    #[test]
    #[sage::test(crates.sage_ir.checking.signature.owner_binder)]
    fn checking_signature_owner_binder() {
        // TODO: Complete the claim-specific verification matrix:
        // Trait, impl, associated-function, field, and body fixtures compare semantic parameter
        // identities across queries and revisions, including separate owner and method generic scopes.
    }

    #[test]
    #[sage::test(crates.sage_ir.checking.signature.storage)]
    fn checking_signature_storage() {
        // TODO: Complete the claim-specific verification matrix:
        // Structural snapshots traverse representative signatures after the parser and source stash
        // borrows have ended, cover retained aliases and predicates, and detect any cross-stash pointer.
    }

    #[test]
    #[sage::test(crates.sage_ir.checking.signature.narrow_detail)]
    fn checking_signature_narrow_detail() {
        // TODO: Complete the claim-specific verification matrix:
        // Query traces select one field or associated item and reject reads of unselected sibling
        // signatures and all bodies; edit tests show that a detail-only change does not remint the owner
        // binder or invalidate unrelated detail queries.
    }

    #[test]
    #[sage::test(crates.sage_ir.checking.body.elaboration)]
    fn checking_body_elaboration() {
        // TODO: Complete the claim-specific verification matrix:
        // Structural Typed-IR snapshots cover paths, fields, calls, generic substitutions, borrows,
        // dereferences, coercions, and recovery; completion checks reject every unresolved or live
        // internal form; and selected slices match the independent rustc oracle by exact text.
    }

    #[test]
    #[sage::test(crates.sage_ir.checking.body.interface_dependencies)]
    fn checking_body_interface_dependencies() {
        // TODO: Complete the claim-specific verification matrix:
        // Cold traces whitelist the semantic facts used by a representative body and reject every callee-
        // body or unrelated-interface read; an unchanged warm request performs no work; and persistent
        // edits distinguish relevant interface changes from unrelated body changes.
    }

    #[test]
    #[sage::test(crates.sage_ir.checking.body.isolated_completion)]
    fn checking_body_isolated_completion() {
        // TODO: Complete the claim-specific verification matrix:
        // Rejected-candidate tests observe no leaked generic bindings, wakes, or obligations; accepted-
        // candidate tests observe one atomic commit; unresolved residual obligations become diagnostics;
        // and successful completion asserts an empty runtime, wake queue, obligation registry, and root
        // transaction set.
    }

    // {anchor}`crates.sage_ir.checking.body.expected_types`
    // TODO: Direct-call expectations are supplied, but recursive expression-specific consumption of expected types remains incomplete.
    #[test]
    #[sage::test(crates.sage_ir.checking.body.expected_types)]
    fn checking_body_expected_types() {
        // TODO: Complete the claim-specific verification matrix:
        // Source-driven tests show expected types guide representative call arguments, literals,
        // closures, and aggregate or branch elements; completed Typed IR contains any selected
        // conversions; and an incompatible expected type cannot choose among otherwise ambiguous solver
        // or method candidates.
    }

    // {anchor}`crates.sage_ir.checking.body.operator_calls`
    // TODO: Generic operator bodies and unknown-Self candidate hints remain incomplete.
    #[test]
    #[sage::test(crates.sage_ir.checking.body.operator_calls)]
    fn checking_body_operator_calls() {
        // TODO: Complete the claim-specific verification matrix:
        // Table-driven checks cover every represented operator-to-trait mapping, including different
        // integer types for shift operands. Query traces show primitive proofs loading ordinary `core`
        // impl identities and headers from external metadata, with no synthetic primitive candidate.
        // Source-driven tests and independent rustc oracle cases cover representative positive and
        // negative expressions; concrete primitive, generic, nonnumeric shift, and user-defined calls;
        // non-`Self` `Rhs` and `Output`; preservation of `Any` domains before proof selection; lazy
        // boolean short-circuit nodes; and rejection of `1 + true`.
    }
}
