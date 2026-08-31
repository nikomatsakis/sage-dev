# Module and Macro Expansion

Module expansion turns parsed module items into the direct semantic members
that later name resolution and checking may consume. It is demand-driven: a
consumer asks for one local module, not for an eagerly expanded crate.

## Contract

### Granularity

The unit of demand and memoization is one local module. Child modules have
their own expansion result; expanding a parent does not flatten the child's
members into the parent.

### Input

The direct input is the module's ordered, unexpanded item symbols. Expansion
may additionally read:

- the module's edition and scope;
- the macro namespace needed to resolve an invocation;
- the selected local or external macro definition and expansion metadata; and
- generated text parsed with provenance linking it to the invocation.

It does not need item signatures or function bodies.

### Output

A successful result is an ordered sequence of the module's direct represented
item symbols after expansion. Ordinary source items retain their position.
Bang-macro output is recursively expanded at the invocation position, and
derive output follows the item that produced it. Generated items use the same
kind-specific local symbols as source-written items.

The destination result also records whether the represented sequence is
complete. If it is incomplete, it retains safe represented symbols together
with reasons that prevent downstream consumers from treating absence as
proof.

The result is not the set of every name visible in the module. A `use` item is
a direct symbol in this output; imports, globs, prelude lookup, and namespace
rules affect which symbols name resolution can discover later.

### Guarantees

When expansion is complete, downstream consumers may rely on the result
containing every direct item represented by the module after supported macro
expansion. Ordering and generated-source provenance are deterministic. A
complete absence can therefore support exhaustive questions such as whether
a local module can contribute an impl.

An incomplete result makes a weaker guarantee: every published symbol denotes
a represented item, but an omitted or unexpanded construct may have produced
additional items. Consumers must not turn that absence into a definite
negative semantic answer.

<a id="exp-a1"></a>
> **EXP-A1 — Expansion returns direct module members, not a visible-name
> environment.** The result preserves deterministic order and provenance for
> source and generated direct items. Child-module members remain in the child,
> while imports and namespace lookup are interpreted later by resolution.
>
> **Required verification:** Fixtures cover ordinary items, `use` items,
> inline child modules, recursively generated items, and derives, asserting
> direct membership, ordering, ownership, and generated provenance without
> treating imported or child names as direct parent members.

<a id="exp-a2"></a>
> **EXP-A2 — Exhaustive absence requires complete expansion.** Published
> symbols are safe to use even when expansion is incomplete, but an omitted
> symbol supports a definite negative answer only when expansion is complete
> for that semantic question. This is the expansion consequence of
> [D16](../decisions.md#d16-incompleteness-is-an-explicit-terminal-outcome).
>
> **Required verification:** Successful expansion permits a ground exhaustive
> negative query, while unresolved, malformed, unsupported, and resource-limited
> expansion produce conservative uncertainty rather than the same negative.

## Entry points

`LocalModSym::expanded_module_items` dispatches to the tracked
`local_expanded_module_items` query for local modules. The query begins with
`unexpanded_items(db, module)` and returns direct `Symbol` values:

```{anchor}
example_expanded_module_query
```

The empty cycle initial value opts the query into Salsa fixed-point recovery:

```{anchor}
architecture_expanded_module_cycle_initial
```

## Construction

Expansion walks unexpanded items in source order. Ordinary items pass through
attribute and derive handling. A bang-macro invocation is resolved in the
macro namespace, its output is parsed into new unexpanded item symbols, and
those symbols are processed by the same recursive expansion path:

```{anchor}
example_expand_items
```

```{anchor}
example_expand_macro
```

Recursive output expansion and fixed-point iteration solve different
problems. Recursion handles a macro whose emitted text directly contains
another invocation. The fixed point handles a resolver cycle: resolving an
invocation may need the expanded symbols of the *same* module—for example,
when an earlier macro invocation emits the definition of a later macro. The
recursive query first observes the empty provisional result, Salsa reruns it
with the newly represented symbols, and iteration stops when the ordered
symbol sequence is unchanged.

Expansion convergence does not by itself establish semantic completeness. A
separate audit must still account for failed, unresolved, unsupported, or
depth-limited constructs.

<a id="exp-a3"></a>
> **EXP-A3 — Recursive expansion and same-module fixed points are distinct.**
> Recursive processing expands invocations emitted directly by another
> invocation. Salsa fixed-point recovery is used only when resolving a macro
> requires the provisional expanded members of that same module; convergence
> does not imply completeness.
>
> **Required verification:** One fixture exercises nested emitted invocations,
> another requires a macro definition emitted earlier in the same module, and
> traces show bounded fixed-point reexecution followed by stable warm reuse.

## Failure and terminal incompleteness

Expansion may terminate without the complete-output guarantee because of:

- invalid or ambiguous source, including malformed generated items;
- a Sage limitation, such as an unsupported `macro_rules!` matcher or active
  attribute;
- unavailable or unresolved external macro expansion; or
- the macro expansion depth limit.

These are terminal outcomes for the current query evaluation, not work waiting
for another scheduling turn. Represented items remain useful for recovery, but
consumers must conservatively account for what unrepresented expansion could
have added.

The current implementation performs that audit for a particular semantic
question. Bang-macro completeness recursively checks resolution, generated
parse success, active attributes, and the depth limit:

```{anchor}
architecture_macro_expansion_completeness
```

Trait candidate discovery and method-provider discovery then refine this
answer for their own needs. For example, an unsupported compiler builtin
derive may be known unable to add an impl of one particular trait while a
general procedural derive remains capable of adding arbitrary providers.
The Semantic Inspector asks the stricter directory question: whether every
child which expansion could add is represented. An unsupported derive is
therefore incomplete even when a narrower trait lookup could safely ignore it:

```{anchor}
architecture_symbol_listing_completeness
```

## Incremental dependencies

The tracked query key is `LocalModSym`. It may observe the module's
unexpanded-item result, identities and tracked fields of macro invocations and
definitions, same-module provisional expansion during resolution, and keyed
external expansion facts. It must not read unrelated item signatures, trait
solutions, or function bodies.

Changing an invocation, a definition it resolves to, or a generated output
must invalidate the affected module result. Moving a source item without
changing its semantic or generated identity should preserve that identity;
equal reconstructed output may be backdated so unchanged consumers do not
reexecute. A change inside a function body is not an expansion dependency.

<a id="exp-a4"></a>
> **EXP-A4 — Expansion observes expansion inputs, never checked semantic
> detail.** A module expansion may depend on parsed item identities, macro
> resolution and definitions, generated output, and completeness facts. It
> must not depend on item signatures, trait solutions, or function bodies.
>
> **Required verification:** Cold and warm traces identify the expansion
> boundary; edits to an invocation or selected macro output invalidate it;
> body-only and unrelated-signature edits do not; and unchanged reconstructed
> output is backdated before downstream consumers execute.

## Worked example

This program requires same-module fixed-point iteration:

```rust
macro_rules! define_macro {
    () => {
        macro_rules! define_generated {
            () => { struct Generated; }
        }
    }
}

define_macro!();
define_generated!();
```

The first invocation emits the `define_generated` macro definition. On the
first pass, resolution of the final invocation sees the cycle's provisional
empty result. The provisional expanded module nevertheless contains the new
macro definition. On the next iteration the final invocation resolves and the
stable direct-member sequence contains:

```text
macro define_macro
macro define_generated    (generated by define_macro!())
struct Generated          (generated by define_generated!())
```

The focused test
`same_module_macro_resolution_reaches_a_fixed_point` uses this input and
asserts that `Generated` is present. The [macro-generated struct
walkthrough](../examples/macro-generated-struct.md) follows the more common
single-expansion path into signature and body checking.

## Code map

| Path | Responsibility |
|---|---|
| `local_syms/mods.rs` | unexpanded and expanded module queries, recursive expansion, fixed-point initial value, completeness audits |
| `local_syms/macro_invocations.rs` | invocation identity, CST, output expansion, and generated parsing |
| `local_syms/macro_defs.rs` | local `macro_rules!` identity and currently supported rule extraction |
| `derive/` | built-in derive recognition and generated sibling impls |
| `check/resolve/` | macro-namespace and later ordinary name resolution |
| `span.rs` and `parse/` | generated-source provenance and parsing output back into ordinary item symbols |

## Current status

### Current frontier

Local empty-input `macro_rules!` invocations, recursively generated items,
same-module macro-definition cycles, external bang procedural macros through
the metadata bridge, and selected built-in derives produce ordinary expanded
symbols with provenance. Expansion is tracked per local module.

### Implemented capabilities and evidence

- **[EXP-A3](#exp-a3) — Same-module fixed point.**
  `same_module_macro_resolution_reaches_a_fixed_point` exercises a macro that
  creates the definition needed by a later invocation. Run
  `cargo test -p sage-test-harness
  same_module_macro_resolution_reaches_a_fixed_point` and inspect the entry
  query and cycle-initial anchors above.
- **[EXP-A2](#exp-a2) — Successful item expansion and exhaustive negative
  search.**
  `successful_item_macro_keeps_ground_negative_search_complete` shows that a
  single successfully represented item macro does not unnecessarily weaken a
  ground trait result. It does not exercise a nested emitted invocation; that
  part of EXP-A3 remains unverified.
- **[EXP-A2](#exp-a2) — Terminal incompleteness.**
  `unresolved_item_macro_prevents_ground_no`,
  `malformed_item_macro_output_prevents_ground_no_without_panicking`, and
  `recursive_item_macro_hits_expansion_limit_and_returns_maybe` show that
  unresolved input, invalid generated Rust, and resource exhaustion cannot
  create a false negative answer.
- **[EXP-A1](#exp-a1) — Generated identity across edits.**
  `moving_source_item_preserves_derive_expansion_identity` moves a source item
  and verifies stable expansion identity while its origin coordinate updates.
- **[EXP-A2](#exp-a2) — Inspector directory completeness.**
  `retained_host_separates_edits_from_demand_and_preserves_symbol_paths`
  adds an unsupported but valid derive and requires the represented root's
  child status to become explicitly incomplete.

The compiler tests live in `crates/sage-test-harness/src/lib.rs`; the retained
directory/edit test lives in `tests/semantic_inspector.rs`. The Semantic
Inspector now supplies the human-readable persistent query trace for these
queries.

### Review packet

| Question | Evidence |
|---|---|
| What Rust is expanded? | the fixed-point source under [Worked example](#worked-example) |
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

### Current limitations

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
- Existing generated-symbol evidence does not yet cover EXP-A1's complete
  direct-membership matrix for imports, inline children, recursive output, and
  derives in one review packet.
- EXP-A3 has fixed-point success and recursive depth-limit evidence, but no
  successful fixture in which one expansion emits another invocation.
- Evidence covers stable generated identity and focused behavior, but not yet
  a structured edit/invalidation trace for the expanded-module query; EXP-A4's
  full positive and negative edit matrix remains prospective.

### Related roadmap slices

- [Auditable architecture and review
  evidence](../../implementation/roadmap.md#in-flight-slice-auditable-architecture-and-review-evidence)
  establishes this chapter and its review trail.
- [Semantic inspector and persistent edit
  testing](../../implementation/roadmap.md#implemented-slice-semantic-inspector-and-persistent-edit-testing)
  will add readable expanded output and query traces across edits.
- [Trait-partitioned impl
  discovery](../../implementation/roadmap.md#planned-slice-trait-partitioned-impl-discovery)
  consumes expansion completeness at a narrow candidate-discovery boundary.
