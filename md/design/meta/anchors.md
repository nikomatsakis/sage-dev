# Anchors and Evidence

## Specification tests and workspace checks

The [Design chapter tree](book.md#meta.book.authority) forms the canonical
specification, with Markdown and `.sage` as its source formats. Named anchors
connect that specification to implementation and tests. The following
paragraphs state the destination; the generated [Anchors](../anchor/README.md)
pages show implementation and test evidence with their explicit statuses.
The [anchor-system RFD](../../rfds/sage-contracts/README.md) records the
motivation and implementation plan.

### Definitions and references

{anchor}`canonical_sources`
A specification rule can be defined in a Markdown or `.sage` Design chapter,
with equal authority. Its definition records the rule's text and source
location; a `.sage` definition also identifies its containing declaration, method, or
envelope clause. Either documentation form can reference a rule defined in
the other without creating another definition. Every reference must resolve to
an existing definition; a dangling reference is an error.

{anchor}`unique_names`
Every specification anchor has a well-formed full name and exactly one
definition in the Design chapter inventory. Malformed markers and duplicate
full names are errors. Names are
[derived from the defining chapter](book.md#meta.book.chapter_names);
[moves between chapter namespaces](book.md#meta.book.anchor_moves)
rename the rule and require updating its references.

Resolved names are dot-separated ASCII identifiers and describe independently
testable behavior. A Markdown definition uses a chapter-local name with the
custom MyST-style inline-role syntax. For example, in `design/meta/language.md`:

```text
{anchor}`calls.declared`
Every observed architectural call must be permitted by its caller's resolved
dependency envelope.
```

The marker and the opening prose form one paragraph: do not leave a blank
line between them. Later blocks can continue the same claim under the
[content rules](#anchor-content). The example resolves to
`meta.language.calls.declared`. Use the same definition syntax inside `.sage`
triple-quoted prose. In either format, `{spec}` followed by a backtick-quoted
full name references an existing rule without creating another definition.

## Anchor content

The extractor selects the same complete source content for generated anchor
views and saved claim reviews. The
[content-boundary record](../../rfds/sage-contracts/implementation.md#anchor-content-boundaries)
describes implementation and corpus migration. Authoring guidance lives in
[Anchor tenets](../tenets.md#anchor-tenets).

{anchor}`content.blocks`
An anchor's content starts after its definition marker and includes every
subsequent Markdown block until the first applicable closing boundary. Blank
lines do not end the content: paragraphs, lists, tables, and code blocks within
that extent belong to the same anchor.

{anchor}`content.next_anchor`
A new anchor definition ends the preceding anchor's content immediately before
the new marker. The new definition belongs to its own anchor; content extents
do not overlap.

{anchor}`content.heading`
A Markdown section heading ends the active anchor's content immediately before
the heading, regardless of heading level. Both hash-prefixed and underline-style
headings are boundaries. Prose following the heading remains outside an anchor
until another definition begins.

{anchor}`content.source_end`
An anchor's content ends at the end of its containing Markdown document or
`.sage` triple-quoted prose block. A `.sage` anchor never captures surrounding
language syntax or content from another prose block, declaration, or method.

{anchor}`content.explicit_end`
A standalone line containing exactly `<!-- end anchor -->`, apart from
surrounding horizontal whitespace, ends the active anchor immediately before
that line. The comment is excluded from the anchor's content. Subsequent prose
remains outside an anchor until a new definition begins; the explicit marker
is optional when another closing boundary already ends the content.

{anchor}`content.unmatched_end`
An explicit end marker with no active anchor is a validation error. Another
end marker after an explicit or implicit end, without an intervening definition,
is therefore also an error. An anchor ending at another definition, a heading,
or its source boundary does not require an explicit end marker.

{anchor}`content.literal_syntax`
Boundary-looking text inside fenced or indented code blocks, inline code, or
escaped text does not open or close an anchor. Raw HTML examples do not supply
anchor definitions, section headings, or end markers. A standalone end comment
acts as a boundary only outside these literal contexts and enclosing HTML blocks.

### Example and authoring

```text
{anchor}`cache_reuse`
An unchanged query reuses its result.

An input edit is unrelated when the query does not depend on that input.

For example, demanding the query again after editing an unrelated file still
reuses the result.

<!-- end anchor -->

This explanatory note is outside the anchor.
```

Each independently testable guarantee still needs its own anchor; a larger
content extent allows qualifications and examples to stay with that guarantee.
When editing a claim, review its trailing prose and insert boundaries or
separate definitions where necessary. The same selected content
feeds the generated anchor views and the [claim review input](reviews.md#meta.reviews.claim_text).

## Rust correspondence

### Rust implementation entries

{anchor}`implementation_references`
A Rust implementation entry is introduced by a
<code>// {anchor}&#96;meta.language.calls.declared&#96;</code> comment, optionally followed
immediately by `<<`. Its name must resolve to a specification definition;
malformed and unknown references are errors. Only actual Rust comments create
these entries: marker-like text inside string literals does not. Entries retain
their source location and enclosing module, and multiple entries may reference
the same rule, including across crates.

{anchor}`implementation_coverage`
Every specification anchor has at least one explicitly declared Rust
implementation entry. An anchor without one is an error. A test annotation or
`#[sage::item(...)]` correspondence alone does not supply this entry. A
placeholder implementation counts as an entry with its explicit status.

{anchor}`point_excerpts`
A point reference has no `<<` suffix. Its initial excerpt contains the marker
line and the next five source lines, truncated at end-of-file. A **See more…**
action opens the surrounding source at that location; the five-line preview
does not claim to delimit the implementation.

```rust,ignore
// {anchor}`meta.language.calls.declared`
check_declared_call(caller, callee)?;
```

{anchor}`region_excerpts`
A reference ending in `<<` opens an explicit region, and a standalone `// >>`
comment closes the nearest open region in the same file. Regions may nest.
The excerpt spans the opening marker line through its closing marker line,
with access to the surrounding source. A point reference inside a region
remains a point reference and does not close or replace that region.

```rust,ignore
// {anchor}`meta.language.calls.declared`<<
let caller = current_operation();
check_declared_call(caller, callee)?;
record_call(caller, callee);
// >>
```

{anchor}`region_balance`
A region opener without a closing delimiter, or a closing delimiter without
an open region, is an error. Delimiters in string literals do not open or close
regions. Region boundaries are determined from source comments, independently
of the five-line previews used for point references.

The existing `// ANCHOR: name` / `// ANCHOR_END: name` convention remains
independent: it delimits explanatory excerpts for mdBook/ezanchor. Canonical
implementation entries require only the specification comment syntax above.

### Tests and explicit placeholders

Tests identify their rules with inert attributes:

```rust,ignore
#[test]
#[sage::test(meta.language.calls.declared)]
#[sage::test(meta.language.calls.forbidden)]
fn reports_direct_and_ancestor_violation_with_complete_path_and_rule() {
    // Assert the observed edge and the violated ancestor's prohibition.
}
```

{anchor}`test_coverage`
Every specification anchor must have at least one annotated Rust test. An
anchor without a test is an error, independently of its implementation
coverage. A placeholder test counts as an entry with its explicit status; an
implementation placeholder alone cannot satisfy the test requirement.

{anchor}`test_references`
Every `#[sage::test(...)]` reference must resolve to an existing specification
anchor and be attached to a Rust test. Dangling and misplaced references are
errors.

One test can cover several rules and a rule can have several tests. Unlike
`sage::item`, a test may reference a rule across crate boundaries. An eligible
test entry is a module-level function with the built-in `#[test]` attribute in
a Cargo target with tests and the standard test harness enabled. Targets with
`test = false` or `harness = false` and unconditionally or conditionally ignored
tests cannot satisfy the requirement.

{anchor}`placeholder_status`
Implementation references and test annotations inside a Rust module named
`todo` record work not yet done; those inside `not_applicable` record that the
implementation or test does not apply. Each entry retains this status and its
accompanying explanation. Ordinary entries, TODO entries, and Not applicable
entries are displayed separately, even when they reference the same rule.
An empty special module creates no entry: it must contain an implementation
reference or an annotated test to account for the respective requirement.

```rust,ignore
mod not_applicable {
    // {anchor}`example.documentation_rule`
    // This rule describes documentation organization and has no runtime implementation.
}

mod todo {
    #[test]
    #[sage::test(example.documentation_rule)]
    fn documentation_rule() {
        // TODO: Check the generated documentation against the rule.
    }
}
```

The example's `example.documentation_rule` stands for an existing definition;
it does not declare a rule. Placeholder status applies independently to the
implementation and test sides. Nesting a module under both `todo` and `not_applicable` is an error.

{anchor}`evidence_status`
An implementation or test entry establishes a declared association, not proof
that the rule is satisfied. Source inventories retain cfg conditions without
claiming that a site was compiled or a test executed. Anchor pages distinguish
test source, recorded run results, and other evidence such as traces. A passing
structural check or a passing dummy test must not present a TODO or Not
applicable entry as established behavioral evidence.

### Anchor pages and book navigation

{anchor}`anchor_pages`
Every specification anchor has a stable page identifying the rule and showing
its definition, context, implementation entries, and tests/evidence. The page
includes Rust excerpts and access to surrounding source, and exposes each
entry's placeholder status and explanation. Multiple sites remain individually
inspectable rather than being reduced to a single coverage badge.

{anchor}`context_navigation`
Clicking an anchor badge opens its anchor page. The page's context link returns
to the defining Markdown paragraph or `.sage` location. Context permalinks
remain available independently of the anchor-page URL, so navigating to the
rule's connections does not lose access to its original specification text.

<!-- end anchor -->

The book renders markers as local shield-style badges with a `spec` label and
the rule's name. The preprocessor implements this custom role without requiring
the rest of MyST or an external badge service.

{anchor}`book_explorer`
The book build includes the architecture explorer and its model and Rust
source snapshots. An explorer chapter embeds the browser and offers a full-page
view. The handwritten Crates landing page links to `.sage` subchapters that provide
the searchable, interlinked presentation defined by the
[browser specification](book.md#displaying-sage-files). Book generation
and local serving share the model and renderer under its
[delivery rule](book.md#meta.book.browser_delivery).
Published browsing, including the Rust source pane, works without a
running `sage-meta serve` process. Local book rebuilds refresh the snapshot
when the model or its Rust sources change.

{anchor}`explorer_navigation`
The explorer's selected Sage entity or specification anchor is reflected in the
enclosing chapter's URL. Opening that URL restores the selection, and browser
back/forward navigation keeps the chapter and explorer in agreement. Sharing a
selection preserves the book's navigation as well as the selected context.
Every entity is directly addressable under the
[entity-link rule](book.md#meta.book.entity_links); these links are also
available to references from other documentation.

### Validation entry points

Missing implementation entries, missing tests, invalid references, and invalid
definitions fail the shared `sage-meta check`. Explicit TODO and Not applicable
entries account for missing work or inapplicable requirements without disabling
reference validation. The same checks run in CI and workspace tests; making a
plain `cargo build` run them is not required.

The library entry point `sage_meta::check(workspace_root)` loads the
[shared Design inventory](book.md#meta.book.shared_model), including
Markdown and `.sage` chapters, and the Cargo-owned Rust source index.
Its `CheckReport` separates model consistency, Rust correspondence,
specification-test coverage, and the committed
[claim/test review checklist](reviews.md). Missing or stale reviews
fail the shared check. A missing `md/` corpus is an error, not an empty
successful check. `ensure_valid()` rejects any validation errors.
`sage_meta::check::check_model(model_root)` performs the same full check for a
selected model directory, locating the containing Cargo workspace and its
`md/` corpus. Neither entry point runs compiler queries or treats source
coverage as runtime conformance. Model-only library consumers can call
`workspace::load_documents` followed by `validate`.

The CLI's default `cargo run -p sage-meta -- check` invokes this shared workspace
check. The CI runner executes that command, and a Cargo integration test calls
the library with a workspace path derived from its manifest directory, so
`cargo test --all --workspace` performs the same checks regardless of its working
directory. Runtime contract tests separately exercise observed enforcement.
