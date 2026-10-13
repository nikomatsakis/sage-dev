# Coverage History

**Status: Retrospective evidence record.** These descriptions were preserved
when the book moved implementation and test status to generated anchor pages.
They retain the earlier observations, query traces, test names, limitations,
and sequencing context; statements such as “current” and “implemented” refer
to the recorded snapshot, not a second live coverage registry. Numbered labels
inside these excerpts are historical references.

For current associations, explicit TODO/N/A gaps, and saved test reviews, use
[Anchors](../../design/anchor/README.md). Update the Rust implementation/test
associations and review snapshots when behavior changes. New plans belong in
scoped RFDs; do not maintain these archived status descriptions as current facts.

## Semantic Inspector

Former status section of [Semantic Inspector](../../design/crates/sage_inspector/inspector-guide.md).

### Semantic Inspector: Current frontier and evidence

- **SI-A1/A8/A9/A13/A15:** `tests/semantic_inspector.rs` exercises a retained
  real `DbDropGuard::db` host, exact macro-expansion metadata allowances,
  product catalogs, local/external navigation, relevant/unrelated edits,
  sibling reorder, rename failure, and explicit host reconstruction.
- **SI-A2/A3:** `real_command_serves_embedded_assets_and_correlated_request_logs`
  launches the production command over
  `test-projects/semantic-inspector/db-drop-guard`, crosses the live workspace,
  actor, and Axum boundary, snapshots reviewer-visible output and demand, and
  follows the advertised `Db` identity link through the catalog and product
  endpoints.
- **SI-A4:** `crates/sage-inspector/web/src/InspectorApp.test.tsx` exercises the
  generic rendering vocabulary and canonical semantic navigation without an
  API mock or product-specific data model. The production command test checks
  embedded routed assets against real API data.
- **SI-A6/A16:** `crates/sage-reflect/src/lib.rs` tests derived field/variant
  shape, repeated arena edges expanded inline, the active-edge cycle guard,
  bounded wide-sequence traversal, and frozen continuation pages; the real
  inspector test snapshots complete Concrete IR, signature, and Typed-IR
  structural shapes, including nested types, resolved static dispatch, and
  semantic links. The generic browser test renders the explicit cycle marker.
- **SI-A9/A11/A12/A17:** the real directory trace snapshots its complete set of
  operation families; the remaining real trace tests cover executed,
  validated, reused, cancelled/unwinding, nested metadata, phase attribution,
  producer-authored sequential solver-query and unordered trait/normalization
  candidate groups, and forbidden query families. Provider and command tests
  supply the stable human-reviewed projections from source.
- `real_command_serves_embedded_assets_and_correlated_request_logs` launches
  the actual command on port 0 and checks embedded direct-route assets, JSON,
  and actor-owned request logs together.
- Startup and reload tests cover an occupied loopback port, watcher readiness,
  a deliberately corrupted metadata-provider invocation, unique stub
  ownership, and a valid keyword extern alias; failures neither panic nor
  replace a working host.
- Injected replacement-watch and obsolete-unwatch failures prove that reloads
  retain a complete watch set and expose any degraded cleanup state.

Run the checked-in sample project through the live inspector:

```text
cargo run --manifest-path ../../../../Cargo.toml --bin cargo-sage -- \
  sage inspect --port 2442 --no-open
```

Run that command from `test-projects/semantic-inspector/db-drop-guard`, or run
against the current crate with `cargo sage inspect --package <name>` after
installing the Cargo subcommand in the normal development workflow.

### Semantic Inspector: Current limitations

- The local symbol path matrix covers the represented `DbDropGuard` sample
  project;
  duplicate same-name definitions and every generated ownership form do not
  yet have a complete adversarial matrix.
- Named external paths and duplicate same-name crate versions resolve by
  rustc's crate disambiguator. Direct replay of a path through an anonymous
  external impl owner does not yet have a complete resolver/evidence matrix.
- Raw Salsa IDs remain visible only inside `unmapped` diagnostic keys. Stable
  checked evidence uses operation families and required/forbidden patterns;
  not every query ingredient has a semantic key projection.
- Revision retention is process-memory and bounded to 64 revisions. It is not
  persisted across inspector processes.
- Cross-browser layout, URL-history behavior, SSE refresh, and CSS behavior do
  not yet have a source-driven browser harness. The component suite establishes
  only generic rendering; the real-process test separately checks embedded
  assets, direct routing, the Axum boundary, visible result fields, and
  actor-owned demand. SI-A3 and the browser-specific parts of SI-A4/A5 remain
  partial until a headless browser drives that same live server. The
  browser-demand portion of SI-A7 is partial for the same reason.
- Exact live Axum snapshots cover representative success, structured-error,
  asset, header, and actor-demand paths. The successful DTO catalog and the
  continuation and revision-history route families do not yet have exhaustive
  source-driven byte snapshots, so the full protocol regression gate remains
  partial.

These limitations narrow evidence or ergonomics; they do not change the
service, protocol, reflection, or revision contracts above.

## Architecture

Former status section of [Architecture](../../design/crates/sage_ir/architecture-guide.md).

### Architecture: Current frontier

The pipeline is operational for the two pinned mini-redis vertical slices
`DbDropGuard::db` and `Parse::next`. Those slices exercise local/generated and
external symbols, signature and body queries, method resolution, trait proof,
associated-type normalization, and elaborated calls.

### Architecture: Evidence

- [crates.sage_ir.crate.identity_before_detail](../../design/anchor/crates/sage_ir/crate/identity_before_detail.html): the [Symbols](../../design/crates/sage_ir/symbols-guide.md) and
  [incrementality](../../design/crates/sage_ir/incrementality-guide.md) review packets connect
  stable local identities to same-database edit experiments, including a
  per-kind detail-edit matrix and associated-item movement traces.
- [crates.sage_ir.crate.narrow_demands](../../design/anchor/crates/sage_ir/crate/narrow_demands.html): the
  [Oracle-checked method body](../../design/examples/oracle-checked-method.md) follows a
  completed method body into exact Sage/rustc output, and
  `clone_method_body_has_a_narrow_reusable_semantic_query_trace` proves the
  requested body executes once, reads selected interface metadata, reads no
  callee body, and is reused unchanged.
- [Mini-redis Conformance Roadmap](mini-redis.md) records the
  acceptance criteria and query-dependency evidence for both completed slices.

### Architecture: Current limitations

- [crates.sage_ir.crate.terminal_incompleteness](../../design/anchor/crates/sage_ir/crate/terminal_incompleteness.html) is not established uniformly: module expansion returns
  represented symbols while completeness is audited separately for particular
  consumers. The destination phase result makes terminal incompleteness
  explicit.
- Same-file unrelated body edits currently cause coarse module/derive and body
  reexecution; selected callee interfaces remain reusable. The
  [incrementality guide](../../design/crates/sage_ir/incrementality-guide.md) links the edit
  evidence.
- General method resolution, complete Typed IR coverage, source `cfg`
  evaluation, meaningful lifetime reasoning, and borrow checking are not yet
  implemented. Their local consequences are documented in the focused
  chapters and roadmap slices.

### Architecture: Related roadmap slices

The [Build-Out Roadmap](README.md) orders cross-cutting
semantic outcomes. The [Mini-redis Conformance
Roadmap](mini-redis.md) supplies the current application-scale
vertical slices.

## Body Checking and Typed-IR Elaboration

Former status section of [Body Checking and Typed-IR Elaboration](../../design/crates/sage_ir/bodies-guide.md).

### Body Checking and Typed-IR Elaboration: Current frontier

Body checking and elaboration pass exact oracle comparison for the pinned
mini-redis `DbDropGuard::db` and `Parse::next` bodies. Those slices cover local
field access, external trait dispatch, explicit adjustments, generic
obligations, and associated-type normalization. Numeric expression literals
now complete with concrete suffix or context-selected types, or with an error
type after one source-category diagnostic when their domain is incompatible or
remains unresolved. Non-lazy binary expressions now use the cached operator
trait method's real signature, ordinary local or `core` impl solving, and a
provisional resolved-call node tied to obligation recovery. Ground primitive,
comparison, and local user-defined operations complete through that path;
`&&` and `||` retain their direct lazy form.

### Body Checking and Typed-IR Elaboration: Implemented capabilities and evidence

- **{spec}`crates.sage_ir.checking.body.elaboration` — Simple field inference.** The [function-body
  walkthrough](../../design/examples/function-body.md) maps the input to a resolved field
  owner and final type.
- **{spec}`crates.sage_ir.checking.body.elaboration` — Elaborated external trait call.**
  `clone_method_call_is_elaborated_to_a_resolved_trait_call` directly inspects
  the completed Sage tree.
- **{spec}`crates.sage_ir.checking.body.elaboration` — Explicit never coercion.**
  `top_level_never_to_any_is_explicit_in_typed_ir` and
  `call_argument_never_to_any_is_explicit_in_typed_ir` inspect completed
  results for `NeverToAny` nodes at function-result and call-argument coercion
  sites; `external_inherent_call_accepts_a_never_argument` exercises the
  resolved-method path with rustc-provided metadata. The
  `basics/never_to_any.rs` oracle case independently emits function-result,
  direct-call, and struct-field trees from Sage and rustc and compares them by
  exact text.
- **{spec}`crates.sage_ir.checking.body.elaboration` — Numeric literal completion.**
  `suffixed_numeric_literals_have_declared_types` and
  `context_constrains_unsuffixed_numeric_literals_through_generic_calls`
  inspect concrete literal types in completed IR.
  `numeric_literal_domain_conflicts_report_once_and_recover`,
  `conflicting_numeric_domains_recover_both_classes`, and
  `domain_recovery_replays_compatible_structural_siblings` inspect precise
  conflict recovery without leaking inference indices.
  `unconstrained_numeric_literals_do_not_default` establishes error-typed
  terminal recovery without choosing a numeric default.
- **{spec}`crates.sage_ir.checking.body.elaboration` — Exact conformance.** The [oracle-checked method
  walkthrough](../../design/examples/oracle-checked-method.md) links the structural Sage
  assertion, independent emitters, exact shared snapshot, and byte identity.
- **{spec}`crates.sage_ir.checking.body.interface_dependencies` — Narrow reuse.**
  `clone_method_body_has_a_narrow_reusable_semantic_query_trace` checks the
  exact external interface reads, rejects all callee-body reads, and verifies
  that an unchanged second request executes neither the body query nor the
  metadata reads.
- **{spec}`crates.sage_ir.checking.body.interface_dependencies` — Current edit boundary.**
  `unrelated_body_edit_exposes_current_body_invalidation` edits a different
  function body and records that this body currently reexecutes, while its
  cached callee-interface metadata queries do not. The test does not assert
  whether module or derive-discovery queries reexecute.
- **{spec}`crates.sage_ir.checking.body.isolated_completion` — Transaction rollback.**
  `external_method_mismatch_discards_partial_generic_bindings` rejects a call
  after its first generic match and verifies that neither the binding nor the
  root semantic revision escapes the child transaction.
- **{spec}`crates.sage_ir.checking.body.isolated_completion` — Terminal obligations.**
  `generic_function_use_checks_its_instantiated_bounds`,
  `equivalent_obligations_deduplicate_after_inference`, and
  `struct_bounds_converge_after_field_inference` cover diagnostic publication,
  canonical deduplication, and an obligation resolved after type inference.
- **{spec}`crates.sage_ir.checking.body.expected_types` — Initial expected-type boundary.** Direct function
  calls inspect the callable signature before checking arguments and invoke
  `check_with_expected` with each available parameter type.
  `call_argument_never_to_any_is_explicit_in_typed_ir` demonstrates that this
  boundary applies a coercion and retains it in completed Typed IR, while
  `direct_call_rejects_too_few_arguments` and
  `direct_call_rejects_too_many_arguments` pin the signature's arity as part of
  the same boundary. `direct_call_checks_an_extra_argument_after_reporting_arity`
  shows that an arity mismatch does not suppress diagnostics within surplus
  argument expressions. Recursive consumption of the expectation by
  expression-specific rules is not yet implemented.
- **{spec}`crates.sage_ir.checking.body.operator_calls` — Ground operator vertical slice.**
  `all_operator_traits_and_methods_resolve_to_typed_symbols` covers all twelve
  trait identities and sixteen associated methods, including comparison
  methods which share one trait. The failure and cache tests cover external
  export identity, unavailable and wrong-kind results, narrow identity demand,
  and warm reuse. `primitive_operators_use_core_impls_and_resolved_calls`
  inspects resolved `Add::add` and mixed-integer `Shl::shl` calls and traces
  ordinary `core` impl headers and associated values on a cold request with no
  repeated metadata reads on the warm request.
  `comparisons_materialize_signature_required_shared_borrows` checks that
  `PartialEq::eq` and `PartialOrd::lt` use the shared borrows required by their
  actual signatures. `local_operator_impl_can_choose_distinct_rhs_and_output`
  checks a local `Left + Right -> Sum` impl, and
  `selected_operator_impl_narrows_unsuffixed_integer_literal` shows that
  narrowing comes from the selected `u32` impl rather than the `+` token.
  `failed_operator_proof_becomes_one_diagnostic_and_an_error_node` and
  `one_failed_operator_result_does_not_poison_an_equivalent_call` inspect
  obligation-linked recovery, while
  `lazy_boolean_operators_remain_control_flow_nodes` and
  `lazy_boolean_operators_reject_non_boolean_operands_without_trait_lookup`
  preserve the explicit boolean short-circuit path. The independent
  `oracle_projects_operators_to_the_same_typed_tree_as_sage` comparison covers
  these representative successful forms. Generic operator bodies,
  unknown-`Self` candidate hints, and the complete source/oracle matrix remain
  open, so `crates.sage_ir.checking.body.operator_calls` stays partial.

Run the review evidence with:

```bash
cargo test -p sage-test-harness clone_method_call_is_elaborated_to_a_resolved_trait_call
cargo test -p sage-test-harness clone_method_body_has_a_narrow_reusable_semantic_query_trace
cargo test -p sage-test-harness unrelated_body_edit_exposes_current_body_invalidation
cargo test -p sage-ir external_method_mismatch_discards_partial_generic_bindings
cargo test -p sage-ir well_known
cargo test -p sage-test-harness trait_obligation_tests
cargo test -p sage-oracle-harness --test binary_operators
cargo test --test type_check_tests
cargo test --test type_check_tests never
cargo test --test type_check_tests direct_call
cargo test -p sage-oracle-harness --test external_impls external_inherent_call_accepts_a_never_argument
cargo test -p sage-oracle-harness --test oracle_compare -- basics/never_to_any.rs
cargo test -p sage-oracle-harness --test oracle_compare -- mini_redis/db_drop_guard.rs
```

### Body Checking and Typed-IR Elaboration: Current limitations

- Rust expression, pattern, coercion, and method-resolution coverage is
  incomplete; diagnostics or recovery nodes mark unsupported paths.
- [KD-7](deviations.md#kd-7-arithmetic-operators-do-not-require-numeric-operands)
  remains open at the unproved parts of `crates.sage_ir.checking.body.operator_calls`. Ground primitive and local
  user-defined operations now prove their operator trait and complete as
  resolved calls, and invalid `bool + bool` recovers diagnostically. Generic
  bodies such as `T: Add<Output = T>` cannot yet use the associated-type
  equality assumption, and an unknown `Self` still lacks the general
  candidate-answer hints needed to accept cases such as `1 + 2u32` without
  defaulting or operator-specific rules. The full source and exact-oracle
  closure matrix is also not assembled.
- Expected-type checking currently synthesizes an expression before applying
  its coercion expectation. Only ordinary direct-call arguments use this
  boundary; literals, closures, nested expression forms, and method arguments
  do not yet consume expectations while they are checked.
- Lifetimes are always `Lifetime::Dummy`, and borrow checking is deliberately
  omitted. Reference/dereference nodes are structural only.
- Trait solving is recursive with current cycle/resource semantics; planned
  fairness and progress publication are not part of this phase result.
- General local/inherent method resolution, dynamic dispatch, GATs,
  specialization, and codegen method/vtable selection remain outside the
  completed slices.
- Query-trace evidence is currently test-asserted text rather than a
  user-facing Semantic Inspector report.
- `crates.sage_ir.checking.body.isolated_completion` has focused rollback and obligation-lifecycle evidence, but its full
  accepted-commit, wake-publication, and quiescence verification is not yet
  assembled as one review packet.
- An unrelated body edit in the same source file currently reexecutes the
  requested body query. The focused edit test above pins this gap from the
  destination while also verifying that cached callee interfaces remain
  reused. Expansion/discovery execution after that edit is not yet pinned by
  this evidence.

### Body Checking and Typed-IR Elaboration: Related roadmap slices

- [Solver recursion, scheduling, and monotone
  progress](README.md#planned-slice-solver-recursion-scheduling-and-monotone-progress)
  changes how obligations are searched without changing the successful Typed
  IR contract.
- [Shutdown::recv](README.md#next-application-slice-shutdownrecv)
  is the next body-coverage target.
- [Semantic inspector and persistent edit
  testing](README.md#implemented-slice-semantic-inspector-and-persistent-edit-testing)
  will expose readable bodies and structured query traces.
- [Numeric inference and operator
  soundness](README.md#next-correctness-slice-numeric-inference-and-operator-soundness)
  has established `crates.sage_ir.checking.body.operator_calls`'s ground-operation vertical slice; its generic,
  candidate-hint, and closure-evidence work will complete the anchor and close
  KD-7.

[numeric-rfd]: ../numeric-inference-variables/README.md
[rustc-operator-research]: ../../design/research/rustc-binary-operator-typing.md

## Shared Checking Design

Former status section of [Shared Checking Design](../../design/crates/sage_ir/checking-guide.md).

The single-keyed signature/body queries, two-stash boundary, transactional
inference, obligation finalization, elaborated output, and Dummy-lifetime
policy described above are implemented for the Rust subset recorded in the
two phase chapters. Their **Current Status** sections own concrete capability,
limitation, evidence, and roadmap information so this shared mechanism page
does not duplicate it.

Current evidence maps to the anchors as follows:

- **{spec}`crates.sage_ir.checking.symbol_keyed_interfaces`, {spec}`crates.sage_ir.checking.identity_minting`.** The [Signature Checking current
  status](coverage-history.md#signature-checking) links binder-identity,
  field-separation, and narrow external-dependency tests.
- **{spec}`crates.sage_ir.checking.owned_products`.** The [struct-signature](../../design/examples/struct-signature.md) and
  [function-body](../../design/examples/function-body.md) walkthroughs follow semantic
  values across fresh signature, field, and body output stashes.
- **{spec}`crates.sage_ir.checking.symbol_keyed_interfaces`, {spec}`crates.sage_ir.inference.terminal_obligations`.** The [Body Checking current
  status](coverage-history.md#body-checking-and-typed-ir-elaboration) links structural Typed-IR,
  terminal-obligation, exact-oracle, and cold/warm dependency evidence. A
  persistent same-file edit currently exposes a coarser invalidation boundary
  than `crates.sage_ir.checking.symbol_keyed_interfaces` requires.
- **{spec}`crates.sage_ir.inference.atomic_checking`.** `external_method_mismatch_discards_partial_generic_bindings`
  is the focused transaction test in `check/method.rs`; complete observable
  wake and staged-obligation evidence is not yet linked from a public review
  packet.

## Module and Macro Expansion

Former status section of [Module and Macro Expansion](../../design/crates/sage_ir/expansion-guide.md).

### Module and Macro Expansion: Current frontier

Local empty-input `macro_rules!` invocations, recursively generated items,
same-module macro-definition cycles, external bang procedural macros through
the metadata bridge, and selected built-in derives produce ordinary expanded
symbols with provenance. Expansion is tracked per local module.

### Module and Macro Expansion: Implemented capabilities and evidence

- **{spec}`crates.sage_ir.modules.expand.recursion_and_fixpoint` — Same-module fixed point.**
  `same_module_macro_resolution_reaches_a_fixed_point` exercises a macro that
  creates the definition needed by a later invocation. Run
  `cargo test -p sage-test-harness
  same_module_macro_resolution_reaches_a_fixed_point` and inspect the entry
  query and cycle-initial anchors above.
- **{spec}`crates.sage_ir.modules.expand.completeness` — Successful item expansion and exhaustive negative
  search.**
  `successful_item_macro_keeps_ground_negative_search_complete` shows that a
  single successfully represented item macro does not unnecessarily weaken a
  ground trait result. It does not exercise a nested emitted invocation; that
  part of `crates.sage_ir.modules.expand.recursion_and_fixpoint` remains unverified.
- **{spec}`crates.sage_ir.modules.expand.completeness` — Terminal incompleteness.**
  `unresolved_item_macro_prevents_ground_no`,
  `malformed_item_macro_output_prevents_ground_no_without_panicking`, and
  `recursive_item_macro_hits_expansion_limit_and_returns_maybe` show that
  unresolved input, invalid generated Rust, and resource exhaustion cannot
  create a false negative answer.
- **{spec}`crates.sage_ir.modules.expand.direct_members` — Generated identity across edits.**
  `moving_source_item_preserves_derive_expansion_identity` moves a source item
  and verifies stable expansion identity while its origin coordinate updates.
- **{spec}`crates.sage_ir.modules.expand.completeness` — Inspector directory completeness.**
  `retained_host_separates_edits_from_demand_and_preserves_symbol_paths`
  adds an unsupported but valid derive and requires the represented root's
  child status to become explicitly incomplete.

The compiler tests live in `crates/sage-test-harness/src/lib.rs`; the retained
directory/edit test lives in `tests/semantic_inspector.rs`. The Semantic
Inspector now supplies the human-readable persistent query trace for these
queries.

### Module and Macro Expansion: Review packet

| Question | Evidence |
|---|---|
| What Rust is expanded? | the fixed-point source under [Worked example](../../design/crates/sage_ir/expansion-guide.md#worked-example) |
| What readable output is expected? | the three-symbol direct-member sequence shown below that source |
| Is the represented result complete? | the fixed-point test requires `Generated`; the `successful_item_macro_keeps_ground_negative_search_complete` test establishes exhaustive downstream use |
| What happens on failure or a limit? | the unresolved, malformed-output, and recursive-limit tests listed above require conservative `Maybe` rather than false `No` |
| Which query runs? | `expanded_module_query_has_a_cold_and_warm_trace` requires the module query on a cold request and its reuse on an unchanged warm request |
| What happens after an edit? | `moving_source_item_preserves_derive_expansion_identity` checks offset movement; an unrelated-body edit trace for expansion itself is not yet asserted |
| Where does implementation detail begin? | the entry, cycle-initial, recursive expansion, and completeness anchors above |

Reproduce the focused packet with:

```bash
cargo test -p sage-test-harness same_module_macro_resolution_reaches_a_fixed_point
cargo test -p sage-test-harness expanded_module_query_has_a_cold_and_warm_trace
cargo test -p sage-test-harness item_macro
cargo test -p sage-test-harness moving_source_item_preserves_derive_expansion_identity
```

### Module and Macro Expansion: Current limitations

- `local_expanded_module_items` returns only `Vec<Symbol>`. Terminal
  incompleteness is represented by separate, consumer-specific audits for
  trait and method-provider discovery rather than one phase result.
- Local `macro_rules!` expansion currently supports only the first empty-input
  rule. Matcher parsing, metavariable substitution, hygiene, and general rule
  selection are not represented.
- General active attributes, including `cfg`, are not evaluated. An affected
  item is omitted from the definite represented set and completeness becomes
  conservative.
- Built-in derive coverage is partial, and general attribute/procedural derive
  expansion is not integrated.
- Existing generated-symbol evidence does not yet cover `crates.sage_ir.modules.expand.direct_members`'s complete
  direct-membership matrix for imports, inline children, recursive output, and
  derives in one review packet.
- `crates.sage_ir.modules.expand.recursion_and_fixpoint` has fixed-point success and recursive depth-limit evidence, but no
  successful fixture in which one expansion emits another invocation.
- Evidence covers stable generated identity and focused behavior, but not yet
  a structured edit/invalidation trace for the expanded-module query; `crates.sage_ir.modules.expand.dependencies`'s
  full positive and negative edit matrix remains prospective.

### Module and Macro Expansion: Related roadmap slices

- [Auditable architecture and review
  evidence](README.md#completed-slice-auditable-architecture-and-review-evidence)
  establishes this chapter and its review trail.
- [Semantic inspector and persistent edit
  testing](README.md#implemented-slice-semantic-inspector-and-persistent-edit-testing)
  will add readable expanded output and query traces across edits.
- [Trait-partitioned impl
  discovery](README.md#planned-slice-trait-partitioned-impl-discovery)
  consumes expansion completeness at a narrow candidate-discovery boundary.

## Incrementality and Query Boundaries

Former status section of [Incrementality and Query Boundaries](../../design/crates/sage_ir/incrementality-guide.md).

### Incrementality and Query Boundaries: Current frontier

Module, signature, body, external metadata, and solver operations are memoized
at semantic keys. Focused tests inspect warm reuse, selected external facts,
generated identity across movement, and narrow associated-value access.

### Incrementality and Query Boundaries: Implemented capabilities and evidence

- **[crates.sage_ir.crate.semantic_query_boundaries](../../design/anchor/crates/sage_ir/crate/semantic_query_boundaries.html)/[crates.sage_ir.crate.incremental_evidence](../../design/anchor/crates/sage_ir/crate/incremental_evidence.html):**
  `clone_method_body_has_a_narrow_reusable_semantic_query_trace` proves warm
  body reuse and rejects external callee-body reads.
- **[crates.sage_ir.crate.semantic_query_boundaries](../../design/anchor/crates/sage_ir/crate/semantic_query_boundaries.html)/[crates.sage_ir.crate.incremental_evidence](../../design/anchor/crates/sage_ir/crate/incremental_evidence.html):**
  `external_adt_default_and_predicate_have_one_narrow_reusable_dependency`
  proves one keyed external ADT dependency and warm reuse.
- **[crates.sage_ir.crate.semantic_query_boundaries](../../design/anchor/crates/sage_ir/crate/semantic_query_boundaries.html)/[crates.sage_ir.crate.incremental_evidence](../../design/anchor/crates/sage_ir/crate/incremental_evidence.html):**
  `local_normalization_reads_one_keyed_value_without_impl_item_enumeration`
  proves normalization reads one associated value through the canonical keyed
  item producer, reuses that producer after aggregate item enumeration, and
  does not depend on the aggregate item-list query. Editing only a sibling
  associated value revalidates the cheap producer while leaving the requested
  value query memoized.
- **[crates.sage_ir.crate.reexecution_and_invalidation](../../design/anchor/crates/sage_ir/crate/reexecution_and_invalidation.html):** `moving_source_item_preserves_derive_expansion_identity`
  distinguishes
  stable generated identity from its changing source coordinate.
- **[crates.sage_ir.crate.semantic_query_boundaries](../../design/anchor/crates/sage_ir/crate/semantic_query_boundaries.html)/[crates.sage_ir.crate.reexecution_and_invalidation](../../design/anchor/crates/sage_ir/crate/reexecution_and_invalidation.html)/[crates.sage_ir.crate.incremental_evidence](../../design/anchor/crates/sage_ir/crate/incremental_evidence.html):**
  `detail_edits_preserve_local_symbol_query_keys` and
  `enum_detail_edits_preserve_variant_query_keys` use persistent edits and
  Salsa observer queries to establish the CST/detail identity firewall across
  local item kinds; the variant case also consumes the revised self-contained
  CST so reuse cannot retain a pointer into an obsolete parent stash.
- **[crates.sage_ir.crate.reexecution_and_invalidation](../../design/anchor/crates/sage_ir/crate/reexecution_and_invalidation.html)/[crates.sage_ir.crate.incremental_evidence](../../design/anchor/crates/sage_ir/crate/incremental_evidence.html):**
  `moving_associated_item_reuses_method_signature_and_body` snapshots the
  work required after inserting a preceding associated method under both trait
  and impl owners and verifies that the unchanged target's signature and body
  do not execute. `moving_associated_items_preserves_symbol_query_keys` extends
  the symbol-key evidence across sibling insertion, removal, and reordering for
  every represented associated-item kind.
- **[crates.sage_ir.crate.reexecution_and_invalidation](../../design/anchor/crates/sage_ir/crate/reexecution_and_invalidation.html):**
  `unrelated_body_edit_exposes_current_body_invalidation` is negative evidence:
  it pins a coarse same-file invalidation that the destination should remove.

### Incrementality and Query Boundaries: Current limitations

- The local trait impl query is trait-keyed but still scans the complete local
  expanded-module vector; the private stable index/backdating firewall is not
  implemented.
- Same-file unrelated body edits currently reexecute a requested body even
  though selected callee-interface metadata is reused. The existing test does
  not assert expansion or derive-discovery execution after that edit.
- Query logs are debug strings and tests rather than a structured persistent
  event schema, so INC-A3 is established only for the focused normalized
  assertions above.
- Edit matrices cover selected semantic paths, not every symbol kind and
  query boundary required by INC-A1 and INC-A2.

### Incrementality and Query Boundaries: Related roadmap slices

- [Semantic inspector and persistent edit
  testing](README.md#implemented-slice-semantic-inspector-and-persistent-edit-testing)
  will provide a persistent database, structured traces, and edit commands.
- [Trait-partitioned impl
  discovery](README.md#planned-slice-trait-partitioned-impl-discovery)
  will implement the local index firewall and its invalidation matrix.

## Type Inference

Former status section of [Type Inference](../../design/crates/sage_ir/inference-guide.md).

### Type Inference: Current frontier

Type variables, transactional equality-class domains, structural equality, a
limited subtyping/coercion relation, transactional candidate work, wake-driven
obligation retries, error-sentinel recovery, and final quiescence support the
two completed mini-redis bodies. Numeric expression literals now use the domain
substrate: suffixes produce their declared primitive types, while unsuffixed
integer and float literals begin in `Integer` and `Float` classes and do not
default during finalization. Ground trait-first operator checking retains proof
and normalization obligations for every non-lazy token. A selected
operator impl can constrain an operand through the ordinary response-import
path, and a failed operator obligation replaces its provisional call with an
error node rather than leaving live inference work.

### Type Inference: Implemented capabilities and evidence

| Anchor | State | Implemented claim and evidence |
|---|---|---|
| {spec}`crates.sage_ir.inference.body_local_quiescence` | Partial | `body_finalization_resolves_types_into_a_fresh_stash` proves nested inference edges resolve into a separately owned output while the working stash is unchanged; `equivalent_obligations_deduplicate_after_inference` covers obligation quiescence after inputs narrow; `clone_method_call_is_elaborated_to_a_resolved_trait_call` inspects an inference-free completed body for the supported method slice. Direct negative completion tests for every live-state class are missing. |
| {spec}`crates.sage_ir.inference.transaction_isolation` | Partial | `canonical_equality_round_trips_transactionally` checks transactional response import, while `implication_assumptions_do_not_leak_to_siblings` verifies proof-branch isolation. Cancellation-specific isolation evidence is missing. |
| {spec}`crates.sage_ir.inference.canonical_boundary` | Partial | `canonical_equality_round_trips_transactionally`, `local_associated_type_normalization_produces_a_type_output`, and `response_local_type_output_round_trips_with_sharing` cover the implemented equality and type-output boundary; `conjunction_retries_after_a_sibling_pins_its_input` verifies retained work observes later hard information. No caller-side expected-type test yet proves that an expectation cannot select one of two otherwise ambiguous normalization candidates. |
| {spec}`crates.sage_ir.inference.completed_body_boundary` | Partial | `clone_method_body_has_a_narrow_reusable_semantic_query_trace` verifies warm reuse for one completed body slice; scheduler-order perturbation is not covered. |
| {spec}`crates.sage_ir.inference.subtyping_and_coercion` | Partial | The source-driven integrations `never_to_any_does_not_apply_beneath_mutable_reference`, `top_level_never_to_any_is_explicit_in_typed_ir`, `call_argument_never_to_any_is_explicit_in_typed_ir`, `struct_field_never_to_any_uses_the_value_span`, `external_inherent_call_accepts_a_never_argument`, `external_generic_method_defers_never_until_other_constraints_settle`, `never_result_does_not_create_a_never_to_never_coercion`, `inferred_never_join_does_not_retain_never_to_never_coercions`, `generic_call_arguments_are_order_independent_and_fall_back_to_never`, `never_fallback_wakes_suspended_method_lookup`, and `quiescent_never_fallback_does_not_settle_unrelated_targets` reject never-to-any beneath an invariant mutable reference; accept it at function-result, direct-call, struct-field, and resolved-method argument sites; preserve operand spans; exclude redundant `!`-to-`!` conversions for known and inferred targets; establish order-independent inference when a target is constrained by both divergence and another argument; and show that quiescent `!` fallback wakes a suspended consumer without prematurely settling unrelated targets. The `basics/never_to_any.rs` oracle case establishes exact Sage/rustc output for the function-result, direct-call, and struct-field conversions. Mutable-reference subtyping relates pointee types by equality. The complete subtype/coercion rule and layout matrix is not yet established. |
| {spec}`crates.sage_ir.inference.numeric_domains` | Partial | `variable_union_intersects_domains_in_both_orders`, `incompatible_transitive_domain_union_rolls_back`, `required_conflict_tracks_provisional_class_without_unvisited_siblings`, `numeric_domains_accept_members_and_reject_other_families`, `exact_bounds_join_the_validated_equality_class`, `domain_narrowing_follows_child_commit_and_discard`, and `reported_error_recovery_is_admitted_by_numeric_domains` cover the versioned equality and recovery substrate. `canonical_domains_distinguish_query_keys_and_reinstantiate`, `proof_narrowing_of_input_domain_round_trips_to_the_caller`, `response_local_domain_round_trips_to_the_caller`, `incompatible_domain_response_is_rejected_atomically`, `distinct_response_domains_do_not_subsume_each_other`, `merged_bare_hints_preserve_a_common_response_domain`, and `merged_nested_hints_preserve_a_common_response_domain` cover canonical identity, propagation, answer merging, and rollback. Source tests `suffixed_numeric_literals_have_declared_types`, `context_constrains_unsuffixed_numeric_literals_through_generic_calls`, `literal_domain_narrows_an_existing_generic_argument`, `literal_domain_filters_rigid_head_impl_candidates_after_later_narrowing`, `numeric_literal_domain_conflicts_report_once_and_recover`, `conflicting_numeric_domains_recover_both_classes`, `domain_recovery_replays_compatible_structural_siblings`, and `unconstrained_numeric_literals_do_not_default` establish expression-literal construction, ordinary generic/ADT propagation, a retained obligation which observes later narrowing, source-category recovery, and inference-free completed types. `selected_operator_impl_narrows_unsuffixed_integer_literal` adds proof-derived operator narrowing through the ordinary `u32: Add<u32>` response; `failed_operator_proof_becomes_one_diagnostic_and_an_error_node` checks terminal recovery for retained operator work. Explicit let annotations, numeric literal patterns, the complete where-clause/wake/isolation matrix, preservation of an ordinary `Any` operator operand before selection, generic operator assumptions, and unknown-`Self` candidate hints remain, so the anchor stays partial. |

### Type Inference: Current limitations

- Subtyping and coercions currently cover only a small structural subset.
- Numeric expression-literal construction distinguishes suffixes and
  integer/float domains, but explicit let annotations, numeric literal
  patterns, closures, casts, and control-flow joins remain incomplete relative
  to Rust. Ground overloadable operations prove their operator traits and
  complete as resolved calls, but generic operator assumptions and inference
  hints from a complete unknown-`Self` candidate set are not yet represented.
  [KD-7](deviations.md#kd-7-arithmetic-operators-do-not-require-numeric-operands)
  therefore remains open.
- All lifetime syntax becomes `Lifetime::Dummy`; no lifetime variables or
  borrow checks are performed.
- The executor is single-threaded and cooperative; planned solver fairness and
  concurrent conjunction work are not implemented.
- Failed bodies recover with error types and diagnostics, not a structured
  phase-incompleteness taxonomy.
- `crates.sage_ir.inference.canonical_boundary`'s source boundary imports normalization output before relating the
  caller expectation, but the required caller-side candidate-isolation
  regression test is missing.

### Type Inference: Related roadmap slices

[Solver recursion, scheduling, and monotone
progress](README.md#planned-slice-solver-recursion-scheduling-and-monotone-progress)
will change task progress and wake behavior. [Shutdown::recv](README.md#next-application-slice-shutdownrecv)
will extend body-level inference coverage. [Numeric inference and operator
soundness](README.md#next-correctness-slice-numeric-inference-and-operator-soundness)
has established `crates.sage_ir.inference.numeric_domains`'s transactional substrate and expression-literal
checkpoint plus proof-derived narrowing in the first operator vertical slice.
Its remaining propagation, generic/candidate-hint, and closure-evidence
checkpoints will complete {spec}`crates.sage_ir.inference.numeric_domains` and close KD-7.

[numeric-rfd]: ../numeric-inference-variables/README.md

## External Metadata

Former status section of [External Metadata](../../design/crates/sage_ir/metadata-guide.md).

### External Metadata: Current frontier

External module/name lookup, traits, associated items, cached well-known
operator trait and method identities, function methods, ADT
generics/defaults/predicates, inherent method candidates, relevant trait impls,
impl headers, associated type values, selected structural ADT properties, and
macro expansion facts support the implemented slices. Relevant-impl lookup
also represents the coarse local-nominal self head and reports explicit binary
operator impl sets as complete where rustc has no separate built-in candidate
source. Float self heads retain their width: represented `f32` and `f64`
queries can therefore exclude rustc's explicit `f16` and `f128` impls without
trying to lower their unsupported signatures or weakening completeness.

### External Metadata: Implemented capabilities and evidence

| Anchor | State | Implemented claim and evidence |
|---|---|---|
| {spec}`crates.sage_ir.metadata.provider_authority` | Partial | The `architecture_external_metadata_interface` source anchor exposes typed owned facts rather than solver verdicts; `external_clone_contract_proves_the_derived_impl` exercises Sage proof after importing external data. A dedicated interface-surface audit is not yet checked in. |
| {spec}`crates.sage_ir.metadata.explicit_incompleteness` | Partial | `external_adt_missing_required_argument_is_not_inferred` and `unsupported_external_default_is_metadata_unavailable_not_an_arity_error` preserve two unavailable-metadata cases without inventing defaults; the full boundary matrix is not present. |
| {spec}`crates.sage_ir.metadata.product_boundaries` | Partial | `external_clone_items_and_method_signature_are_typed_metadata` verifies separate trait-item enumeration and selected method-signature lowering. `operator_identity_queries_are_layered_cached_and_narrow` verifies that operator-method identity reads associated items and names without loading trait or function signatures or impls. `primitive_operators_use_core_impls_and_resolved_calls` then traces the independently demanded method signature, relevant impl identities, matching headers, and associated output values for ground `Add` and `Shl` bodies. `represented_float_operator_impls_resolve` checks the separate `Float32` and `Float64` keys, and `local_operator_impl_can_choose_distinct_rhs_and_output` observes the `LocalAdt` partition while Sage selects a local impl. `clone_method_body_has_a_narrow_reusable_semantic_query_trace` rejects callee-body reads, and `local_normalization_reads_one_keyed_value_without_impl_item_enumeration` covers keyed local associated-value demand. The full external product/isolation matrix is not assembled. |
| {spec}`crates.sage_ir.metadata.keyed_reuse` | Implemented for selected products | `clone_method_body_has_a_narrow_reusable_semantic_query_trace` pins cold calls and warm reuse for the method slice. `external_adt_default_and_predicate_have_one_narrow_reusable_dependency` pins one ADT-signature read and warm reuse. `operator_identity_queries_are_layered_cached_and_narrow` separately pins layered trait/method lookup and a metadata-free warm repeat. `primitive_operators_use_core_impls_and_resolved_calls` extends cold/warm reuse evidence through the operator method-signature, impl-header, and associated-value products. |

### External Metadata: Current limitations

- The raw model covers only the Rust definition kinds and semantic details
  required by implemented slices.
- Unsupported defaults, const identities, predicates, receivers, or impl
  headers become unavailable/ineligible metadata rather than full Sage types.
- `crates.sage_ir.metadata.provider_authority` lacks a mechanical interface-surface check, `crates.sage_ir.metadata.explicit_incompleteness` lacks a complete
  unavailable-metadata matrix, and `crates.sage_ir.metadata.product_boundaries` lacks a focused external
  associated-value trace.
- External module children are broader than several desired semantic lookup
  keys; finer queries should be introduced when evidence shows unnecessary
  invalidation.
- `LocalAdt` is intentionally a coarse safe partition. Finer local nominal
  self-head keys and their full indexed-versus-exhaustive comparison remain
  part of trait-partitioned impl-discovery work.
- Float widths are intentionally finer than the other primitive families.
  Sage represents `f32` and `f64`, while the metadata provider classifies
  `f16` and `f128` as distinct rigid heads so their unsupported headers are
  provably irrelevant to represented float queries.
- The production frontend remains coupled to rustc's unstable private APIs
  through its driver implementation, though `TcxDb` isolates that coupling.

### External Metadata: Related roadmap slices

- [Trait-partitioned impl
  discovery](README.md#planned-slice-trait-partitioned-impl-discovery)
  aligns the local boundary with the existing external trait/head key.
- [Semantic inspector and persistent edit
  testing](README.md#implemented-slice-semantic-inspector-and-persistent-edit-testing)
  presents metadata calls alongside Salsa execution traces.
- [Shutdown::recv](README.md#next-application-slice-shutdownrecv)
  will add only the metadata forms required by that vertical slice.
- [Numeric inference and operator
  soundness](README.md#next-correctness-slice-numeric-inference-and-operator-soundness)
  uses cached well-known-item queries over ordinary path and associated-item
  resolution. Its first trait-first vertical slice traces ground primitive
  `core` impl discovery and local-nominal partitioning through this boundary;
  the generic and closure matrices remain open.

## Parsing and Stable Symbol Creation

Former status section of [Parsing and Stable Symbol Creation](../../design/crates/sage_ir/parsing-guide.md).

### Parsing and Stable Symbol Creation: Current frontier

The parser represents the item, signature, expression, pattern, generic, and
attribute forms needed by the completed mini-redis slices and the existing
unit suite. Source-written and generated text enter the same symbol/CST model.

### Parsing and Stable Symbol Creation: Implemented capabilities and evidence

- **{spec}`crates.sage_ir.modules.parse.syntax_identities`, {spec}`crates.sage_ir.modules.parse.detail_firewall` — Per-item CST and later
  parameter identity.** The [struct-signature
  walkthrough](../../design/examples/struct-signature.md) shows the parsed stash and the
  separate checked signature.
- **{spec}`crates.sage_ir.modules.parse.syntax_identities` — File-module scope.**
  `items_in_a_file_backed_module_use_that_module_as_their_scope` verifies that
  a child file's item is owned by the child module rather than the crate root.
- **{spec}`crates.sage_ir.modules.parse.syntax_identities` — Generated-source parsing.** The [macro-generated struct
  walkthrough](../../design/examples/macro-generated-struct.md) follows macro output
  parsed into an ordinary struct symbol.
- **{spec}`crates.sage_ir.modules.parse.detail_firewall` — Offset-insensitive generated identity.**
  `moving_source_item_preserves_derive_expansion_identity` verifies stable
  generated identity when source text moves.
- **{spec}`crates.sage_ir.modules.parse.detail_firewall` — Per-kind CST/detail firewall.**
  `detail_edits_preserve_local_symbol_query_keys` applies persistent edits to
  every top-level local item shape and proves through a Salsa observer query
  that CST detail does not replace the symbol. The companion
  `enum_detail_edits_preserve_variant_query_keys` covers variants and their
  constructors, including consumption of the revised variant CST after the
  edit. `moving_associated_items_preserves_symbol_query_keys` covers all three
  associated item kinds through both the trait and impl parsing paths while
  inserting, removing, and reordering siblings.

Run the focused ownership test with:

```bash
cargo test -p sage-test-harness items_in_a_file_backed_module_use_that_module_as_their_scope
```

### Parsing and Stable Symbol Creation: Current limitations

- Rust grammar coverage is incomplete; unsupported syntax can become explicit
  error nodes even when rustc accepts it.
- Tree-sitter recovery is not yet exposed as a structured phase-completeness
  result or a complete diagnostic set; `crates.sage_ir.modules.parse.recovery_completeness` is therefore not established as
  one unified phase contract.
- Local symbol stability has a per-kind detail-edit matrix and associated-item
  insertion, deletion, and reordering coverage, but those structural edits and
  genuine-identity edits are not yet covered for every top-level item kind, so
  the full `crates.sage_ir.modules.parse.detail_firewall` matrix remains prospective.
- There is not yet a parse-only cold/warm query trace establishing `crates.sage_ir.modules.parse.dependencies`'s
  complete positive and negative dependency set.
- Signature parsing retains lifetime syntax, but later checking currently
  lowers every lifetime to `Lifetime::Dummy`.

### Parsing and Stable Symbol Creation: Related roadmap slices

[Semantic inspector and persistent edit
testing](README.md#implemented-slice-semantic-inspector-and-persistent-edit-testing)
will expose parsed symbols and edit-invalidation evidence. Future mini-redis
application slices extend grammar coverage only as required by an accepted
vertical target.

## Name Resolution

Former status section of [Name Resolution](../../design/crates/sage_ir/resolution-guide.md).

### Name Resolution: Current frontier

Normal type/value lookup, lexical generic/local lookup, local and external
module paths, named imports, transitive globs with cycle termination, edition
prelude traits, and the method-scope subset needed by the completed mini-redis
slices are operational.

### Name Resolution: Implemented capabilities and evidence

| Anchor | State | Implemented claim and evidence |
|---|---|---|
| {spec}`crates.sage_ir.resolution.lookup.completeness` | Partial | `unresolved_item_macro_keeps_method_scope_incomplete` verifies that partial expansion cannot justify exhaustive provider selection; the complete absent/ambiguous/incomplete matrix is not present. |
| {spec}`crates.sage_ir.resolution.lookup.precedence` | Partial | `transitive_local_globs_resolve_and_glob_cycles_terminate` covers a transitive reexport and a glob cycle; `a_trait_from_another_editions_prelude_is_not_a_provider` and `an_explicitly_imported_renamed_external_trait_is_a_provider` cover edition-specific preludes and renamed imports. Lexical and namespace coverage remains incomplete. |
| {spec}`crates.sage_ir.resolution.lookup.identity_only` | Not separately established | The `architecture_resolve_path` source anchor shows the identity lookup boundary, but no focused normalized trace proves the complete forbidden-dependency set. |
| {spec}`crates.sage_ir.resolution.lookup.dependencies` | Not separately established | Existing body traces reject selected callee-body reads, but no resolution-focused cold/warm trace covers the full lookup footprint and edit behavior. |

### Name Resolution: Current limitations

- Qualified lookup through a type, such as general `T::Item`, is not integrated
  into `resolve_path`; it requires type/trait-directed associated lookup.
- Local glob-provider completeness is conservative because all recursive
  export edges are not yet represented as an exhaustive indexed result.
- General visibility, hygiene, active attributes, macro rules, and Rust's full
  namespace edge cases are incomplete.
- Resolution results and completeness do not yet have a standalone readable
  Semantic Inspector view.

### Name Resolution: Related roadmap slices

[Semantic inspector and persistent edit
testing](README.md#implemented-slice-semantic-inspector-and-persistent-edit-testing)
will expose path results and their query trail. [Shutdown::recv](README.md#next-application-slice-shutdownrecv)
is the next application slice likely to extend method and associated lookup.

## Signature Checking

Former status section of [Signature Checking](../../design/crates/sage_ir/signatures-guide.md).

### Signature Checking: Current frontier

The represented local item kinds and external interfaces required by the
completed `DbDropGuard::db` and `Parse::next` slices have checked,
symbol-keyed signatures. Associated items reuse owner binders; local and
external predicates needed by those slices are retained.

### Signature Checking: Implemented capabilities and evidence

- **{spec}`crates.sage_ir.checking.signature.owner_binder`, {spec}`crates.sage_ir.checking.signature.storage`, {spec}`crates.sage_ir.checking.signature.narrow_detail` — Binder
  identity and field separation.** The [struct-signature
  walkthrough](../../design/examples/struct-signature.md) follows `sig` and `fields`
  through separate stashes.
- **{spec}`crates.sage_ir.checking.signature.owner_binder` — Owner and method generics.**
  `trait_and_impl_signatures_share_complete_generic_binders` and
  `associated_items_reuse_owner_generics_and_bodies_resolve_fields` verify
  owner-linked identities.
- **{spec}`crates.sage_ir.checking.signature.body_independence` — Parameter environments.**
  `function_and_adt_signatures_retain_parameter_environments` checks retained
  bounds on callable and ADT interfaces.
- **{spec}`crates.sage_ir.checking.signature.body_independence` — Narrow reusable external dependency.**
  `external_adt_default_and_predicate_have_one_narrow_reusable_dependency`
  inspects cold and warm query/metadata logs.

### Signature Checking: Current limitations

- Rust signature coverage and external definition-kind coverage are partial.
- Local `Check` collects signature diagnostics internally, but the current
  `Stashed` signature results do not publish that diagnostic vector as part of
  a unified phase result.
- Existing query evidence covers selected external dependencies, but not yet
  `crates.sage_ir.checking.signature.body_independence`'s full local body-edit matrix or `crates.sage_ir.checking.signature.narrow_detail`'s unselected-sibling matrix.
- The two-stash walkthroughs support `crates.sage_ir.checking.signature.storage`, but there is not yet an automated
  structural test covering every signature and alias form for cross-stash
  references.
- Every lifetime form lowers to `Lifetime::Dummy`; borrow checking is omitted.
- Some unsupported predicates are retained only as solver-ineligible
  environments, preventing definite trait conclusions.
- Const-call completeness and several advanced forms, including GATs and
  general opaque normalization, are not implemented.

### Signature Checking: Related roadmap slices

- [Trait-partitioned impl
  discovery](README.md#planned-slice-trait-partitioned-impl-discovery)
  will narrow a major signature-to-solver dependency.
- [Shutdown::recv](README.md#next-application-slice-shutdownrecv)
  is the next application slice expected to extend interface coverage.
- [Semantic inspector and persistent edit
  testing](README.md#implemented-slice-semantic-inspector-and-persistent-edit-testing)
  will make signature output and edit behavior directly inspectable.

## Trait Solver Design

Former status section of [Trait Solver Design](../../design/crates/sage_ir/solver-guide.md).

### Trait Solver Design: Current frontier

The solver performs positive local and reachable-external explicit-impl proof,
associated-type normalization with a type output, isolated asynchronous
candidate evaluation, order-independent completed-answer reduction, final hard
hints, conservative candidate-source completeness, a provisional inductive
cycle cutoff, and a proof-depth limit. Operator checking now exercises this
same path for ground primitive and local user-defined implementations. For a
local nominal `Self`, the external query uses a coarse `LocalAdt` head: rigid
upstream nominal impls cannot match, while blanket and otherwise unclassifiable
upstream impls remain candidates.

### Trait Solver Design: Implemented capabilities and evidence

| Anchor | State | Implemented claim and evidence |
|---|---|---|
| {spec}`crates.sage_ir.solver.normalization_demand` | Partial | `aliases_round_trip_through_canonical_query_and_response_stashes` preserves the alias family, and `local_normalization_reads_one_keyed_value_without_impl_item_enumeration` verifies a narrow associated-value read; named expansion and opaque reveal are not implemented. |
| {spec}`crates.sage_ir.solver.bounded_completeness` | Partial | `unresolved_item_macro_prevents_ground_no`, `unresolved_trait_impl_head_prevents_ground_no`, and `proof_depth_limit_is_maybe` establish that the implemented incomplete/depth cases do not manufacture `No`; the destination groundness and resource matrix is not built. |
| {spec}`crates.sage_ir.solver.canonical_outputs` | Partial | `canonical_response_rejects_an_output_for_the_wrong_operation`, `local_associated_type_normalization_produces_a_type_output`, `response_local_type_output_round_trips_with_sharing`, and `incompatible_normalization_outputs_are_ambiguous_but_identical_outputs_merge` exercise operation pairing, response binding, and output-aware merging. The incompatible-output test constructs `SolverGoal::Normalize` directly, so it does not establish caller expected-type isolation. |
| {spec}`crates.sage_ir.solver.candidate_completeness` | Partial | `generic_impl_where_clause_is_proved_by_nested_impl` covers nested local candidate obligations. The `Parse::next` evidence in the [Mini-redis roadmap](mini-redis.md#slice-2-parsenext) covers trait/head-keyed reachable-external discovery and warm reuse. `primitive_operators_use_core_impls_and_resolved_calls` traces relevant `core` impl identities, headers, and associated values for `Add` and mixed-integer `Shl`, then observes warm reuse. `local_operator_impl_can_choose_distinct_rhs_and_output` exercises the conservative `LocalAdt` external partition while selecting a local impl. The local index remains provisional, and the full indexed-versus-exhaustive matrix is not assembled. |
| {spec}`crates.sage_ir.solver.local_index_firewall` | Not implemented | No current evidence claims the destination private local index or its edit-invalidation firewall. |
| {spec}`crates.sage_ir.solver.alternative_isolation` | Partial | `incompatible_normalization_outputs_are_ambiguous_but_identical_outputs_merge` and the `final_answer_rules_are_order_independent` unit test cover completed answer reduction; concurrent conjunction and cancellation isolation remain planned. |
| {spec}`crates.sage_ir.solver.shared_final_result` | Partial | `same_parent_requests_share_one_live_producer_and_result` verifies shared producer identity and one equal final response; bounded polling rounds and monotone intermediate publication have no current evidence. |
| {spec}`crates.sage_ir.solver.canonical_cycles` | Partial | `inductive_impl_cycle_is_no` and `proof_depth_limit_is_maybe` cover the provisional inductive cutoff and depth result only, not the destination path-kind/fixpoint semantics. |
| {spec}`crates.sage_ir.solver.schedule_independence` | Partial | `final_answer_rules_are_order_independent` covers reducer input order; scheduler perturbation and deterministic resource accounting are not implemented. |

### Trait Solver Design: Current limitations

- Local trait-keyed discovery is a provisional linear scan over expanded local
  impl identities; the stable private index and edit-invalidation firewall are
  not built.
- Groundness-sensitive ambiguity causes, term-size limits, deterministic work
  limits, polling rounds, and intentional yield points are not implemented.
- Conjunctions are sequential. Candidate and frame progress envelopes are not
  published.
- Cycle handling uses parent-chain inductive cutoff and depth overflow rather
  than the destination provisional fixpoint/coinductive path semantics.
- General negative reasoning, specialization, GATs, named-alias expansion,
  opaque reveal, and method/vtable output operations are outside the current
  frontier.
- Generic operator bodies which rely on an associated-type equality such as
  `T: Add<Output = T>` are not yet provable because that assumption is not
  represented in the solver environment. Unknown-`Self` operator goals also
  lack a general complete-answer hint mechanism; neither gap is filled by
  operator-specific candidates.
- `crates.sage_ir.solver.canonical_outputs` lacks an integration test in which the caller expects one of two
  incompatible normalization outputs and the solver nevertheless preserves
  ambiguity.

### Trait Solver Design: Related roadmap slices

- [Trait-partitioned impl
  discovery](README.md#planned-slice-trait-partitioned-impl-discovery)
  owns the local incremental firewall.
- [Solver recursion, scheduling, and monotone
  progress](README.md#planned-slice-solver-recursion-scheduling-and-monotone-progress)
  coordinates the [Cycle Semantics](../trait-solver-cycle-semantics/README.md),
  [Scheduling and Fairness](../trait-solver-scheduling/README.md), and
  [Incremental Results](../incremental-trait-results/README.md) RFDs.

## Spans (`span.rs`)

Former status section of [Spans (`span.rs`)](../../design/crates/sage_ir/spans-guide.md).

### Spans (`span.rs`): Current frontier and evidence

Real source files, bang-macro output, and derive output carry distinct
`ParseSource` identities. Source-root CST and Typed IR use relative spans
within an item; diagnostics resolve them through the current absolute item
span.
`moving_source_item_preserves_derive_expansion_identity` verifies that moving
a derived item updates its origin coordinates without reminting the expansion,
and `duplicate_derive_occurrences_have_distinct_generated_source_identity`
verifies occurrence identity. This establishes the exercised generated-source
portion of {spec}`crates.sage_ir.types.spans.generated_identity` and supplies one movement case for
{spec}`crates.sage_ir.types.spans.offset_reuse`.
`moving_associated_items_preserves_symbol_query_keys` covers associated
functions, types, and constants under both trait and impl owners across actual
sibling insertion, removal, and reordering, while
`moving_associated_item_reuses_method_signature_and_body` snapshots the
post-edit Salsa executions for both owner paths and verifies that the unchanged
method signature and body remain memoized. Together these establish the
exercised associated item portion of {spec}`crates.sage_ir.types.spans.relative_composition` and
{spec}`crates.sage_ir.types.spans.offset_reuse`.

### Spans (`span.rs`): Current limitations

- Generated-source diagnostics have less source-snippet support than real
  files, and tooling does not yet expose a provenance backtrace.
- Identity/edit evidence covers associated items and selected generated moves
  rather than every local item and nested expansion shape required by {spec}`crates.sage_ir.types.spans.generated_identity`
  and {spec}`crates.sage_ir.types.spans.offset_reuse`.
- The exhaustive parsing and resolution matrix required by {spec}`crates.sage_ir.types.spans.relative_composition` is not yet
  collected as a chapter-level review packet.
- Lifetime `Dummy` is a semantic type representation and is unrelated to span
  provenance; meaningful lifetime tracking remains deferred.

### Spans (`span.rs`): Related roadmap slices

The [Semantic Inspector and persistent edit-testing
slice](README.md#implemented-slice-semantic-inspector-and-persistent-edit-testing)
will expose source/expansion provenance and identity changes across edits.

## Symbols and Semantic Identity

Former status section of [Symbols and Semantic Identity](../../design/crates/sage_ir/symbols-guide.md).

### Symbols and Semantic Identity: Current frontier

Local and external item identities, kind-specific wrappers, associated-item
ownership, generic parameter identities, and symbol references in checked
types and the implemented Typed IR slices are operational.

### Symbols and Semantic Identity: Implemented capabilities and evidence

- **{spec}`crates.sage_ir.symbols.local_identity` — Generated-source identity.** The test
  `moving_source_item_preserves_derive_expansion_identity` moves a derived
  source item, verifies that its expansion identity is unchanged, and verifies
  that the updated origin coordinate remains observable.
- **{spec}`crates.sage_ir.symbols.local_identity` — Distinct generated occurrences.** The test
  `duplicate_derive_occurrences_have_distinct_generated_source_identity`
  verifies that two derive occurrences on one item do not collapse to one
  generated source identity.
- **{spec}`crates.sage_ir.symbols.local_identity` — Local detail-edit matrix.**
  `detail_edits_preserve_local_symbol_query_keys` verifies the top-level item
  family, `enum_detail_edits_preserve_variant_query_keys` verifies variants
  and constructors while reading the updated self-contained variant CST, and
  `moving_associated_items_preserves_symbol_query_keys` verifies associated
  functions, types, and constants under both trait and impl owners across
  insertion, removal, and reordering of siblings.
- **{spec}`crates.sage_ir.symbols.local_identity` — One impl-item producer.**
  `local_normalization_reads_one_keyed_value_without_impl_item_enumeration`
  verifies that focused normalization and aggregate impl-item enumeration use
  the same per-item producer query without making normalization depend on the
  aggregate item list; its edit trace also separates cheap producer
  revalidation from requested-value reuse when a sibling value changes.
- **{spec}`crates.sage_ir.symbols.associated_identity` — Associated ownership.** The test
  `trait_items_have_stable_symbols_and_owner_identity` checks function, type,
  and const item ownership and verifies that an associated method reuses its
  trait's generic identities.
- **{spec}`crates.sage_ir.symbols.definition_identity`/{spec}`crates.sage_ir.symbols.kind_wrappers` — Resolved output.** The
  [function body example](../../design/examples/function-body.md)
  inspects a resolved field owner in Typed IR, and the [oracle-checked method
  example](../../design/examples/oracle-checked-method.md) inspects external definition
  identities in exact conformance output.
- **{spec}`crates.sage_ir.symbols.kind_wrappers` — Typed operator identities.**
  `all_operator_traits_and_methods_resolve_to_typed_symbols` verifies that
  fixed operator paths and associated-item names produce `TraitSymbol` and
  `FnSymbol` results, while the failure tests reject wrong-kind definitions.

The implementation entry points are the anchored local, external, and wrapper
definitions above. Run the focused harness evidence with:

```bash
cargo test -p sage-test-harness moving_source_item_preserves_derive_expansion_identity
cargo test -p sage-test-harness trait_items_have_stable_symbols_and_owner_identity
cargo test -p sage-test-harness detail_edits_preserve_local_symbol_query_keys
cargo test -p sage-test-harness moving_associated_items_preserves_symbol_query_keys
cargo test -p sage-ir well_known
```

### Symbols and Semantic Identity: Current limitations

- Local tracked identities are based on the identity fields and creation
  structure used by the current parser/lowering queries. Evidence covers the
  per-kind detail-edit matrix, offset movement, and associated-item sibling
  insertion, deletion, and reordering, but not those structural edits for
  every top-level item kind required by {spec}`crates.sage_ir.symbols.local_identity`.
- `SymExt` interns the carried `kind` alongside crate number and definition
  index even though the logical definition identity is the latter pair; the
  representation therefore does not yet make the {spec}`crates.sage_ir.symbols.external_identity` split exact.
- Current tests exercise several wrapper conversions and external queries but
  do not yet provide the complete compile-time conversion matrix required by
  {spec}`crates.sage_ir.symbols.kind_wrappers` or the external operation matrix required by {spec}`crates.sage_ir.symbols.external_fact_queries`.
- Current resolved-output examples show that definitions are recorded by
  symbol, but do not establish {spec}`crates.sage_ir.symbols.definition_identity`'s distinct-path/same-symbol matrix.
- The associated-ownership test establishes owner and generic identity but
  does not yet include {spec}`crates.sage_ir.symbols.associated_interfaces`'s sibling-body dependency assertion.
- Some fine-grained identities are owner-relative rather than global symbols:
  fields use owner plus index and locals use body-local IDs. The architecture
  does not yet promise stable field/local identity across structural edits.
- Unsupported external definition kinds can be retained as raw metadata
  children but cannot all be converted to a typed `SymbolData` variant.

### Symbols and Semantic Identity: Related roadmap slices

- The [Semantic Inspector](../../design/crates/sage_inspector/inspector-guide.md) exposes stable
  display paths and readable identity-bearing semantic output.
- The [Build-Out Roadmap](README.md) tracks vertical
  slices that extend the set of symbol kinds and semantic queries exercised in
  completed bodies.

## Typed IR

Former status section of [Typed IR](../../design/crates/sage_ir/typed-ir-guide.md).

### Typed IR: Current frontier

The `DbDropGuard::db` and `Parse::next` mini-redis slices produce explicit
field identities, dereferences, `Dummy` borrows, selected functions, static or
direct dispatch, owner/method substitutions, and associated-type normalization
results. Never-to-any coercion is represented by an explicit `NeverToAny`
expression node. The common rigid/alias family survives inference, canonical
solver boundaries, display, and both semantic emitters. Ground non-lazy binary
operations over primitive and local user-defined operands now produce the same
static-trait `ResolvedCall` representation, while lazy boolean operations
remain source-shaped control-flow nodes.

### Typed IR: Implemented capabilities and evidence

- **{spec}`crates.sage_ir.types.body.typed_provenance`:** [Function body and field access](../../design/examples/function-body.md)
  inspects a
  resolved local field and substituted final type.
- **{spec}`crates.sage_ir.types.body.typed_provenance`/{spec}`crates.sage_ir.types.calls.selected_semantics`:** [An oracle-checked method
  body](../../design/examples/oracle-checked-method.md) inspects
  the resolved `Clone::clone` tree and its exact checked-in snapshot.
- **{spec}`crates.sage_ir.types.body.typed_provenance`/{spec}`crates.sage_ir.types.calls.selected_semantics`:** The `Parse::next` evidence in the [Mini-redis
  roadmap](mini-redis.md#slice-2-parsenext) checks direct and
  static-trait calls, substitutions, and normalized iterator item type.
- **{spec}`crates.sage_ir.types.body.typed_provenance`:** `top_level_never_to_any_is_explicit_in_typed_ir`
  and `call_argument_never_to_any_is_explicit_in_typed_ir` inspect
  source-driven completed bodies whose `NeverToAny` nodes have a `!` operand
  and `u32` result. `struct_field_never_to_any_uses_the_value_span` additionally
  verifies that the node retains the value expression's span. The
  `basics/never_to_any.rs` oracle case compares the independently emitted Sage
  and rustc function-result, direct-call, and struct-field trees exactly.
- **{spec}`crates.sage_ir.types.aliases.identity`:** `alias_variants_copy_fold_and_display_without_erasing_identity` and
  `aliases_round_trip_through_canonical_query_and_response_stashes` exercise
  the shared alias representation.
- **{spec}`crates.sage_ir.symbols.body_interfaces`:** `clone_method_body_has_a_narrow_reusable_semantic_query_trace`
  rejects callee-body reads and proves warm reuse for the pinned method body.
- **{spec}`crates.sage_ir.types.operators.trait_calls`, partial:**
  `primitive_operators_use_core_impls_and_resolved_calls` inspects `Add::add`
  and mixed-integer `Shl::shl` as static trait calls with their owner
  substitutions and final result types.
  `comparisons_materialize_signature_required_shared_borrows` verifies the
  explicit shared-reference operands required by `PartialEq` and `PartialOrd`,
  and `local_operator_impl_can_choose_distinct_rhs_and_output` checks a local
  `Left + Right -> Sum` call. Failed proof and normalization obligations replace
  their provisional call with a diagnosed error node, and
  `lazy_boolean_operators_remain_control_flow_nodes` checks that `&&` bypasses
  trait-call elaboration. `oracle_projects_operators_to_the_same_typed_tree_as_sage`
  compares the representative primitive, comparison, local user-defined, and
  lazy forms exactly with rustc. Generic calls and the complete operator/oracle
  matrix are still missing, so this evidence does not establish {spec}`crates.sage_ir.types.operators.trait_calls` in full.

### Typed IR: Current limitations

- `TyExprData` still contains source-shaped variants for constructs whose
  general elaboration is not implemented. Such variants are not permitted in
  a successful oracle slice that claims their semantics.
- Closures/captures, async/coroutine bodies, indexing, callable dispatch,
  general coercions/unsizing, `for`, `?`, and fully elaborated `.await` remain
  planned. Operator trait calls are built for the ground primitive and local
  user-defined slice, but generic operator bodies and the full source/operator
  matrix remain incomplete.
- Method resolution is limited to the conservative external trait/inherent
  slices and selected local obligations, so {spec}`crates.sage_ir.types.calls.dispatch_families`'s full dispatch-family
  matrix is not yet established.
- Named-alias expansion and opaque reveal are planned; associated projection
  normalization covers the pinned first slice rather than GATs in general, so
  {spec}`crates.sage_ir.types.aliases.revelation`'s reveal-boundary matrix remains incomplete.
- `Lifetime::Dummy` and the absence of borrow checking remain the deliberate
  temporary soundness hole described above. The exhaustive lifetime-origin
  evidence required by {spec}`crates.sage_ir.types.lifetimes.dummy` is not yet assembled.
- {spec}`crates.sage_ir.symbols.body_interfaces` has focused no-callee-body evidence but not yet the complete
  callee-body edit matrix.

### Typed IR: Related roadmap slices

- [Shutdown::recv](README.md#next-application-slice-shutdownrecv)
  is the next Typed-IR application target.
- [Mini-redis library
  coverage](README.md#future-slice-mini-redis-library-coverage)
  expands the construct table through vertical acceptance slices.
- [Semantic inspector and persistent edit
  testing](README.md#implemented-slice-semantic-inspector-and-persistent-edit-testing)
  will provide a readable typed-tree view independent of oracle JSON.
- [Numeric inference and operator
  soundness](README.md#next-correctness-slice-numeric-inference-and-operator-soundness)
  owns {spec}`crates.sage_ir.types.operators.trait_calls`'s remaining generic and closure-evidence matrix.

## Oracle Test Harness

Former status section of [Oracle Test Harness](../../design/crates/sage_oracle_harness/oracle-guide.md).

### Oracle Test Harness: Current frontier and evidence

The harness independently emits rustc and Sage reference IR, validates
fixture-specific coverage for the pinned mini-redis and binary-operator slices,
requires checked-in snapshots where the slice defines one, and then compares
deterministic serialized text.
The [oracle-checked method review
packet](../../design/examples/oracle-checked-method.md#review-packet) links the source,
structural Sage assertion, snapshot, exact comparison, query trace, and edit
experiment. Together these artifacts exercise the exact snapshot and
comparison portion of [crates.sage_oracle_harness.crate.exact_comparison](../../design/anchor/crates/sage_oracle_harness/crate/exact_comparison.html), the adjustment and external-kind portions of
[crates.sage_oracle_harness.crate.fixed_projection](../../design/anchor/crates/sage_oracle_harness/crate/fixed_projection.html), exercise [crates.sage_oracle_harness.crate.deterministic_emission](../../design/anchor/crates/sage_oracle_harness/crate/deterministic_emission.html)'s local/external
identity ordering, and provide the slice-specific coverage evidence currently
available for [crates.sage_oracle_harness.crate.independent_coverage](../../design/anchor/crates/sage_oracle_harness/crate/independent_coverage.html).
`oracle_projects_operators_to_the_same_typed_tree_as_sage` additionally checks
the fixed operator projection from rustc HIR into static trait calls for
primitive addition, mixed-integer shift, comparison borrows, a local
user-defined `Add`, and a lazy boolean expression. In particular, the rustc
emitter reconstructs the language-item method when a primitive operation has
no retained type-dependent method marker; it does not consult Sage's output.
This is focused partial evidence for [crates.sage_oracle_harness.crate.fixed_projection](../../design/anchor/crates/sage_oracle_harness/crate/fixed_projection.html) and
[crates.sage_oracle_harness.crate.independent_coverage](../../design/anchor/crates/sage_oracle_harness/crate/independent_coverage.html), not the complete operator matrix.
The same full fixture suite is the regression gate that keeps the Semantic
Inspector's richer canonical external-navigation paths out of this shared
conformance projection.

Run all fixtures with `cargo test -p sage-oracle-harness`, or the pinned body
with:

```bash
cargo test -p sage-oracle-harness --test oracle_compare -- mini_redis/db_drop_guard.rs
cargo test -p sage-oracle-harness --test binary_operators oracle_projects_operators
```

### Oracle Test Harness: Current limitations

- General coverage accounting is not yet schema-wide; each new vertical slice
  must add explicit structural coverage checks so paired omission cannot pass.
  ORACLE-A4 is therefore established only for the pinned fixtures.
- The shared `rust-ref` schema covers only the currently represented Typed IR
  families, so the full adapter matrix required by ORACLE-A2 grows with Typed
  IR coverage.
- ORACLE-A1's independent perturbation and diagnostic-ordering matrix, and
  ORACLE-A2's adversarial one-sided omission matrix, are not yet presented as
  focused evidence.
- Oracle JSON is exact and reviewable but not the intended ergonomic
  human-facing semantic inspection format.
- The adversarial repetition, forward-reference, and reordered-input matrix
  required by ORACLE-A3 is not yet presented as focused evidence.

### Oracle Test Harness: Related roadmap slices

- [Shutdown::recv](README.md#next-application-slice-shutdownrecv)
  will add the next exact vertical fixture.
- [Semantic inspector and persistent edit
  testing](README.md#implemented-slice-semantic-inspector-and-persistent-edit-testing)
  adds readable Sage-only inspection without relaxing oracle equality.

## Stash (`sage-stash`)

Former status section of [Stash (`sage-stash`)](../../design/crates/sage_stash/storage-guide.md).

### Stash (`sage-stash`): Current frontier and evidence

Append-only hash-consed values/slices, collision handling, deterministic
rooted fingerprints, cross-stash copying, and Salsa updates are operational
across CST, signatures, solver values, and Typed IR. Body completion now
rebuilds settled inference output into a fresh stash. Focused evidence
includes:

- **[crates.sage_stash.storage.handle_ownership](../../design/anchor/crates/sage_stash/storage/handle_ownership.html):** `cross_stash_ptr_wrong_type`,
  `cross_stash_ptr_same_type_panics_in_debug`,
  `cross_stash_slice_panics_in_debug`, `copy_into_simple`,
  `copy_into_compound`, and `copy_into_with_slice`; typed-storage alignment is
  covered by `over_aligned_scalar_slice_and_zst_have_aligned_addresses`,
  `over_aligned_entries_survive_growth_clone_and_hash_cons_lookup`, and
  `over_aligned_values_copy_between_stashes`;
- **[crates.sage_stash.storage.typed_interning](../../design/anchor/crates/sage_stash/storage/typed_interning.html):** `hash_cons_dedup`, `hash_cons_slice_dedup`, and
  `hash_cons_collision_stores_both`, plus
  `derived_stash_hash_leaf_struct` and `derived_stash_hash_with_ptr_field`;
- **[crates.sage_stash.storage.rooted_equality](../../design/anchor/crates/sage_stash/storage/rooted_equality.html):** `stashed_eq_same_content_fingerprint`,
  `stashed_ne_different_content_fingerprint`,
  `stashed_hash_consistent_with_eq`, `stashed_ord_consistent_with_eq`,
  `stashed_eq_compound_dag`, and `fingerprint_deterministic`.
- **[crates.sage_stash.storage.immutable_entries](../../design/anchor/crates/sage_stash/storage/immutable_entries.html):** `body_finalization_resolves_types_into_a_fresh_stash`
  retains the inference node in its append-only working stash while inspecting
  the resolved type in a separately owned completed body. Absence of
  `IndexMut` and mutable-entry methods makes the prohibition an API property.

Run the storage tests with `cargo test -p sage-stash` and the finalization
boundary with
`cargo test -p sage-ir body_finalization_resolves_types_into_a_fresh_stash`.

### Stash (`sage-stash`): Current limitations

- Fingerprint equality is probabilistic at 128 bits; the architecture accepts
  that collision risk for incremental equality but semantic hash-consing still
  checks actual content before sharing a handle.
- Stash values are `Copy` and use a closed derive-supported storage model;
  owned variable-sized data must be represented through slices or interned
  semantic values.
- Focused stash tests establish selected value behavior for STASH-A1 through
  STASH-A3, but the reordered/unreachable-allocation matrix is not yet
  explicit and STASH-A1's completed-query ownership walk is not yet isolated.
  A query-level relevant/equal edit experiment directly establishing STASH-A4
  has not yet been isolated as stash-specific evidence.

### Stash (`sage-stash`): Related roadmap slices

The [Semantic Inspector and persistent edit-testing
slice](README.md#implemented-slice-semantic-inspector-and-persistent-edit-testing)
will make the effect of equal/backdated stashed results visible in structured
traces. Application slices extend the set of tree nodes but do not change this
storage contract.

## Book rendering

Former status section of [Book rendering](../../design/meta/book.md).

The migration connects the shared Design inventory to the generated Anchors
chapter and the Crates explorer. Implementation and test associations, including
explicit TODO/N/A entries, are displayed on each generated anchor page.
The [review checklist](../../design/meta/reviews.md) records the adequacy review of complete
claim/test sets separately from test execution. The
[RFD plan](../sage-contracts/implementation.md#canonical-anchor-system)
records implementation checkpoints and remaining work.

### Book rendering: Browser implementation and evidence

The book, standalone exporter, and loopback server share the core
crate/module/declaration views, inherited methods, envelope navigation, search
over declarations, methods, and named members, and a revealable Rust source pane. Search has
crate/kind filters and indexes prose and mapped Rust paths. Exported snapshots
contain the model, Rust source, and browser assets. The book preprocessor
regenerates the explorer snapshot during a book build; its generated chapters
open the relevant crate or module. The book works without a separate Sage
server. Entity selection follows the enclosing chapter URL, including direct
links and Back/Forward navigation. The Rust pane displays unevaluated cfg
conditions. The loopback server renders once before binding; it has no live reload.

The embedded explorer follows the book's active light or dark theme, including
changes made while the chapter is open and the book's automatic system setting.
The book's theme control governs the embedded view; the standalone explorer
retains its own theme toggle.

`crates/sage-meta/tests/browser.rs` checks invalid models and serve arguments,
standalone export, and exact script-safe model/source embedding.
`crates/sage-meta/web/browser.test.cjs` checks rendered routes, search, filters,
dependency targets, multiple Rust sites, source/prose escaping, and parent-chapter
navigation. `tests/book.rs` checks generated chapter trees, custom book roots,
model ownership, source relocation, script-safe snapshot data, and regeneration.

The [contract rules](../../design/meta/contracts.md) specify source correspondence and observed
runtime conformance independently of this browser. The generated anchor pages
retain any remaining incomplete coverage as explicit TODO entries.

## Architectural contracts

Former status section of [Architectural contracts](../../design/meta/contracts.md).

- `CorrespondenceIndex` implements bidirectional, crate-correct source checking.
  `correspondence.rs` tests cover Cargo ownership, multiple sites, abstract
  markers, supported generator inputs, unsupported placements, and source
  snapshot mismatch. Source inventory preserves cfg alternatives without
  asserting which declarations exist in the running binary.
- `ContractModel` rejects inconsistent models; `ContractSession` retains an
  immutable shared model. `SalsaObserver` verifies supplied compiled source
  snapshots and refuses to report conformance for observed unpaired files.
  Automatic runtime discovery, cached first-use loading, parallel initialization,
  and verification of model edits across subsequent loads remain **TODO**.
- The isolated real Salsa fixture checks cold, warm, validated/reexecuted,
  generated-API, forbidden, unmapped, cancellation, and panic behavior. Runtime
  engine tests check recursion and receiver-sensitive permissions. Ordinary
  function instrumentation and enforcement throughout compiler tests remain
  **TODO**; an inert annotation does not itself emit runtime events.
- Reports reject observed mapping, lifecycle, and receiver gaps and retain
  unchecked `reads`/`creates` effects separately. Current success is limited to
  supplied operation boundaries: automatic discovery of missing ordinary-call
  boundaries, effect hooks, async/task propagation, and comprehensive
  scenario-aware diagnostics remain **TODO**.

Existing behavioral tests carry references to the rules they exercise.
Additional implementation/test entries in `tests/contracts_plan.rs::todo`
record the unfinished portions explicitly; their passing placeholder tests do
not establish those behaviors. The
[implementation plan](../sage-contracts/implementation.md) tracks the
remaining compiler rollout and broader evidence.

## Architecture language

Former status section of [Architecture language](../../design/meta/language.md).

`sage-meta` provides a lexer, parser, public AST, shared workspace check API,
runtime contract engine, and command-line validator/exporter. Diagnostics
identify source locations. Run:

```sh
cargo run -p sage-meta -- check
cargo run -p sage-meta -- check --json
cargo test -p sage-meta
cargo test --all --workspace
```

The default check covers the repository model, Rust correspondences, named
specification paragraphs and their tests, and saved review decisions.
CI and the workspace integration test
use the same public library path. `check --correspondence` remains a compatible
spelling of the full check. `check path/to/model` selects a model directory
but still requires its containing Cargo workspace and documentation. The legacy
positional invocation `sage-meta path/to/model` validates an isolated model
without those requirements.

### Architecture language: Model and correspondence evidence

The corpus describes the core compiler demand graph and manifests for all
workspace crates. Supporting service operations are summarized, while derive
and harness internals have crate-level descriptions only. The conversion is
an architectural abstraction: for example `signature` maps to `sig`, external
subclasses model enum dispatch, and `BodyCheck` groups expression and pattern
checking implementations. It is not an exhaustive extraction of Rust calls.

`crates/sage-meta/tests/language.rs` supplies annotated tests for abstract-method
completeness, override compatibility, forbidden paths, virtual dispatch, and
inherited prohibitions. It also exercises AST parsing, source ranges,
manifest/module checks, and consistency of the repository corpus.
`tests/envelopes.rs` checks declaration kinds and dependency permissions,
including optional calls, call order, and direct versus indirect dependencies.
`tests/value_types.rs` checks the semantic type grammar and name resolution;
its annotations and the value-type tests in `tests/language.rs` identify the
individual rules they exercise.
`tests/correspondence.rs` checks missing/dangling/wrong-crate anchors, kind
compatibility, cfg alternatives, multiple Cargo targets, explicit module paths,
generator inputs, ambiguous/unmapped sites, nested functions, unsupported
placements, and source snapshot mismatches. CLI and workspace-check tests
verify the shared report and failure exit status. Specification fixtures check
invalid markers, duplicate or missing rules, invalid test references, and
Markdown examples that must not count as declarations. Initial
syntax supports one module level, structs and enums, opaque generic types,
nested type applications, and argument-free method references. Value-type tests
cover prose preservation, recursive structures, generic scope and arity,
cross-crate references, and structural override checking. Method parameters,
method/class generics, context initialization and binding flow, computed
expressions, and complete implementation conformance remain unimplemented. Context ownership is described, not escape-checked. Virtual
calls deliberately overapproximate possible receiver implementations.

See the [review-language roadmap slice](README.md#architectural-review-language)
for coverage work beyond this initial conversion.

The Rust browser integration tests exercise model rejection, script-safe
embedding, and repository export. After generating the snapshot,
`node crates/sage-meta/web/browser.test.cjs` checks search, navigation, every
rendered model link, conservative dispatch, and HTML escaping. These checks do
not substitute for visual layout testing in a browser engine.


### Architecture language: Embedded Rust source

The exporter embeds files reached by the same Rust correspondence index used
by validation. The pane supports multiple implementation sites and labels
source-only module markers and known generator inputs. Source navigation is
available for crate/module headers, types, and individual methods. It does not
resolve guessed Rust type names or require maintaining reverse annotations.

The snapshot does not evaluate cfg or expand arbitrary macros. Regenerate it
after Rust edits; no source server is required. The CLI's correspondence report
states both source-site and executable-declaration counts. An executable
source declaration is not, by itself, proof of runtime instrumentation.

### Architecture language: Contract infrastructure evidence

`tests/runtime_contracts.rs` supplies annotated evidence for
[`meta.language.calls.declared`](../../design/meta/language.md#meta.language.calls.declared) and
[`meta.language.calls.forbidden`](../../design/meta/language.md#meta.language.calls.forbidden). It exercises the shared checking
engine with explicit roots, recursion, interface dispatch, inherited
prohibitions, receiver checks, unbalanced events, and visible unchecked effects. Violations retain both the
undeclared edge and ancestor prohibition with their rule and complete path.

`tests/contracts.rs` runs the isolated real Salsa fixture under
`tests/fixtures/contracts/`. Its reviewed expectations name Sage operations and
independent Rust sites for cold, reused, and internally validated runs. Negative
cases include a forbidden request with the leaf cold or prewarmed, swapped and
missing mappings, ambiguity, stale source, macro provenance gaps, panic cleanup,
and accumulator requests. These test-only nodes are absent from the compiler
model. They establish infrastructure behavior, not complete compiler coverage.

`crates/sage-test-harness/tests/architecture_contracts.rs` supplies a separate
production check over a checked-in caller/leaf program. The empty function's
signature is mapped to `LocalFnSym::sig` and checked on cold and reused requests.
The caller-body scenario retains missing receiver/ordinary-boundary gaps and
therefore does not claim conformance; it independently verifies that no callee
body was requested. Complete caller-body enforcement remains outstanding.

The optional `sage-meta` `salsa` feature provides the shared callback adapter.
Runtime sessions are immutable; automatic process-wide loading/cache discovery,
async context propagation, general ordinary-function instrumentation, and
receiver projection in the Salsa adapter remain unfinished. `reads` and
`creates` are reported as unchecked. The first caller/callee body acceptance
scenario and replay/ACP/MCP stages remain follow-on work.

### Architecture language: Serve the explorer locally

From the repository root, run:

```sh
cargo run -p sage-meta -- serve 3001
```

Open <http://127.0.0.1:3001/>. The command validates and generates the snapshot
in memory, then serves it on loopback until Ctrl-C. Restart after model or Rust
source edits. For a remote workspace, forward port 3001 to your machine.
An optional model directory follows the port: `serve 3001 path/to/model`.
The port must be between 1 and 65535. Both `/` and `/sage-browser.html`
serve the explorer; other paths return 404.

## Specification test review

Former status section of [Specification test review](../../design/meta/reviews.md).

The review checklist consumes the shared Markdown/`.sage` Design inventory
and Cargo-owned Rust tests. `review prepare`, `review record`, and the shared
check implement exact claim/test extraction, stale-input detection,
input-checked recording, and the validation gate. The generated Anchors pages
display saved decisions alongside the current implementation and test entries.

The review is about the visible test assertions. Source inventory, saved
review, actual execution, and observed runtime conformance remain separate.
The current counts and individual TODO/N/A explanations are generated in
[Anchors](../../design/anchor/README.md), rather than copied into this chapter.

## Language subsetting

Former status section of [Language subsetting](../../design/subsetting.md).

### Language subsetting: Current frontier and evidence

The destination feature list above is not an implementation-coverage list.
Current coverage is driven by reviewed vertical slices and recorded locally in
the architecture guide. These links establish [crates.sage_ir.crate.restrictions_and_gaps](../../design/anchor/crates/sage_ir/crate/restrictions_and_gaps.html)'s current ownership
split for the named phases:

- [Module and Macro Expansion](coverage-history.md#module-and-macro-expansion)
  records the empty-input local `macro_rules!` subset, built-in derives,
  external bang expansion, active-attribute incompleteness, and depth limit.
- [Name Resolution](coverage-history.md#name-resolution) records
  represented path/import/prelude behavior and associated lookup limitations.
- [Signature Checking](coverage-history.md#signature-checking) records
  supported interface forms and the `Dummy` lifetime boundary.
- [Body Checking](coverage-history.md#body-checking-and-typed-ir-elaboration) records the
  implemented expression, method, inference, and obligation frontier.
- [Typed IR](coverage-history.md#typed-ir) records which source constructs are
  fully elaborated rather than merely parsed or retained for recovery.

Unsupported or malformed items remain explicit error items. Incomplete item
or attribute expansion is conservatively visible to name resolution and trait
candidate discovery, so unrepresented code cannot justify a false proof or
ground negative result. Lint-only inner attributes are treated as inert; other
unrepresented active attributes remain incomplete. The focused tests
`unresolved_item_macro_prevents_ground_no`,
`malformed_item_macro_output_prevents_ground_no_without_panicking`, and
`recursive_item_macro_hits_expansion_limit_and_returns_maybe` establish the
unresolved, malformed, and resource-limited expansion portion of
[crates.sage_ir.crate.represented_completeness](../../design/anchor/crates/sage_ir/crate/represented_completeness.html).

### Language subsetting: Current limitations

- **Known deviation [KD-3](deviations.md#kd-3-derive-helper-attributes-do-not-produce-the-promised-warning):**
  registered derive helper attributes are not distinguished from arbitrary
  potentially active attributes, so Sage omits the affected item
  conservatively instead of retaining it and reporting the promised warning.
- The restriction list does not yet link focused acceptance, rejection, or
  approximation fixtures for every entry, so SUB-A1 is not established as a
  complete matrix.
- Several entries are phrased as “not yet supported.” They still need an audit
  to determine whether they are deliberate destination restrictions or
  temporary phase gaps before SUB-A2 is fully established.
- SUB-A3 has focused expansion-to-solver evidence, but not yet the complete
  downstream resolution and solver matrix for every incomplete source class.

### Language subsetting: Related roadmap slices

The [Build-Out Roadmap](README.md) orders the next
cross-cutting coverage slices. This page owns only intentional language
restrictions; phase Current Status sections own temporary implementation gaps.

## Design tenets

Former status section of [Design tenets](../../design/tenets.md).

These tenets span several phases, so their claim-specific evidence lives in
the focused chapters rather than being duplicated here:

- [crates.sage_ir.symbols.body_interfaces](../../design/anchor/crates/sage_ir/symbols/body_interfaces.html) and [crates.sage_ir.types.binder.owner_parameters](../../design/anchor/crates/sage_ir/types/binder/owner_parameters.html): [Signature
  Checking](../../design/crates/sage_ir/signatures-guide.md), [Body Checking and Typed-IR
  Elaboration](../../design/crates/sage_ir/bodies-guide.md), and [Symbols](../../design/crates/sage_ir/symbols-guide.md);
- [crates.sage_stash.storage.query_value](../../design/anchor/crates/sage_stash/storage/query_value.html): [Stash](../../design/crates/sage_stash/storage-guide.md);
- [crates.sage_ir.resolution.lookup.precedence](../../design/anchor/crates/sage_ir/resolution/lookup/precedence.html): [Name Resolution](../../design/crates/sage_ir/resolution-guide.md);
- [crates.sage_ir.crate.reexecution_and_invalidation](../../design/anchor/crates/sage_ir/crate/reexecution_and_invalidation.html): [Incrementality and Query
  Boundaries](../../design/crates/sage_ir/incrementality-guide.md); and
- [crates.sage_oracle_harness.crate.exact_comparison](../../design/anchor/crates/sage_oracle_harness/crate/exact_comparison.html): [Oracle Test Harness](../../design/crates/sage_oracle_harness/oracle-guide.md); and
- [crates.sage_inspector.inspection.source_evidence](../../design/anchor/crates/sage_inspector/inspection/source_evidence.html): [Validation and Inspection](../../design/meta/validation.md) and
  [Semantic Inspector](../../design/crates/sage_inspector/inspector-guide.md).

The principal current gap is [crates.sage_ir.crate.reexecution_and_invalidation](../../design/anchor/crates/sage_ir/crate/reexecution_and_invalidation.html): several coarse same-file and
module dependencies cause reexecution before an equal semantic result can be
backdated. The incrementality chapter distinguishes that extra execution from
downstream propagation and records the current edit evidence.
