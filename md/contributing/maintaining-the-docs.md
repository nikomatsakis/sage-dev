# Maintaining This Book

Update the specification with the code in the same change. Read the Design
[overview](../design/README.md), the affected crate's modeled entities, and any
relevant RFD before editing. The book is the source of truth for Sage's current
design; the generated anchor inventory reports implementation and test coverage.

## What belongs where

- **Meta** specifies this book, `.sage`, evidence extraction, checking, and review.
- **Tenets** explains principles and motivation without specification anchors.
- **Crates** starts with a handwritten reading map. Its generated `.sage`
  chapters contain architecture, operations, and claims beside their owning
  entities. Focused Markdown chapters can explain cross-cutting behavior and
  substantial semantic algorithms.
- **Anchors** is generated. Do not maintain copies of claims, source excerpts,
  tests, or status tables there.
- **RFDs** record motivation, alternatives, affected specification pages, and
  implementation sequencing. Their branch includes the normative Design diff.
  Completed records remain useful history; they do not define active rules.
- **The User's Guide** explains how to use tools. Worked examples explain
  concrete scenarios without replacing the specification or its evidence.

There is no separate numbered Architecture Decisions registry or manually
maintained implementation-status catalog. Preserve reasoning in an existing RFD
or an explicitly retrospective record. Plans and ordering belong in RFDs;
per-rule implementation, test coverage, and limitations belong with the
source associations displayed on anchor pages.

## Writing a rule

Give each independently testable claim its own anchor. Follow the
[Anchor tenets](../design/tenets.md#anchor-tenets) when deciding what belongs
together. Prefix the opening paragraph with a local marker, without a blank
line before its text:

```text
{anchor}`cache_invocations`
Every annotated invocation is checked, including cached requests.
```

In `design/meta/contracts.md` this names
`meta.contracts.cache_invocations`. A chapter namespace derives from its path
relative to `design/`, with hyphens normalized to underscores, extensions
removed, and `README` denoting its directory. Headings and sidebar order do
not affect identity. Use the same definition marker inside `.sage` triple-quoted
prose; put the claim beside its owning declaration or method.

A `{spec}` role with a full rule name references the rule without defining it.
Code examples are not definitions. See [anchor mechanics](../design/meta/anchors.md)
for exact syntax, content boundaries, and reference validation.

Keep qualifications needed to interpret the guarantee inside the anchor's
content. Multiple paragraphs or an example can explain one guarantee; split
requirements that can be implemented, tested, or changed independently.

The specified content extends until the next definition, any section heading,
a standalone `<!-- end anchor -->`, or the containing document/prose-block end.
The explicit end lets unrelated commentary follow without another heading.
Blank lines retain subsequent paragraphs, lists, tables, and examples in the
claim. The generated views and saved review use that same complete content.
Keep the end marker on its own line outside code and HTML examples; an end
marker without an active anchor is an error. Inspect the extracted claim when
editing nearby prose, and review enlarged inputs through the normal checklist
workflow. The [content-boundary migration](../rfds/sage-contracts/implementation.md#anchor-content-boundaries)
records the implementation and corpus audit.

Move detailed contracts into `.sage` as their owning entities become clear;
introduce more structural syntax when it makes the requirements more precise.
Formalization must preserve existing requirements.

## Implementation and tests

Connect each claim to both implementation and tests:

```rust,ignore
// {anchor}`meta.contracts.cache_invocations`<<
check_invocation(caller, callee)?;
// >>

#[test]
#[sage::test(meta.contracts.cache_invocations)]
fn cached_invocation_is_checked() {
    // The actual test must visibly exercise and assert the claim.
}
```

A point comment without `<<` gives a five-line preview after the marker. A
region explicitly bounds the excerpt. Multiple sites are supported. Ordinary
entries, entries inside `todo`, and entries inside `not_applicable` retain
independent statuses and explanations. Missing entries are errors; they are
not implicitly TODO. Do not introduce a placeholder when meaningful coverage
already exists, or label a passing placeholder as implemented behavior.

Eligible tests use the built-in `#[test]` attribute in a Cargo target with a
standard enabled test harness. Ignored tests do not supply coverage. Source
inventory retains cfg conditions without claiming that those conditions held
or the test executed. Test adequacy review and test execution are separate.

Use `.agents/skills/sage-review-checklist/SKILL.md` when the claim or its complete
test set changes. Reviewers inspect the exact inputs, including TODO/N/A
explanations. Record reviewed decisions through `sage-meta review record`;
do not rewrite snapshots merely to make CI pass. Unchanged claim/test inputs
retain their saved review even if source line numbers move.

## Navigation and generated content

Register handwritten chapters in `SUMMARY.md`, except companion Markdown
guides under `design/crates/<crate>/`: the Crates chapter includes those through
the shared chapter inventory. Keep `.sage` files in that same crate directory
with a `crate.sage` manifest. The preprocessor generates crate subchapters and
anchor pages; do not create Markdown copies of `.sage` sources. CI's orphan
chapter check uses this inventory, including generated chapters, and rejects
Markdown sources that would not be published. Write links to the relevant
source chapters and use direct entity links when pointing to an operation or
value type.

A source badge opens the rule's generated anchor page. Its context link returns
to the precise definition. Moving or renaming a rule requires updating its
implementation/test references and incoming links together. The modeled
entity identity, such as `$sage_ir::symbols::Symbol`, is independent of the
chapter-derived rule name.

The book snapshots Rust source. Rebuild after Rust edits to refresh excerpts.
It works without a running Sage server, and embedded explorers follow the
book's theme. Keep the handwritten Crates landing page concise and link its
subsystem introductions to the owning modeled crates.

The separate `// ANCHOR:` / `// ANCHOR_END:` excerpt convention remains useful
for explanatory worked examples; it does not establish specification coverage.

## Verification

Run the shared check and build the book:

```sh
cargo run -p sage-meta -- check
mdbook build
```

With the configured HTML and linkcheck renderers, the rendered site is in
`book/html/`. The Pages workflow publishes that directory and checks that
`index.html` exists there before uploading; publishing `book/` instead nests
the site under `/html/` and leaves the site's root URL returning 404.

The same library check runs in workspace Cargo tests. Run focused behavior tests
for changed claims, format Rust, and build the workspace. Inspect the generated
index and representative source → anchor → source links, including `.sage`
claims. Keep TODO/N/A visible until substantive implementation and evidence
replace them. Remove an old chapter only after accounting for its numbered and
unnumbered requirements and preserving useful rationale and links.
