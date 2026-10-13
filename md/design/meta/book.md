# Specification Book

This chapter specifies the book that contains it. Its rules govern the source
chapters, generated views, and the workflow for changing the specification.
The [anchor mechanics](anchors.md#specification-tests-and-workspace-checks)
define implementation references, tests, excerpts, and placeholder entries.
The [migration plan](../../rfds/sage-contracts/implementation.md#canonical-anchor-system)
tracks the work; [Current status](../../rfds/implementation-sequencing/coverage-history.md#book-rendering) distinguishes the rules below
from today's tooling.

## Authority and source chapters

{anchor}`authority`
The mdBook Design chapter tree is the canonical specification. Markdown and
`.sage` are equally authoritative source formats within that tree. Every
active rule definition belongs to a Design chapter with identifiable source
context; its appearances in generated indexes or anchor pages refer to that
definition and do not create additional definitions.

{anchor}`source_roles`
The Design overview contains Meta, Tenets, Anchors, Crates, and RFDs. Meta
specifies the specification system itself. Tenets explains design principles
without defining specification anchors. Anchors is generated from all active
specification rules. Crates combines a handwritten reading map and modeled
crate chapters. RFDs record changes and reasoning without defining active
rules. The User's Guide provides non-normative usage instructions.

{anchor}`self_description`
The specification-book system is itself specified by anchors in Design. Its
rules participate in the same naming, reference validation, implementation and
test coverage, status reporting, index, and anchor-page generation as all other
rules. A rule about the validator or renderer receives no exemption from those
requirements; unfinished work uses explicit TODO entries.

{anchor}`sage_chapters`
`.sage` sources reside within the book's configured source directory and each
is rendered as a Design chapter. Design contains a Crates chapter listing the
modeled crates. Each crate has an overview chapter showing its purpose,
dependencies, and modules, with its remaining `.sage` source chapters nested
beneath it. These chapters present the `design/crates/` source tree using the
[architecture browser specified below](#displaying-sage-files). Each source has
one chapter in the shared inventory; rendering does not create a second
canonical copy. Rendering requires no hand-maintained Markdown copies of
`.sage` sources and the built book works without a running Sage server.

{anchor}`crate_ownership`
Every `.sage` chapter lies beneath a directory identifying its Rust crate.
The crate's `crate.sage` manifest establishes the modeled crate boundary;
the book loader checks that ownership against the Rust workspace. Book chapter
namespaces and compiler-model item identities are distinct: moving a crate's
book directory preserves identities such as `$sage_ir::symbols::Symbol` when
the modeled crate and declaration paths are unchanged.

{anchor}`crates_landing`
The Crates landing page preserves handwritten Markdown from
`design/crates/README.md`. Its curated subsystem introductions link to rendered
crate chapters and modeled entities. Generated crate and module chapters are
children of this page; generation does not replace its prose with the explorer.
Links written to `.sage` source chapters resolve to their rendered chapters.

<!-- end anchor -->

The configured book source is currently `md/`. Generated indexes and anchor
pages are views of its specification sources, not additional definitions.

## Displaying `.sage` files

The architecture browser presents the declared model for review and navigation.
These rules define its behavior in the Crates chapter, standalone HTML exports,
and the local server. The browser does not evaluate `.sage` prose or execute
compiler queries merely because a reader opens a declaration.

{anchor}`browser_structure`
Readers can navigate from crates to modules, declarations, and their members,
with the owning crate, module, and declaration visible at each level. Crate
views show purpose and dependencies. Declaration views show kind, descriptive
prose, generic parameters, and the modeled structure: struct fields, enum
variants and their fields, or the description of an opaque type. Class and
context views show fields, base classes, and methods, distinguishing declared,
inherited, abstract, and overriding methods. Method views show query/function
kind, signature, and prose. References to modeled types, bases, and methods
link to their declarations.

{anchor}`entity_links`
Every declared Sage entity has an unambiguous direct link, including crates,
modules, types, classes, contexts, methods/queries, and named members such as
fields and enum variants. Opening a link selects the declaration or member in
its rendered context without a prior search or browser session. Links resolve
through the entity's identity and owning scope: equal local names in different
owners remain distinct, and relocating a chapter does not break links to
unchanged model entities. Markdown and `.sage` sources can reference these
identities; rendered references link to the corresponding entity and unresolved
references are errors.

Entity links and specification anchors have different purposes. For example,
`$sage_ir::symbols::Symbol` identifies a modeled declaration, whereas
`meta.book.entity_links` identifies a testable rule. Making a declaration
linkable does not create a specification rule or require an extra named
paragraph. The authoring syntax for entity references and URL spelling remain
implementation-plan decisions. Selection URLs and browser history follow the
[navigation rule](anchors.md#meta.anchors.explorer_navigation).

{anchor}`browser_search`
Search finds declared entities by name, qualified model path, descriptive
prose, and corresponding Rust paths when available. Crate and entity-kind
filters combine with the search text. Results identify each entity's kind and
owner and link directly to it, including named members. The view states when
no entities match. Changing search or filters does not invalidate an entity's
direct link or prevent opening a linked entity outside the current results.

{anchor}`browser_dependencies`
Method views render each declared envelope clause with its effect kind and
target: invocations, input reads, context creation, and transitive prohibitions.
They expose possible direct callees, reverse callers, and overriding
implementations, using the validator's conservative dispatch resolution.
Inherited prohibitions retain their declaring method. Linked targets lead to
the corresponding entities. These views label declared permissions and
prohibitions as such; they do not imply that a permitted call happened or that
runtime conformance has been established.

{anchor}`browser_rust_source`
Readers can reveal or hide a Rust source pane without leaving the selected
Sage entity. The pane follows selection and uses indexed Rust correspondences,
showing the file, line numbers, and highlighted declaration with surrounding
source. Multiple mapped sites are individually selectable. Missing source is
reported explicitly. Module markers, macro-input sites, and cfg conditions
remain distinguishable from a confirmed active executable implementation.
The pane identifies its contents as a source snapshot; navigating to code does
not establish that the code executed or satisfied a specification rule.

{anchor}`browser_delivery`
Book generation, standalone export, and local serving use the same resolved
model and renderer to implement these browsing rules. Invalid models produce
diagnostics instead of a partially valid explorer. A generated book or
standalone HTML artifact includes the data and assets needed for navigation,
search, entity links, and Rust source display without a Sage server. Each view
identifies itself as a snapshot. Model or Rust edits become visible when that
snapshot is rebuilt, exported again, or regenerated by restarting the server.

{anchor}`browser_serve`
The `sage-meta serve <port>` command validates and renders the selected model
before serving its snapshot on the loopback interface. It reports the local
URL and serves until stopped. The port must be an integer from 1 through 65535;
an optional model path selects the input. Invalid arguments, model errors, and
binding failures produce a failing exit status with a diagnostic. Opening the
reported URL provides the specified browser without another serving command.

For example, `cargo run -p sage-meta -- serve 3001` serves the snapshot at
`http://127.0.0.1:3001/`. `cargo run -p sage-meta -- browse` writes a standalone
HTML snapshot. The commands are ways to deliver the browser specified here.

## Chapter-derived names

{anchor}`chapter_names`
An anchor's full name combines its chapter namespace with the local name
written at its definition. The namespace is derived from the chapter source
path relative to Design, removing the extension and joining path components
with dots. For example, local `point_excerpts` in `design/meta/anchors.md` names
`meta.anchors.point_excerpts`. The same derivation applies to `.sage` chapters.
Section headings, chapter titles, and sidebar ordering do not change these
names; only the source path and local name contribute. Ambiguous
namespace derivations or duplicate full names are errors.

The intended Markdown source spelling is local:

```text
{anchor}`point_excerpts`
A point reference displays its marker line and the next five source lines.
```

Rust references use the resolved full name, such as
`#[sage::test(meta.anchors.point_excerpts)]` or
<code>// {anchor}&#96;meta.anchors.point_excerpts&#96;</code>. Filename components must normalize to ASCII identifiers; different source
paths that normalize to the same chapter namespace are rejected.

{anchor}`anchor_moves`
Moving a rule within its chapter preserves its identity. Changing its local
name or moving it into a different chapter namespace renames the rule and
requires updating specification, implementation, and test references. Published
links to the old location retain a redirect or context alias. A compatibility
link does not create a second canonical definition or satisfy a stale code
reference.

## Generated review views

{anchor}`index_rows`
The Design section includes an automatically generated index of every active
anchor. Each anchor has a main row containing the final component of its name, with the namespace
supplied by the enclosing group or page heading, and separate implementation
and test statuses. The name's tooltip shows its full identity. A disclosure
control opens a second row containing its rendered specification text. Hiding the text preserves the main
row. The linked name opens that anchor's own page, independently of expanding
or collapsing the text.

{anchor}`index_groups`
The anchor index groups entries hierarchically by their name prefixes, such as
`meta` and `meta.book`. Each group can reveal or hide all its descendants, and
reports implementation and test status counts for that subtree. Grouping and
counts are derived from the same active anchors displayed in the index.

{anchor}`index_namespace_pages`
The generated Anchors landing page lists every active rule. Each namespace
prefix has a page with the same table restricted to that prefix. A rule named
`meta.foo.bar` has a direct page under `design/anchor/meta/foo/bar`; the page
shows the rule and navigation within its surrounding namespace. If a rule's
name is also a prefix of other rules, one page presents both its own claim and
its descendants. Generated pages do not create new specification definitions.

{anchor}`index_status`
Implementation and test status are computed independently from their declared
entries: ordinary entries display **Implemented**, `todo` entries display
**TODO**, and `not_applicable` entries display **N/A**. Mixed statuses remain
visible, including outstanding TODOs when another entry is implemented. An
absent implementation or test entry is a validation error, not an implicit
TODO or N/A. These labels describe declared coverage; recorded run results
remain separate evidence.

<!-- end anchor -->

The [anchor-page rule](anchors.md#meta.anchors.anchor_pages)
specifies each rule's page, with links to definition context, implementation,
and tests/evidence. It applies to this chapter's rules as well. The index
summarizes those same connections and never maintains its own status list.

The [interactive index sketch](../../rfds/sage-contracts/anchor-index-sketch.html)
illustrates the group disclosures, text rows, and anchor-page navigation. Its
sample statuses explore the presentation; they are not a validation report.

{anchor}`shared_model`
Book generation and `sage-meta check` consume the same resolved Design chapter
inventory, rule identities, source provenance, and correspondence data. Plugins
that contribute specification chapters participate in that inventory. A source
chapter omitted from the intended Design inventory is diagnosed rather than
silently checked but unpublished or published but unchecked. Generated copies
of rule text are not re-indexed as new definitions. The CLI, CI, and workspace
test apply the shared validation to the book system's own rules too.

## Changing the specification

{anchor}`rfd_changes`
An RFD branch contains both the design record and the proposed normative Design
changes. The RFD records motivation, alternatives, goals, and a high-level
overview of the required changes, with links to the affected Design pages and
anchors. Its implementation plan develops that overview into stages.
Acceptance merges the record and specification changes together; unfinished
implementation or tests remain explicit TODO entries. Subsequent implementation
replaces those entries with code and evidence. Completing an RFD does not require
copying its rules into Design, because the normative changes were already
expressed there.

## Implementation and test evidence

The generated [anchor pages](../anchor/meta/book.html) report implementation entries,
annotated tests, explicit TODO/N/A gaps, and saved claim/test reviews. These
associations do not themselves establish that a test ran.

The [earlier coverage snapshot](../../rfds/implementation-sequencing/coverage-history.md#book-rendering)
preserves the former capability descriptions, edit experiments, and limitations
as historical evidence. Current coverage comes from the generated entries;
implementation plans and decision rationale belong in the relevant RFDs.
