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

### KD-1: Associated-item CST spans use the owner as their base

- **Contracts:** [SPAN-A1](../design/spans.md#span-a1) and
  [D17](../design/decisions.md#d17-nested-spans-are-relative-to-stable-item-provenance).
- **Observed implementation:** Trait and impl parsing measures spans throughout
  each associated function, type, or constant relative to the enclosing trait
  or impl. Extracting the associated item preserves those coordinates and
  stores the owner's absolute span as `cst_base`.
- **Consequence:** Diagnostics resolve correctly through the compensating base,
  but inserting or resizing an earlier associated item changes nested spans in
  an otherwise unchanged later item. Its CST and downstream semantic products
  therefore cannot be reused as independently as the design requires.
- **Closure evidence:** Remove the owner-relative `cst_base` special case and
  add edit tests which insert, remove, and resize an earlier associated item.
  The later item's symbol, item-relative CST, and Typed IR must remain equal and
  reusable while its composed absolute source position updates.

### KD-2: Most local-symbol CST fields participate in symbol identity

- **Contracts:** [ARC-A1](../design/architecture.md#arc-a1),
  [PAR-A2](../design/pipeline/parsing.md#par-a2), and
  [SYM-A2](../design/infrastructure/symbols.md#sym-a2).
- **Observed implementation:** `LocalFnSym::cst` is independently tracked, but
  most other local item symbols currently carry their CST as a Salsa identity
  field. Changing that syntax can therefore create a replacement tracked
  identity rather than update detail behind the existing symbol.
- **Consequence:** A detail-only edit can remint the affected symbol and
  invalidate consumers which depend only on an unchanged interface or sibling
  fact.
- **Closure evidence:** Make CST an independently tracked field for every local
  item kind and add a per-kind persistent-edit matrix. Body, signature, member,
  and attribute edits must preserve symbol identity and rerun only consumers of
  the changed detail; genuine name, owner, or occurrence changes must still
  produce the intended identity change.

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

None yet.
