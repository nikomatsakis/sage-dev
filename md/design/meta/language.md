# Architectural Pseudocode

The `.sage` language exposes the compiler as symbols, contexts, and methods.
Each method states its intent in triple-quoted prose and its allowed dependencies
in a structured envelope. `sage-meta` is the shared build and validation layer
for correspondence among Markdown specifications, `.sage` models, and Rust.
Its library checks declared structure at build time and observed calls during
runtime sessions; the CLI, Cargo tests, CI, and browser use those same rules.
It does not execute Rust semantic specifications.

This review layer follows [Architectural pseudocode has checkable dependency envelopes](../../rfds/architectural-foundations/README.md#architectural-pseudocode-has-checkable-dependency-envelopes).
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

{anchor}`classes.single_inheritance`
A class or context declaration has at most one `extends` base.

<!-- end anchor -->

An abstract class can declare abstract methods.

{anchor}`classes.abstract_complete`
A concrete class must implement every inherited abstract method.

{anchor}`classes.override_signature`
An overriding method must preserve the inherited method's operation kind and
resolved result type.

{anchor}`contexts.inheritance`
Contexts are a distinct declaration kind and may inherit only from contexts.
An ordinary or abstract class cannot inherit from a context.

<!-- end anchor -->

Contexts describe computation-local working state. Their intended ownership
does not by itself establish runtime allocation or escape checking.

{anchor}`fields.kinds`
Fields retain their declared `identity`, `input`, or `state` kind, distinguishing
identity from changing inputs and local working state.

{anchor}`fields.context_state`
A `state` field is valid only in a context declaration.

{anchor}`methods.kinds`
Methods retain their declared operation kind: `query` or `fn`. Resolving a
method's contract preserves that distinction.

Queries describe memoized semantic products; functions describe ordinary
computation. These annotations document boundaries; the model has no evaluator
or cache.

{anchor}`methods.prose`
Every method body begins with a triple-quoted description, followed by its
`envelope` or `abstract;` declaration. The description is retained as prose;
method-like text within it adds no dependency permissions.

### Dependency envelopes

{anchor}`envelopes.reads`
`reads self.field;` permits reading a field declared on this receiver's class
or inherited from a base. Unknown fields and non-`self` field paths are errors.

{anchor}`envelopes.creates`
`creates name: crate::module::Context;` declares a fresh owned working context
under the given binding name. Its target must resolve to a context declaration.
This describes context creation; validation does not perform the allocation.

{anchor}`envelopes.receiver_calls`
`invokes self.method();` permits a call on the current semantic receiver.
A matching method on a different receiver does not satisfy that permission.

{anchor}`envelopes.class_calls`
`invokes crate::module::Class::method();` permits a call on any instance of the
named class or a known subclass. This permission does not require equality
with the caller's receiver.

{anchor}`envelopes.leaf`
An empty envelope describes a leaf operation with no modeled dependencies.
It is distinct from an abstract method, which has no declared envelope.

{anchor}`envelopes.permissions`
Declared calls are permissions: none must occur, and calls may occur in an
order different from the order of their envelope clauses.

{anchor}`envelopes.direct_calls`
Each invocation clause grants a direct dependency. Transitive reachability is
derived from those direct edges and does not grant the caller permission to
invoke an indirect dependency directly.

{anchor}`model.forbidden_paths`
`forbid transitive crate::module::Class::method;` prohibits reaching its target
over any permitted nonempty call path. A rejected path identifies the offending
method sequence.

{anchor}`model.virtual_dispatch`
The checker resolves declared calls conservatively, including every known
overriding implementation in the supplied model.

{anchor}`model.inherited_prohibitions`
An overriding implementation inherits the prohibitions of the base method.

Prohibitions record design intent even when the permitted graph already
excludes the target. Adding a permitted edge must still fail validation if it
creates a forbidden path.

## Semantic value types

Value declarations complement identity-bearing classes. They describe semantic
contents without prescribing Rust storage layout.

### Declarations and prose

{anchor}`types.struct_fields`
A `struct` contains named, typed fields. The parsed declaration retains their
names, types, and source order; an empty struct is permitted.

{anchor}`types.enum_variants`
An `enum` contains named variants in source order. A variant can have no
payload, an ordered tuple payload, or named, typed fields. These three shapes
remain distinct, including when a tuple or named payload is empty.

{anchor}`types.generic_parameters`
Structs, enums, and opaque types can declare a nonempty comma-separated list of
type parameters in angle brackets. Parameters are available throughout their
own declaration and do not become names in other declarations.

{anchor}`types.opaque_representation`
An opaque `type` declares a name and any type parameters without fields or
variants. It requires triple-quoted descriptive prose, either inside braces
or before a terminating semicolon. A payload inside its body is an error.

{anchor}`types.declaration_prose`
A struct or enum can carry triple-quoted prose immediately inside its opening
brace. The parsed declaration retains that text; omitting it is permitted.

{anchor}`types.field_prose`
A named field can carry triple-quoted prose immediately before its name.
The parsed field retains that text independently of the containing declaration
or variant's prose. At the start of a body, the first prose string describes
the container; a description for its first field follows as a second string.

{anchor}`types.variant_prose`
A variant can carry triple-quoted prose immediately inside its payload's
opening delimiter, or immediately after a unit variant's name. The parsed
variant retains that text independently of other variants.

{anchor}`types.member_commas`
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

{anchor}`types.type_expressions`
Type expressions are named types with optional generic arguments, or tuples.
Generic arguments and tuple elements are themselves type expressions, allowing
arbitrary nesting. The parsed expression retains that nesting.

{anchor}`types.name_resolution`
An unqualified type name resolves first to a parameter of its declaration,
then to a type, class, or context in the same module, then to the built-in
prelude. An earlier match shadows a later one.

{anchor}`types.prelude`
The built-in prelude contains `Vec<T>`, `Option<T>`, `bool`, `u32`, `u64`, and
`String`. These names are available without declaring them in the model.

{anchor}`types.qualified_names`
A type outside the current module requires a `crate::` or `$cratename::`
qualified path. Cross-crate type references require a declared dependency;
unqualified lookup does not search other modules or the crate root.

{anchor}`types.tuples`
Parentheses always denote a tuple, including `()` and a single element with or
without a trailing comma. `(T)` is distinct from `T` when result types are
compared for an override.

{anchor}`types.argument_arity`
Each named type application must supply exactly the declared number of type
arguments. This applies to opaque types, built-in types, parameters, and nested
applications; a type parameter takes zero arguments.

{anchor}`types.unknown_names`
Every type name in a value field, variant payload, class or context field, or
method result must resolve. An unknown name is an error even when nested in
generic arguments or tuples.

{anchor}`types.unique_parameters`
A value declaration cannot repeat a type parameter's name.

{anchor}`types.unique_fields`
Named fields have distinct names within a struct or a single enum variant.
Different declarations and different variants may reuse field names.

{anchor}`types.unique_variants`
Variants have distinct names within an enum. Different enums may reuse a
variant name.

{anchor}`types.recursive_values`
Semantic value types can refer directly to themselves or to each other, without
`Box` or another storage indirection. Such recursion is not a validation error.

<!-- end anchor -->

Override result types follow the
[resolved signature rule](#meta.language.classes.override_signature).

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

{anchor}`calls.declared`
Every observed architectural call must be permitted by its caller's resolved
dependency envelope. An undeclared call is an error, including recursive entry
into the same operation and requests satisfied from a query cache.

{anchor}`calls.forbidden`
An observed operation must not violate a prohibition of any active ancestor,
even when its immediate caller permits the call. A violation is an error and
retains the prohibition and complete call path.

<!-- end anchor -->

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

The [browser specification](book.md#displaying-sage-files) defines
declaration rendering, search, dependencies, Rust source display, entity links,
and delivery. The following commands describe the current implementation.

`cargo run -p sage-meta -- browse` validates the model and exports a standalone
`target/sage-browser.html`. The exporter embeds serialized AST documents and the
browser assets; no server or frontend dependency installation is required.
The [Crates](../crates/README.md) section embeds the same explorer in the built book,
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

## Verification authority

Model consistency, source correspondence, specification-test coverage, and
observed runtime conformance answer different questions. An omitted `.sage`
edge can leave the static model consistent; the runtime checker reports an
error if that edge is observed. Full implementation conformance additionally
requires every relevant boundary to be observed. A source annotation alone
cannot establish that coverage. See the generated [contract evidence](../anchor/meta/contracts.html)
and the [query-boundary evidence](../crates/sage_ir/incrementality-guide.md).
Type and trait algorithms remain prose boundaries; a formal semantics
integration is prospective.

## Implementation and test evidence

The generated [anchor pages](../anchor/meta/language.html) report implementation entries,
annotated tests, explicit TODO/N/A gaps, and saved claim/test reviews. These
associations do not themselves establish that a test ran.

The [earlier coverage snapshot](../../rfds/implementation-sequencing/coverage-history.md#architecture-language)
preserves the former capability descriptions, edit experiments, and limitations
as historical evidence. Current coverage comes from the generated entries;
implementation plans and decision rationale belong in the relevant RFDs.
