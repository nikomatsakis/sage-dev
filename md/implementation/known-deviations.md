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

### KD-6: Never-to-any coercion is modeled as subtyping

- **Contract:** [INF-A5](../design/subsystems/type-inference.md#inf-a5)
  requires subtyping to preserve representation and never-to-any to be selected
  only as a coercion. [BODY-A1](../design/pipeline/body-checking.md#body-a1) and
  [D11](../design/decisions.md#d11-completed-bodies-are-elaborated-typed-trees)
  additionally require the selected coercion to be materialized in completed
  Typed IR rather than accepted through an unrelated type relation.
- **Observed implementation:** `InferCtx::require_sub` accepts `Ty::Never` as
  a subtype of every target, while `require_coerce` merely delegates to
  `require_sub`. Rust's never-to-any behavior is a coercion, not ordinary
  subtyping. The ignored
  `never_to_any_does_not_apply_beneath_mutable_reference` integration
  regression checks the resulting source behavior and fails against the
  current implementation.
- **Consequence:** A direct subtype request such as `! <: u32` succeeds, and
  recursively applying the same rule beneath references can admit invalid
  relations such as `&mut ! <: &mut u32`. Completed bodies can therefore rely
  on a coercion which was neither requested nor represented.
- **Closure evidence:** Move never-to-any handling to the coercion path,
  represent the selected operation in completed Typed IR where required, and
  make `never_to_any_does_not_apply_beneath_mutable_reference` pass without
  `#[ignore]`. Add complementary positive evidence for top-level never
  coercion.

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
