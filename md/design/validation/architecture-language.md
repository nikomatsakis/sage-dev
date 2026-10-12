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

`sage-src/<crate>/crate.sage` describes the crate's purpose, logical modules, and
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

Classes support a single base class. An abstract class can declare abstract
methods.

{anchor}`meta.classes.abstract_complete`
A concrete class must implement every inherited abstract method.

{anchor}`meta.classes.override_signature`
An overriding method must preserve the inherited method's operation kind and
resolved result type.

Contexts hold computation-local working state and may inherit only from
contexts. Fields distinguish identity, changing input, and context state.
Queries identify memoized semantic products; `fn` identifies ordinary
computation. These annotations document boundaries; the model itself has no
evaluator or cache.

Each method starts with prose, followed by an envelope containing:

- `reads self.field;`: permission to read a declared field, including inherited fields;
- `creates name: crate::module::Context;`: a fresh owned working context;
- `invokes self.method();`: permission to invoke a method on this receiver;
- `invokes crate::module::Class::method();`: permission to invoke that method
  on any instance of the named class or a known subclass; and
- `forbid transitive crate::module::Class::method;`: a prohibition over every
  nonempty call path reachable from this operation.

An empty envelope is a leaf in this model. Prose can describe internal work at
that abstraction level, but modeled calls must be declared explicitly. Calls
are permissions, not a list of required executions or an execution sequence.
Direct permissions produce the call graph; indirect dependencies are derived.

<a id="meta-a1"></a>

{anchor}`meta.model.forbidden_paths`
No permitted nonempty call path may reach a transitively forbidden operation.
A rejected path identifies the offending method sequence.

{anchor}`meta.model.virtual_dispatch`
The checker resolves declared calls conservatively, including every known
overriding implementation in the supplied model.

{anchor}`meta.model.inherited_prohibitions`
An overriding implementation inherits the prohibitions of the base method.

Prohibitions record design intent even when the permitted graph already
excludes the target. Adding a permitted edge must still fail validation if it
creates a forbidden path. The `meta-a1` link remains an alias for this group of
rules, formerly presented as META-A1.

## Semantic value types

Value declarations complement identity-bearing classes. `struct` defines named
fields; `enum` defines unit, tuple, or named-field variants. Both can declare
generic parameters. Triple-quoted prose can describe the declaration, a named
field, or an individual variant. Variant prose appears just inside payload
delimiters, or after a unit variant's name. Fields and variants are separated
by commas, with optional trailing commas.

Opaque types retain checked parameter arity without exposing representation:

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

Type expressions include local names, crate-qualified paths, generic
applications, and tuples. Unqualified names first resolve to a declaration's
parameter, then a type/class in the same module, then the built-in prelude:
`Vec<T>`, `Option<T>`, `bool`, `u32`, `u64`, and `String`. Cross-module references
must be qualified. Parentheses always denote tuples. The checker validates
nested argument arities, unknown types, duplicate parameters/fields/variants,
and the structural equality of override result types after name resolution.

Recursive semantic types require no `Box` or arena pointer. The model erases
`Stashed`, pointer/slice storage, and database lifetimes while retaining semantic
fields. `sage_ir/types.sage` describes `Ty`, aliases, constants, signatures,
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

`cargo run -p sage-meta -- browse` validates the model and exports a standalone
`target/sage-browser.html`. The exporter embeds serialized AST documents and the
browser assets; no server or frontend dependency installation is required.
The optional positional arguments select the model directory and output HTML:
`cargo run -p sage-meta -- browse sage-src /tmp/sage-architecture.html`.

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

A specification paragraph can start with a named marker, using the custom
MyST-style inline-role syntax:

```text
{anchor}`meta.calls.declared`
Every observed architectural call must be permitted by its caller's resolved
dependency envelope.
```

The marker and its prose form one paragraph: do not leave a blank line between
them. Names are dot-separated ASCII identifiers, describe independently testable
behavior, and stay stable when text moves. The book's `sage-meta` preprocessor
renders a shield-style badge with a `spec` label and the rule's name. Each badge
links directly to its paragraph; following a fragment link highlights the badge.
The HTML and stylesheet are local, so badges work offline without an image
service. The preprocessor does not implement the rest of MyST. Fenced examples, inline code,
and the existing ezanchor excerpt blocks do not declare specification anchors.

Tests identify their rules with inert attributes:

```rust,ignore
#[test]
#[sage::test(meta.calls.declared)]
#[sage::test(meta.calls.forbidden)]
fn reports_direct_and_ancestor_violation_with_complete_path_and_rule() {
    // Assert the observed edge and the violated ancestor's prohibition.
}
```

{anchor}`meta.spec.unique_names`
Every specification anchor must have a well-formed name that is unique across
the Markdown corpus. Malformed markers and duplicate names are errors.

{anchor}`meta.spec.test_coverage`
Every specification anchor must have at least one annotated Rust test. An
anchor without a test is an error.

{anchor}`meta.spec.test_references`
Every `#[sage::test(...)]` reference must resolve to an existing specification
anchor and be attached to a Rust test. Dangling and misplaced references are
errors.

One test can cover several rules and a rule can have several tests. Unlike
`sage::item`, a test may reference a rule across crate boundaries. Eligible
evidence is a module-level function with the built-in `#[test]` attribute in a
Cargo target with tests and the standard test harness enabled. Targets with `test = false` or `harness = false`
and unconditionally or conditionally ignored tests cannot satisfy a rule. The source inventory records cfg
conditions without evaluating them; an annotation proves neither that a test
ran in this build nor that its assertions establish the rule. Running and
reviewing those tests remain separate evidence.

The library entry point `sage_meta::check(workspace_root)` loads `sage-src`,
the Cargo-owned Rust source index, and the Markdown specifications under `md/`.
Its `CheckReport` separates model consistency, Rust correspondence, and
specification-test coverage. A missing `md/` corpus is an error, not an empty
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

The default check covers the repository model, Rust correspondences, and named
specification paragraphs and their tests. CI and the workspace integration test
use the same public library path. `check --correspondence` remains a compatible
spelling of the full check. `check path/to/sage-src` selects a model directory
but still requires its containing Cargo workspace and documentation. The legacy
positional invocation `sage-meta path/to/sage-src` validates an isolated model
without those requirements.

The corpus describes the core compiler demand graph and manifests for all
workspace crates. Supporting service operations are summarized, while derive
and harness internals have crate-level descriptions only. The conversion is
an architectural abstraction: for example `signature` maps to `sig`, external
subclasses model enum dispatch, and `BodyCheck` groups expression and pattern
checking implementations. It is not an exhaustive extraction of Rust calls.

`crates/sage-meta/tests/language.rs` supplies annotated tests for abstract-method
completeness, override compatibility, forbidden paths, virtual dispatch, and
inherited prohibitions. It also exercises AST parsing, source ranges,
manifest/module checks, and consistency of the repository corpus. The named
paragraphs on this page are the initial checked specification; older numbered
anchors elsewhere in the book have not been migrated automatically.
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
An optional model directory follows the port: `serve 3001 path/to/sage-src`.
The port must be between 1 and 65535. Both `/` and `/sage-browser.html`
serve the explorer; other paths return 404.
