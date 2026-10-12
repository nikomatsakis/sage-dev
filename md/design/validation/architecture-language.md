# Architectural Pseudocode

The `.sage` language exposes the compiler as symbols, contexts, and methods.
Each method states its intent in triple-quoted prose and its allowed dependencies
in a structured envelope. `sage-meta` parses that description into an AST and
checks the declared architecture independently of executing Rust semantics.

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
methods; concrete subclasses must implement them. Overrides preserve the
operation kind and result type. Contexts hold computation-local working state
and may inherit only from contexts. Fields distinguish identity, changing input,
and context state. Queries identify memoized semantic products; `fn` identifies
ordinary computation. These annotations document boundaries; the model itself
has no evaluator or cache.

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
Prohibitions are inherited by overriding implementations.

<a id="meta-a1"></a>
> **META-A1 — Dependency envelopes are checked conservatively.** The checker
> resolves every declared dependency and expands virtual dispatch to all known
> implementations in the supplied model. No permitted nonempty path may reach
> a transitively forbidden operation. Rejected paths include the offending
> method sequence so a reviewer can inspect the contradiction.
>
> **Required verification:** Tests cover direct, indirect, recursive, inherited,
> cross-crate, and overriding method calls, unknown references, and a base
> prohibition violated by an override.

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
validation. A session checks explicit operation entries against direct call
permissions and all active ancestor prohibitions. Entry into the same operation
again counts as recursion. Receiver-sensitive permissions require semantic
receiver identity; missing identity remains an observation gap. Query-engine
internal reexecution installs context without fabricating a source call.

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

## Verification authority

Model consistency does not prove Rust conformance. In particular, an omitted
edge can conceal a dependency that exists in Rust. Source review and the
[query-boundary evidence](../infrastructure/incrementality.md) are needed to
establish correspondence. Type and trait algorithms remain prose boundaries;
a formal semantics integration is prospective.

## Current status

`sage-meta` provides a lexer, parser, public AST, validation API,
and command-line validator/exporter. Diagnostics identify source byte ranges. Run:

```sh
cargo run -p sage-meta -- check --correspondence
cargo run -p sage-meta -- check --correspondence --json
cargo test -p sage-meta
```

The corpus describes the core compiler demand graph and manifests for all
workspace crates. Supporting service operations are summarized, while derive
and harness internals have crate-level descriptions only. The conversion is
an architectural abstraction: for example `signature` maps to `sig`, external
subclasses model enum dispatch, and `BodyCheck` groups expression and pattern
checking implementations. It is not an exhaustive extraction of Rust calls.

`crates/sage-meta/tests/language.rs` exercises META-A1, AST parsing, source ranges,
manifest/module checks and consistency of the repository corpus.
`tests/correspondence.rs` checks missing/dangling/wrong-crate anchors, kind
compatibility, cfg alternatives, multiple Cargo targets, explicit module paths,
generator inputs, ambiguous/unmapped sites, nested functions, unsupported
placements, and source snapshot mismatches. `tests/cli.rs` verifies separate
model/correspondence reports and failure exit status. Initial
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

`tests/runtime_contracts.rs` exercises the shared checking engine with explicit
roots, recursion, interface dispatch, inherited prohibitions, receiver checks,
unbalanced events, and visible unchecked effects. Violations retain both the
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
