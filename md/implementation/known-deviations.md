# Known Architecture Deviations

This register tracks confirmed cases where the current implementation
contradicts an accepted design anchor. It is not a general backlog: an
unimplemented destination, incomplete verification matrix, or intentionally
unsupported Rust feature belongs in the relevant architecture chapter and
roadmap instead.

Each entry names the violated contract, describes the observed implementation
without weakening the destination design, and states the evidence required to
close it. The affected architecture chapter links back to the entry from its
**Current Status** section.

## Open deviations

### KD-7: Arithmetic operators do not require numeric operands

- **Contract:** [BODY-A1](../design/pipeline/body-checking.md#body-a1), including
  the body-phase guarantee that user type errors produce diagnostics rather
  than an apparently valid typed operation.
- **Observed implementation:** Unsuffixed integer literals begin as
  unconstrained type variables, and binary `+` currently equates its two
  operand types without also requiring a numeric operator domain. While adding
  surplus-argument diagnostic coverage, the expression `1 + true` was checked
  but produced no type diagnostic: the integer variable unified with `bool`.
- **Consequence:** A ground, ill-typed arithmetic expression can survive body
  checking as a resolved binary operation instead of producing an error and
  recovery node.
- **Closure evidence:** Add a source-driven integration reproducer which
  rejects `1 + true`, implement numeric literal constraints and operator-domain
  checking without relying on a caller's expected type, and cover
  representative integer, float, boolean, comparison, and mixed-operand cases
  against the independent rustc oracle.

### KD-4: Stash byte storage does not guarantee entry alignment

- **Contract:** [STASH-A1](../design/stash.md#stash-a1), together with the
  `StashData` representation contract that every accepted value can be read
  through its typed handle.
- **Observed implementation:** `Stash` stores entries in `Vec<u8>` and aligns
  each entry's byte offset, but `Vec<u8>` only guarantees byte alignment for
  the allocation's base address. Adding an aligned offset cannot make an
  under-aligned base suitable for a type with alignment greater than one, so
  the typed raw-pointer reads can be undefined behavior for over-aligned
  `StashData` values.
- **Consequence:** The ordinary Sage node types happen to receive typical
  allocator alignment in practice, but the public unsafe storage contract is
  not upheld for every accepted `T`, and a future over-aligned node could make
  otherwise valid stash access unsound.
- **Closure evidence:** Replace the byte buffer with storage whose base and
  entry layout guarantee `align_of::<T>()` for every allocation (or constrain
  and enforce the admitted alignments), then cover scalar and slice allocation,
  growth, cloning, hash-cons lookup, and cross-stash copy with
  deliberately over-aligned values under Miri.

### KD-3: Derive helper attributes do not produce the promised warning

- **Contract:** [No derive helper attributes](../design/subsetting.md#no-derive-helper-attributes),
  with the user-visible response governed by
  [SUB-A1](../design/subsetting.md#sub-a1).
- **Observed implementation:** Sage does not yet import a proc-macro derive's
  registered helper-attribute names. A helper such as `#[serde(...)]` is
  therefore treated like an unknown potentially active attribute: the affected
  item is omitted from the definite expanded set and completeness becomes
  conservative, rather than retaining the item and reporting a warning.
- **Consequence:** Code using a recognized derive's helper attributes can lose
  otherwise usable semantic information and receive no focused explanation.
- **Closure evidence:** A fixture using a registered external derive helper
  must retain the attributed item, emit an unknown-helper warning at the helper
  span, and remain complete for semantic facts which the helper cannot alter.
  An arbitrary unclassified active attribute must continue to produce
  conservative incompleteness rather than being assumed inert.

## Closed deviations

### KD-6: Never-to-any coercion was modeled as subtyping

- **Contracts:** [INF-A5](../design/subsystems/type-inference.md#inf-a5),
  [BODY-A1](../design/pipeline/body-checking.md#body-a1), and
  [D11](../design/decisions.md#d11-completed-bodies-are-elaborated-typed-trees).
- **Closed:** 2026-09-01. `require_sub` no longer treats `Ty::Never` as a
  subtype of every type, mutable-reference subtyping relates its pointee types
  by equality, and coercion sites select never-to-any separately. The selected
  conversion becomes an explicit `TyExprData::NeverToAny` node and survives
  body finalization.
- **Evidence:** `never_to_any_does_not_apply_beneath_mutable_reference` rejects
  `&mut !` where `&mut u32` is required.
  `top_level_never_to_any_is_explicit_in_typed_ir` accepts a diverging body at
  a `u32` result coercion site, and
  `call_argument_never_to_any_is_explicit_in_typed_ir` does the same for a
  direct-call argument. Both inspect the completed node's `!` operand and
  `u32` result; `struct_field_never_to_any_uses_the_value_span` covers a field
  coercion and its operand span, while
  `external_inherent_call_accepts_a_never_argument` covers the resolved-method
  path against rustc-provided metadata. The
  `never_result_does_not_create_a_never_to_never_coercion`
  and `inferred_never_join_does_not_retain_never_to_never_coercions` exclude
  redundant conversion for known and join-inferred targets.
  `generic_call_arguments_are_order_independent_and_fall_back_to_never`
  verifies that later non-diverging constraints determine a generic expected
  type regardless of argument order, while a target constrained only by
  divergence falls back to `!` without a conversion;
  `never_fallback_wakes_suspended_method_lookup` verifies that this fallback
  wakes a consumer suspended on the inferred type instead of deadlocking, and
  `quiescent_never_fallback_does_not_settle_unrelated_targets` verifies that
  intermediate recovery does not settle targets which later body constraints
  can still determine.
  `external_generic_method_defers_never_until_other_constraints_settle`
  verifies the same deferred choice for a rustc-metadata-backed generic
  method. The source-driven
  `basics/never_to_any.rs` oracle case compares Sage's and rustc's independently
  emitted trees by exact text at the function-result, direct-call, and
  struct-field conversion sites.

### KD-5: The scripted inspector advertised products it could not return

- **Contracts:** [SI-A3](../design/validation/semantic-inspector.md#si-a3) and
  [SI-A15](../design/validation/semantic-inspector.md#si-a15).
- **Closed:** 2026-08-31. The scripted provider and `--fixture` command path
  were removed. Catalogs and products now come only from the live provider over
  Rust source, so precomputed catalog and response models cannot drift apart.
- **Evidence:**
  `real_command_serves_embedded_assets_and_correlated_request_logs` launches
  the live sample project, selects `Db` from the source-built directory, and
  fetches its advertised identity page through the production actor and Axum
  server. The same test snapshots the representative visible result and
  actor-owned demand.

### KD-1: Associated-item CST spans used the owner as their base

- **Contracts:** [SPAN-A1](../design/spans.md#span-a1) and
  [D17](../design/decisions.md#d17-nested-spans-are-relative-to-stable-item-provenance).
- **Closed:** 2026-08-18. Associated-item placement is stored separately from
  the item-local function, type, or const CST; relative diagnostics now resolve
  from the associated item's own absolute span. The `cst_base` special case was
  removed.
- **Evidence:** `moving_associated_items_preserves_symbol_query_keys` covers all
  three associated kinds through both the trait and impl parsing paths across
  sibling insertion, removal, and reordering, and
  `moving_associated_item_reuses_method_signature_and_body` proves through the
  post-edit Salsa traces for both owners that an unchanged moved method retains
  its semantic queries.

### KD-2: Most local-symbol CST fields participated in symbol identity

- **Contracts:** [ARC-A1](../design/architecture.md#arc-a1),
  [PAR-A2](../design/pipeline/parsing.md#par-a2), and
  [SYM-A2](../design/infrastructure/symbols.md#sym-a2).
- **Closed:** 2026-08-18. CST, module source/attributes, variant shape, and
  local generic-parameter spans are tracked detail rather than definition
  identity.
- **Evidence:** `detail_edits_preserve_local_symbol_query_keys` covers every
  top-level local item shape,
  `enum_detail_edits_preserve_variant_query_keys` covers variants and
  constructors while consuming the updated self-contained variant CST, and
  `moving_associated_items_preserves_symbol_query_keys` covers associated
  functions, types, and constants under both trait and impl owners across
  sibling insertion, removal, and reordering.
  `local_normalization_reads_one_keyed_value_without_impl_item_enumeration`
  verifies that focused and aggregate impl-item paths share the canonical
  per-item producer query and that editing a sibling value does not rerun the
  requested-value query.
