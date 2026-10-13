# Architectural Pseudocode

The `.sage` language exposes the compiler as symbols, contexts, and methods.
Each method states its intent in triple-quoted prose and its allowed dependencies
in a structured envelope. `sage-meta` is the shared build and validation layer
for correspondence among Markdown specifications, `.sage` models, and Rust.
Its library checks declared structure at build time and observed calls during
runtime sessions; the CLI, Cargo tests, CI, and browser use those same rules.
It does not execute Rust semantic specifications.

This review layer follows [D21](../decisions.md#d21-architectural-pseudocode-has-checkable-dependency-envelopes).
Rust semantic rules, potentially specified in a-mir-formality, are separate from
which compiler operation requests a signature, creates an inference context,
or invokes a solver query.

## Files and names

`md/design/crates/<crate>/crate.sage` describes the crate's purpose, logical modules, and
workspace dependencies. Crate names use underscores as in Rust paths. A module
`module symbols;` resides in `symbols.sage`, whose header is
`module crate::symbols;`. `$sage_ir::symbols::FnSymbol::signature` is an absolute
cross-crate method reference; `crate::symbols::FnSymbol::signature` refers to
the current crate. Cross-crate references require a declared dependency.

Manifests describe logical structure rather than duplicating Rust source modules.
The Rust implementation declares correspondences with `#[sage::item(...)]`.
Third-party
libraries are currently outside the modeled crate graph.

## Objects and operations

{anchor}`meta.classes.single_inheritance`
A class or context declaration has at most one `extends` base.

An abstract class can declare abstract methods.

{anchor}`meta.classes.abstract_complete`
A concrete class must implement every inherited abstract method.

{anchor}`meta.classes.override_signature`
An overriding method must preserve the inherited method's operation kind and
resolved result type.

{anchor}`meta.contexts.inheritance`
Contexts are a distinct declaration kind and may inherit only from contexts.
An ordinary or abstract class cannot inherit from a context.

Contexts describe computation-local working state. Their intended ownership
does not by itself establish runtime allocation or escape checking.

{anchor}`meta.fields.kinds`
Fields retain their declared `identity`, `input`, or `state` kind, distinguishing
identity from changing inputs and local working state.

{anchor}`meta.fields.context_state`
A `state` field is valid only in a context declaration.

{anchor}`meta.methods.kinds`
Methods retain their declared operation kind: `query` or `fn`. Resolving a
method's contract preserves that distinction.

Queries describe memoized semantic products; functions describe ordinary
computation. These annotations document boundaries; the model has no evaluator
or cache.

{anchor}`meta.methods.prose`
Every method body begins with a triple-quoted description, followed by its
`envelope` or `abstract;` declaration. The description is retained as prose;
method-like text within it adds no dependency permissions.

### Dependency envelopes

{anchor}`meta.envelopes.reads`
`reads self.field;` permits reading a field declared on this receiver's class
or inherited from a base. Unknown fields and non-`self` field paths are errors.

{anchor}`meta.envelopes.creates`
`creates name: crate::module::Context;` declares a fresh owned working context
under the given binding name. Its target must resolve to a context declaration.
This describes context creation; validation does not perform the allocation.

{anchor}`meta.envelopes.receiver_calls`
`invokes self.method();` permits a call on the current semantic receiver.
A matching method on a different receiver does not satisfy that permission.

{anchor}`meta.envelopes.class_calls`
`invokes crate::module::Class::method();` permits a call on any instance of the
named class or a known subclass. This permission does not require equality
with the caller's receiver.

{anchor}`meta.envelopes.leaf`
An empty envelope describes a leaf operation with no modeled dependencies.
It is distinct from an abstract method, which has no declared envelope.

{anchor}`meta.envelopes.permissions`
Declared calls are permissions: none must occur, and calls may occur in an
order different from the order of their envelope clauses.

{anchor}`meta.envelopes.direct_calls`
Each invocation clause grants a direct dependency. Transitive reachability is
derived from those direct edges and does not grant the caller permission to
invoke an indirect dependency directly.

{anchor}`meta.model.forbidden_paths`
`forbid transitive crate::module::Class::method;` prohibits reaching its target
over any permitted nonempty call path. A rejected path identifies the offending
method sequence.

{anchor}`meta.model.virtual_dispatch`
The checker resolves declared calls conservatively, including every known
overriding implementation in the supplied model.

{anchor}`meta.model.inherited_prohibitions`
An overriding implementation inherits the prohibitions of the base method.

Prohibitions record design intent even when the permitted graph already
excludes the target. Adding a permitted edge must still fail validation if it
creates a forbidden path.

## Semantic value types

Value declarations complement identity-bearing classes. They describe semantic
contents without prescribing Rust storage layout.

### Declarations and prose

{anchor}`meta.types.struct_fields`
A `struct` contains named, typed fields. The parsed declaration retains their
names, types, and source order; an empty struct is permitted.

{anchor}`meta.types.enum_variants`
An `enum` contains named variants in source order. A variant can have no
payload, an ordered tuple payload, or named, typed fields. These three shapes
remain distinct, including when a tuple or named payload is empty.

{anchor}`meta.types.generic_parameters`
Structs, enums, and opaque types can declare a nonempty comma-separated list of
type parameters in angle brackets. Parameters are available throughout their
own declaration and do not become names in other declarations.

{anchor}`meta.types.opaque_representation`
An opaque `type` declares a name and any type parameters without fields or
variants. It requires triple-quoted descriptive prose, either inside braces
or before a terminating semicolon. A payload inside its body is an error.

{anchor}`meta.types.declaration_prose`
A struct or enum can carry triple-quoted prose immediately inside its opening
brace. The parsed declaration retains that text; omitting it is permitted.

{anchor}`meta.types.field_prose`
A named field can carry triple-quoted prose immediately before its name.
The parsed field retains that text independently of the containing declaration
or variant's prose. At the start of a body, the first prose string describes
the container; a description for its first field follows as a second string.

{anchor}`meta.types.variant_prose`
A variant can carry triple-quoted prose immediately inside its payload's
opening delimiter, or immediately after a unit variant's name. The parsed
variant retains that text independently of other variants.

{anchor}`meta.types.member_commas`
Adjacent named fields, enum variants, and tuple payload elements require a
comma separator. A trailing comma before the closing delimiter is optional.

For example:

```sage
type Binder<T> {
    """A value under the generic parameters binding its references."""
}

struct Items {
    items: Vec<crate::symbols::Symbol>,
}

enum MethodReceiver {
    Value { mutable_binding: bool },
    Ref {
        """A borrowed receiver with the declared mutability."""
        mutability: Mutability,
    },
}
```

### Type expressions and validation

{anchor}`meta.types.type_expressions`
Type expressions are named types with optional generic arguments, or tuples.
Generic arguments and tuple elements are themselves type expressions, allowing
arbitrary nesting. The parsed expression retains that nesting.

{anchor}`meta.types.name_resolution`
An unqualified type name resolves first to a parameter of its declaration,
then to a type, class, or context in the same module, then to the built-in
prelude. An earlier match shadows a later one.

{anchor}`meta.types.prelude`
The built-in prelude contains `Vec<T>`, `Option<T>`, `bool`, `u32`, `u64`, and
`String`. These names are available without declaring them in the model.

{anchor}`meta.types.qualified_names`
A type outside the current module requires a `crate::` or `$cratename::`
qualified path. Cross-crate type references require a declared dependency;
unqualified lookup does not search other modules or the crate root.

{anchor}`meta.types.tuples`
Parentheses always denote a tuple, including `()` and a single element with or
without a trailing comma. `(T)` is distinct from `T` when result types are
compared for an override.

{anchor}`meta.types.argument_arity`
Each named type application must supply exactly the declared number of type
arguments. This applies to opaque types, built-in types, parameters, and nested
applications; a type parameter takes zero arguments.

{anchor}`meta.types.unknown_names`
Every type name in a value field, variant payload, class or context field, or
method result must resolve. An unknown name is an error even when nested in
generic arguments or tuples.

{anchor}`meta.types.unique_parameters`
A value declaration cannot repeat a type parameter's name.

{anchor}`meta.types.unique_fields`
Named fields have distinct names within a struct or a single enum variant.
Different declarations and different variants may reuse field names.

{anchor}`meta.types.unique_variants`
Variants have distinct names within an enum. Different enums may reuse a
variant name.

{anchor}`meta.types.recursive_values`
Semantic value types can refer directly to themselves or to each other, without
`Box` or another storage indirection. Such recursion is not a validation error.

Override result types follow the
[resolved signature rule](#meta.classes.override_signature).

### Current model examples

The current model erases `Stashed`, pointer/slice storage, and database lifetimes
while retaining semantic fields. `sage_ir/types.sage` describes `Ty`, aliases, constants, signatures,
receiver forms, parameter environments, and trait items from `ty.rs`.
`Binder<T>` is intentionally prose-defined. The model preserves current
limitations such as type-only ADT arguments and the dummy lifetime variant.
The broad `Signature` sum and `Items` wrapper are labeled review abstractions,
not Rust runtime data structures. Fn/Trait/Impl methods use a uniform optional
interface, lifting the Rust local results to `Some` while retaining unavailable
external results as `None`.

## Rust-declared correspondence

Rust items carry inert `#[sage::item(...)]` tool attributes. Each Rust crate
registers the `sage` tool namespace on the pinned nightly. Attribute arguments
name architectural items, not Rust declarations; `crate` means the Cargo
package's modeled crate, with hyphens normalized to underscores. An explicit
crate name must identify that same owner. Cargo library and binary targets are
both inventoried, even when their target names differ from their package name.

Correspondence units are crate/module headers, named type/class/context
declarations, and explicitly declared methods. Fields, variants, generic
parameters, and envelope clauses remain parts of their containing declaration.
Every unit requires at least one Rust anchor; every annotation must resolve to
a compatible unit in its own crate. Rust attributes are the single
correspondence authority; `.sage` declarations contain no Rust-origin annotation.

A declaration can have several Rust implementations. Non-executable anchors
may carry several correspondences; an executable function has one operation
identity. An empty annotated `mod r#abstract {}` anchors a concept spread over
multiple implementation sites, an abstract interface, or an external/primitive
representation. A module anchor neither maps all descendants nor supplies
runtime execution evidence. Concrete method markers count as source locations
only; executable coverage is reported separately.

The index reads unexpanded Rust syntax, preserves source ranges and textual cfg
conditions, and traverses modules and methods. The known `define_kind_symbols!`
input grammar has an explicit adapter; arbitrary macro token text is not proof
of a generated declaration. Source coverage across cfg alternatives is distinct
from resolving a query observed in a particular compiled binary.
Conditional Sage annotations or module paths inside `cfg_attr` are rejected;
place unconditional annotations on separate `cfg`-gated declarations instead.

The top-level `sage` facade has no doctests and disables its rustdoc test
harness: rustdoc injects `--extern sage`, which shadows the registered tool
namespace. Its unit and integration tests remain enabled. Other workspace
crates retain their doctests.

## Observed contracts

An immutable resolved contract model shares the name/dispatch rules of static
validation. A session starts from explicitly allowed root operations.

{anchor}`meta.calls.declared`
Every observed architectural call must be permitted by its caller's resolved
dependency envelope. An undeclared call is an error, including recursive entry
into the same operation and requests satisfied from a query cache.

{anchor}`meta.calls.forbidden`
An observed operation must not violate a prohibition of any active ancestor,
even when its immediate caller permits the call. A violation is an error and
retains the prohibition and complete call path.

Receiver-sensitive permissions require semantic receiver identity; missing
identity remains an observation gap. Query-engine internal reexecution installs
context without fabricating a source call, while retaining ancestor prohibitions.
Violations remain in the session report; checked callers reject them at the
report boundary with `assert_calls_conform()`. Salsa callbacks record violations
without unwinding through query-engine lifecycle handling.

A Salsa adapter joins structured query source provenance to the Rust index,
then feeds request and execution events into that session. Cached requests still
need permission. Source snapshots supplied by the caller must match the indexed
text before a compiled query can be treated as mapped. Missing, ambiguous,
stale, and unsupported matches prevent a conformance success. No debug-name
matching or automatic envelope broadening is permitted.

Correspondence, observed calls, and unchecked effects are distinct results.
Inert annotations do not instrument ordinary functions; input reads, context
construction, task propagation, and unconstrained generated code need additional
hooks. A passing run establishes only its explicitly observed scope. The
[Sage Contracts and Walkthroughs RFD](../../rfds/sage-contracts/README.md) stages
broader compiler coverage, browser replay, and agent tools.

## Interactive architecture explorer

The [browser specification](../meta/book.md#displaying-sage-files) defines
declaration rendering, search, dependencies, Rust source display, entity links,
and delivery. The following commands describe the current implementation.

`cargo run -p sage-meta -- browse` validates the model and exports a standalone
`target/sage-browser.html`. The exporter embeds serialized AST documents and the
browser assets; no server or frontend dependency installation is required.
The [Crates](../crates.md) section embeds the same explorer in the built book,
with crate overviews and a subchapter for each `.sage` module. Its sources live
in `md/design/crates/`; the default model loader follows `[book].src` in
`book.toml`. The optional positional arguments select the model directory and
output HTML:
`cargo run -p sage-meta -- browse md/design/crates /tmp/sage-architecture.html`.

The explorer provides crate/module navigation, search and kind filters,
linked value types and variants, Rust source correspondences, base classes,
methods, and dependency envelopes. Method pages show conservative direct-call
targets, reverse callers, overriding implementations, and inherited prohibitions.
Hash routes support bookmarks and browser history. Press `/` to search. Click a Rust annotation or the **Rust source** header button to reveal a source
pane with the definition highlighted. The pane follows navigation and links
methods to their own annotated implementation or marker sites. It can be hidden without leaving the architecture view.

Exported files are immutable snapshots: regenerating is required after model
edits. The browser inspects declared architecture rather than running compiler
queries; it is separate from the live Semantic Inspector. The serialized model
is escaped for HTML script embedding and all model-derived text is escaped
when rendered. Browser code lives in `crates/sage-meta/web/`; `src/browser.rs`
is the export boundary, and `main.rs` provides the command.

## Specification tests and workspace checks

The [Design chapter tree](../meta/book.md#meta.book.authority) forms the canonical
specification, with Markdown and `.sage` as its source formats. Named anchors
connect that specification to implementation and tests. The following
paragraphs state the destination; [current status](#specification-anchor-status) records which
parts are implemented and which have explicit placeholders. The rollout is
tracked in the [anchor-system plan](../../rfds/sage-contracts/implementation.md#canonical-anchor-system).

### Definitions and references

{anchor}`meta.spec.canonical_sources`
A specification rule can be defined in a Markdown or `.sage` Design chapter,
with equal authority. Its definition records the rule's text and source
location; a `.sage` definition also identifies its containing declaration, method, or
envelope clause. Either documentation form can reference a rule defined in
the other without creating another definition. Every reference must resolve to
an existing definition; a dangling reference is an error.

{anchor}`meta.spec.unique_names`
Every specification anchor has a well-formed full name and exactly one
definition in the Design chapter inventory. Malformed markers and duplicate
full names are errors. Names are
[derived from the defining chapter](../meta/book.md#meta.book.chapter_names);
[moves between chapter namespaces](../meta/book.md#meta.book.anchor_moves)
rename the rule and require updating its references.

Resolved names are dot-separated ASCII identifiers and describe independently
testable behavior. A Markdown definition uses a chapter-local name with the
custom MyST-style inline-role syntax. For example, in `design/meta/calls.md`:

```text
{anchor}`declared`
Every observed architectural call must be permitted by its caller's resolved
dependency envelope.
```

The marker and its prose form one paragraph: do not leave a blank line between
them. The example resolves to `meta.calls.declared`. Fenced examples, inline
code, and ezanchor excerpt blocks do not declare specification anchors. The
exact syntax for definitions and references inside
`.sage`, and for references in Markdown, is a remaining grammar decision;
the shared identity and correspondence requirements above do not depend on it.

### Rust implementation entries

{anchor}`meta.spec.implementation_references`
A Rust implementation entry is introduced by a
<code>// {anchor}&#96;meta.calls.declared&#96;</code> comment, optionally followed
immediately by `<<`. Its name must resolve to a specification definition;
malformed and unknown references are errors. Only actual Rust comments create
these entries: marker-like text inside string literals does not. Entries retain
their source location and enclosing module, and multiple entries may reference
the same rule, including across crates.

{anchor}`meta.spec.implementation_coverage`
Every specification anchor has at least one explicitly declared Rust
implementation entry. An anchor without one is an error. A test annotation or
`#[sage::item(...)]` correspondence alone does not supply this entry. A
placeholder implementation counts as an entry with its explicit status.

{anchor}`meta.spec.point_excerpts`
A point reference has no `<<` suffix. Its initial excerpt contains the marker
line and the next five source lines, truncated at end-of-file. A **See more…**
action opens the surrounding source at that location; the five-line preview
does not claim to delimit the implementation.

```rust,ignore
// {anchor}`meta.calls.declared`
check_declared_call(caller, callee)?;
```

{anchor}`meta.spec.region_excerpts`
A reference ending in `<<` opens an explicit region, and a standalone `// >>`
comment closes the nearest open region in the same file. Regions may nest.
The excerpt spans the opening marker line through its closing marker line,
with access to the surrounding source. A point reference inside a region
remains a point reference and does not close or replace that region.

```rust,ignore
// {anchor}`meta.calls.declared`<<
let caller = current_operation();
check_declared_call(caller, callee)?;
record_call(caller, callee);
// >>
```

{anchor}`meta.spec.region_balance`
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
#[sage::test(meta.calls.declared)]
#[sage::test(meta.calls.forbidden)]
fn reports_direct_and_ancestor_violation_with_complete_path_and_rule() {
    // Assert the observed edge and the violated ancestor's prohibition.
}
```

{anchor}`meta.spec.test_coverage`
Every specification anchor must have at least one annotated Rust test. An
anchor without a test is an error, independently of its implementation
coverage. A placeholder test counts as an entry with its explicit status; an
implementation placeholder alone cannot satisfy the test requirement.

{anchor}`meta.spec.test_references`
Every `#[sage::test(...)]` reference must resolve to an existing specification
anchor and be attached to a Rust test. Dangling and misplaced references are
errors.

One test can cover several rules and a rule can have several tests. Unlike
`sage::item`, a test may reference a rule across crate boundaries. An eligible
test entry is a module-level function with the built-in `#[test]` attribute in
a Cargo target with tests and the standard test harness enabled. Targets with
`test = false` or `harness = false` and unconditionally or conditionally ignored
tests cannot satisfy the requirement.

{anchor}`meta.spec.placeholder_status`
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
implementation and test sides. How to handle a module nested under both
`todo` and `not_applicable` remains an explicit implementation-plan decision.

{anchor}`meta.spec.evidence_status`
An implementation or test entry establishes a declared association, not proof
that the rule is satisfied. Source inventories retain cfg conditions without
claiming that a site was compiled or a test executed. Anchor pages distinguish
test source, recorded run results, and other evidence such as traces. A passing
structural check or a passing dummy test must not present a TODO or Not
applicable entry as established behavioral evidence.

### Anchor pages and book navigation

{anchor}`meta.spec.anchor_pages`
Every specification anchor has a stable page identifying the rule and showing
its definition, context, implementation entries, and tests/evidence. The page
includes Rust excerpts and access to surrounding source, and exposes each
entry's placeholder status and explanation. Multiple sites remain individually
inspectable rather than being reduced to a single coverage badge.

{anchor}`meta.spec.context_navigation`
Clicking an anchor badge opens its anchor page. The page's context link returns
to the defining Markdown paragraph or `.sage` location. Context permalinks
remain available independently of the anchor-page URL, so navigating to the
rule's connections does not lose access to its original specification text.

The book renders markers as local shield-style badges with a `spec` label and
the rule's name. The preprocessor implements this custom role without requiring
the rest of MyST or an external badge service.

{anchor}`meta.spec.book_explorer`
The book build includes the architecture explorer and its model and Rust
source snapshots. An explorer chapter embeds the browser and offers a full-page
view. The Crates chapter and its `.sage` subchapters provide the searchable,
interlinked presentation defined by the
[browser specification](../meta/book.md#displaying-sage-files). Book generation
and local serving share the model and renderer under its
[delivery rule](../meta/book.md#meta.book.browser_delivery).
Published browsing, including the Rust source pane, works without a
running `sage-meta serve` process. Local book rebuilds refresh the snapshot
when the model or its Rust sources change.

{anchor}`meta.spec.explorer_navigation`
The explorer's selected Sage entity or specification anchor is reflected in the
enclosing chapter's URL. Opening that URL restores the selection, and browser
back/forward navigation keeps the chapter and explorer in agreement. Sharing a
selection preserves the book's navigation as well as the selected context.
Every entity is directly addressable under the
[entity-link rule](../meta/book.md#meta.book.entity_links); these links are also
available to references from other documentation.

### Validation entry points

Missing implementation entries, missing tests, invalid references, and invalid
definitions fail the shared `sage-meta check`. Explicit TODO and Not applicable
entries account for missing work or inapplicable requirements without disabling
reference validation. The same checks run in CI and workspace tests; making a
plain `cargo build` run them is not required.

The library entry point `sage_meta::check(workspace_root)` loads the
[shared Design inventory](../meta/book.md#meta.book.shared_model), including
Markdown and `.sage` chapters, and the Cargo-owned Rust source index.
Its `CheckReport` separates model consistency, Rust correspondence,
specification-test coverage, and the committed
[claim/test review checklist](../meta/reviews.md). Missing or stale reviews
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

## Verification authority

Model consistency, source correspondence, specification-test coverage, and
observed runtime conformance answer different questions. An omitted `.sage`
edge can leave the static model consistent; the runtime checker reports an
error if that edge is observed. Full implementation conformance additionally
requires every relevant boundary to be observed. A source annotation alone
cannot establish that coverage. See the runtime evidence and limitations below
and the [query-boundary evidence](../infrastructure/incrementality.md).
Type and trait algorithms remain prose boundaries; a formal semantics
integration is prospective.

## Current status

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

### Specification anchor status

The canonical anchor system above and the [specification book](../meta/book.md)
are plans under review. Today the checker
indexes Markdown definitions and `#[sage::test(...)]` references, checks
Markdown-name uniqueness and test coverage, and rejects invalid test placements.
It loads `.sage` from `md/design/crates/` and scans all Markdown under `md/`; it
does not yet restrict definitions to the Design inventory or derive their names
from chapters. Full names in markers remain a migration spelling until local
names and the planned `design/meta/spec.md` chapter are supported.
It does not yet index `.sage` definitions/references or Rust implementation
comments, enforce implementation coverage, extract point/region previews, or
interpret implementation placeholder status. The review checklist classifies
annotated tests in `todo` / `not_applicable` modules and reports reviewed
placeholders separately from approved behavioral tests. Badges currently link directly to
their paragraphs; anchor pages and anchor-page navigation are not implemented.
The Crates chapters now embed the shared explorer, with source chapters
selecting their modeled crate or module and the enclosing URL tracking entity
selection. Direct entity links and browser Back/Forward restore that selection.

`crates/sage-meta/tests/spec.rs` contains behavioral tests for the existing
Markdown and test-reference rules. In particular, its evidence for
[`meta.spec.unique_names`](#meta.spec.unique_names) is limited to Markdown.
`crates/sage-meta/tests/spec_plan.rs` contains explicitly empty tests in a
`todo` module for the additional requirements. They keep each new rule linked
to a test while the specification is reviewed. Their passing results are
placeholder bookkeeping, not evidence that the planned behavior works. The
review report preserves their TODO status. Replace them with behavioral tests as the
[staged implementation](../../rfds/sage-contracts/implementation.md#canonical-anchor-system)
lands; then add or migrate implementation entries for the entire anchor corpus.

### Model and correspondence evidence

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

See the [review-language roadmap slice](../../implementation/roadmap.md#architectural-review-language)
for coverage work beyond this initial conversion.

The Rust browser integration tests exercise model rejection, script-safe
embedding, and repository export. After generating the snapshot,
`node crates/sage-meta/web/browser.test.cjs` checks search, navigation, every
rendered model link, conservative dispatch, and HTML escaping. These checks do
not substitute for visual layout testing in a browser engine.


### Embedded Rust source

The exporter embeds files reached by the same Rust correspondence index used
by validation. The pane supports multiple implementation sites and labels
source-only module markers and known generator inputs. Source navigation is
available for crate/module headers, types, and individual methods. It does not
resolve guessed Rust type names or require maintaining reverse annotations.

The snapshot does not evaluate cfg or expand arbitrary macros. Regenerate it
after Rust edits; no source server is required. The CLI's correspondence report
states both source-site and executable-declaration counts. An executable
source declaration is not, by itself, proof of runtime instrumentation.

### Contract infrastructure evidence

`tests/runtime_contracts.rs` supplies annotated evidence for
[`meta.calls.declared`](#meta.calls.declared) and
[`meta.calls.forbidden`](#meta.calls.forbidden). It exercises the shared checking
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

### Serve the explorer locally

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
