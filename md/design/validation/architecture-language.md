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
The Rust implementation is linked from comments in the model. Third-party
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

## Rust correspondence annotations

**Planned replacement:** The draft [Sage Contracts and Walkthroughs
RFD](../../rfds/sage-contracts/README.md) replaces the annotations below with
inert Rust-side `#[sage::item(...)]` tool attributes, including module anchors
for abstract concepts. Runtime source indexing matches Salsa query provenance
to model items. It stages bidirectional correspondence checking, runtime
enforcement against a cached model, and shared trace-based walkthroughs. The
following describes the implementation available today; none of that runtime
instrumentation is implemented yet.

Each top-level declaration in the repository model carries `#[rust(...)]`:

```sage
#[rust(crate::ty::Binder)]
type Binder<T> {
    """A value under the generic parameters binding its references."""
}
```

`crate` refers to the containing Rust crate. Paths are Rust source names, not
`.sage` namespace references: an external path uses `std::vec::Vec`, not a
`$` prefix. The annotation supports named type constructors, applications,
primitives, and tuples, and can name a trait for an interface. Rust lifetimes
and generic arguments may be omitted when naming a constructor.

A review-only grouping without a corresponding Rust type uses
`#[rust(none, """explanation""")]`. This prevents conceptual groupings such as
`Signature` and `BodyCheck` from implying nonexistent Rust declarations.
The AST distinguishes absent annotations from explicit unmapped concepts and
retains the annotation's source range. Unknown annotation names, duplicate
annotations, and empty unmapped explanations are parse errors.

Annotations are optional in standalone examples but required on every type,
class, and context by the repository corpus test. They do not create dependency
edges or resolve in the `.sage` type namespace. The checker does not verify
Rust source existence, visibility, or generic arity. Individual fields, variants,
methods, and crate/module headers do not carry separate origin annotations.

## Interactive architecture explorer

`cargo run -p sage-meta -- browse` validates the model and exports a standalone
`target/sage-browser.html`. The exporter embeds serialized AST documents and the
browser assets; no server or frontend dependency installation is required.
The optional positional arguments select the model directory and output HTML:
`cargo run -p sage-meta -- browse sage-src /tmp/sage-architecture.html`.

The explorer provides crate/module navigation, search and kind filters,
linked value types and variants, Rust correspondence annotations, base classes,
methods, and dependency envelopes. Method pages show conservative direct-call
targets, reverse callers, overriding implementations, and inherited prohibitions.
Hash routes support bookmarks and browser history. Press `/` to search. Click a Rust annotation or the **Rust source** header button to reveal a source
pane with the definition highlighted. The pane follows navigation; methods show
their owning type. It can be hidden without leaving the architecture view.

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
cargo run -p sage-meta -- sage-src
cargo test -p sage-meta
```

The corpus describes the core compiler demand graph and manifests for all
workspace crates. Supporting service operations are summarized, while derive
and harness internals have crate-level descriptions only. The conversion is
an architectural abstraction: for example `signature` maps to `sig`, external
subclasses model enum dispatch, and `BodyCheck` groups expression and pattern
checking implementations. It is not an exhaustive extraction of Rust calls.

`crates/sage-meta/tests/language.rs` exercises META-A1, AST parsing, source ranges,
manifest/module checks, Rust-origin parsing and corpus coverage, and consistency
of the repository corpus. Initial
syntax supports one module level, structs and enums, opaque generic types,
nested type applications, and argument-free method references. Value-type tests
cover prose preservation, recursive structures, generic scope and arity,
cross-crate references, and structural override checking. Method parameters,
method/class generics, context initialization and binding flow, computed
expressions, and source-to-model conformance checking remain unimplemented. Context ownership is described, not escape-checked. Virtual
calls deliberately overapproximate possible receiver implementations.

See the [review-language roadmap slice](../../implementation/roadmap.md#architectural-review-language)
for coverage work beyond this initial conversion.

The Rust browser integration tests exercise model rejection, script-safe
embedding, and repository export. After generating the snapshot,
`node crates/sage-meta/web/browser.test.cjs` checks search, navigation, every
rendered model link, conservative dispatch, and HTML escaping. These checks do
not substitute for visual layout testing in a browser engine.


### Embedded Rust source

When the model lives under a Cargo workspace, the exporter uses offline Cargo
metadata and parses workspace Rust modules with `syn`. It follows explicit
imports and re-exports, including module aliases, and embeds only files reached
by annotations. Generic arguments can have separate source definitions; a
selector exposes these and multiple conditional definitions. Primitive and
standard-library types have no embedded workspace source. Review abstractions
show their annotation's explanation.

This is best-effort navigation, separate from model validation. It does not
evaluate cfg, expand macros, or resolve wildcard imports. Recognizable type
declarations in macro inputs are shown with an explicit label. Source is a
snapshot: regenerate the HTML after Rust edits. No source server is required.

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
