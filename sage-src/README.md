# Reviewing Sage through `.sage`

Start with `sage_ir/crate.sage`, then `sage_ir/symbols.sage`. Each crate manifest
states its purpose, logical modules, and workspace dependencies. Crate names
use Rust identifiers (`sage_ir` for Cargo package `sage-ir`). A reference such
as `$sage_ir::symbols::FnSymbol::signature` starts in another crate;
`crate::symbols::FnSymbol::signature` starts in the current crate.

Run from the repository root:

```sh
cargo run -p sage-meta -- check --correspondence
cargo run -p sage-meta -- check --correspondence --json
cargo test -p sage-meta
```

Methods begin with triple-quoted intent and have structured envelopes:

```sage
query body() -> crate::types::Body {
    """Check this function in its defining environment and finish inference."""
    envelope {
        reads self.syntax;
        creates inference: crate::inference::InferCtx;
        invokes self.signature();
        invokes crate::symbols::FnSymbol::signature();
        invokes crate::inference::InferCtx::finalize();
        forbid transitive crate::symbols::LocalFnSym::body;
    }
}
```

`invokes` grants permission, not an unconditional execution or ordering.
`self.signature()` constrains the receiver to self. A type-qualified invocation
permits any receiver of that type, including its known subclasses. The checker
conservatively includes all known overriding implementations for both forms;
it does not track correlations between receiver instances. `forbid transitive`
rejects any nonempty path to the named method, including recursive reentry. A
base method's prohibition also applies to overriding implementations.

## Reading the conversion

This is an initial architectural conversion, not a line-for-line Rust mirror.
The compiler model is organized by semantic responsibilities, with source
pointers in comments. `signature` names Rust's `sig`; `BodyCheck`, `Solver`, and
`ExternalFacts` group Rust functions behind logical interfaces. External
subclasses describe enum dispatch through `SymExt`, not new Rust structs.
`types.sage` spells out the semantic fields and variants of the key Rust types,
including `Ty`, aliases, receivers, signatures, predicates, and trait items.
`Binder<T>` and identity concepts remain prose-defined opaque types. `Signature`
is an explicitly review-only sum for grouped operations; `Value` remains opaque.
The Fn/Trait/Impl class interfaces use optional results uniformly: their local
implementations lift Rust's non-optional results to `Some`. These descriptions
do not specify Rust typing or trait rules.

The model covers the central demand graph, major symbol families, and crate
boundaries. Manifests also inventory harness, derive, and maintenance crates;
those crates' internal algorithms are not yet converted. Some support services
are represented as prose leaves. A missing modeled edge is not evidence that
Rust lacks that dependency. Generated-model consistency does not establish
source conformance, exhaustive dependency coverage, or incremental reuse.

Exact per-kind identity fields, unavailable external operations, narrow metadata
keys, expression-specific dependencies, and signatures of context operations
need further refinement during review. The value descriptions preserve current
limitations: ADT arguments are types, and lifetime representation is `Dummy`. Source comments identify the relevant
implementation; the architecture book remains authoritative for destination
contracts and known implementation limitations.

## Rust correspondence

The Rust implementation names its architectural items with inert tool
attributes. Crate roots register the namespace on the pinned nightly:

```rust,ignore
#![feature(register_tool)]
#![register_tool(sage)]

#[sage::item(crate::types::Binder)]
pub struct Binder<T> { /* implementation */ }

#[sage::item(crate::checking::BodyCheck)]
mod r#abstract {}
```

`crate` refers to the containing Cargo package's model, not its Rust module.
The payload names a Sage item; Rust and Sage names need not match. Crate/module
headers, named declarations, and explicitly declared methods each require one
or more anchors. Fields and variants belong to their enclosing declaration.
Non-executable sites can carry several annotations. Functions implementing
operations have one identity each.

Module anchors represent abstract concepts and distributed implementations.
They provide source correspondence, not observed execution. The CLI reports
source-only anchors separately from executable declarations; neither count
implies runtime conformance. It rejects missing anchors, unknown targets,
wrong-crate annotations, incompatible kinds, and unsupported placements.
The source inventory includes cfg alternatives without evaluating them.

There are no `#[rust]` annotations in `.sage`: browser links and validation use
the Rust-side index as their common mapping authority. The known
`define_kind_symbols!` grammar has an explicit parser adapter; arbitrary tokens
inside other macros do not count as generated Rust declarations.

## Language boundary

The parser produces a typed AST with byte ranges. The checker validates manifests,
names, types, inheritance, overrides, field reads, context creation, and forbidden
transitive calls in the supplied closed world. A CLI failure prints the file,
byte range, and a concrete call path for a dependency violation.

The grammar supports single inheritance, one level of modules, structured and
opaque generic value types, identity/input/state fields, and argument-free method
references. Parentheses on methods denote the operation, not its Rust parameter
list. Method/class generics, method parameters, context initialization,
instance-qualified context calls, execution order, and computation expressions
are not yet supported. `creates` declares local ownership but does not prove
context state confinement.
No prose is executed or interpreted by the checker. Future semantic rules can
be linked to a-mir-formality; no such integration exists yet.

See `md/design/validation/architecture-language.md` for the full contract.

## Value types

Use a struct or enum when structure helps review; leave a concept opaque when
its meaning matters more than its fields:

```sage
type Binder<T> {
    """A value under the generic parameters binding its references."""
}

struct FnSignature {
    params: Vec<Ty>,
    result: Ty,
}

enum Ty {
    """A semantic type, independent of storage."""
    Never,
    Tuple("""Ordered component types.""" Vec<Ty>),
    Ref {
        """A reference to another type."""
        referent: Ty,
        mutable: bool,
    },
}
```

This small example illustrates syntax; `sage_ir/types.sage` is the source-based
model and retains more information. Structs have named fields; enum variants
can be unit, tuple, or named-field variants. Opening prose is optional on
structs/enums and variants, and required on opaque types. Unit-variant prose
follows its name; payload-variant prose goes immediately inside its delimiters.
Additional prose before a named field documents that field. Fields and variants
are comma-separated, with an optional trailing comma. The compact spelling
`type Name """meaning""";` remains supported.

Type expressions are local names, crate-qualified paths, generic applications,
or tuples. For example: `Ty`, `$sage_ir::types::Ty`, `Binder<Vec<Ty>>`, `(Ty, Ty)`,
and `()`. Parentheses always form a tuple, including a one-element tuple.
Unqualified names resolve to a declaration's type parameter, then a type/class
in the current module, then the built-in prelude: `Vec<T>`, `Option<T>`, `bool`,
`u32`, `u64`, and `String`. There are no implicit module imports. Generic structs,
enums, and opaque types declare their parameters as `<T, U>`; applications are
checked for existence and arity, including nested arguments and method results.
Overrides compare resolved type structure rather than spelling.

Recursive value declarations need no indirection. Storage wrappers, pointers,
and database lifetimes are absent from this language-level representation.

## Interactive explorer

Generate a standalone browser snapshot:

```sh
cargo run -p sage-meta -- browse
```

Open `target/sage-browser.html` in a web browser. Everything is embedded in that
file: no web server, JavaScript package installation, or network connection is
required. For a different model or output location:

```sh
cargo run -p sage-meta -- browse sage-src /tmp/sage-architecture.html
```

Use the sidebar to search names, method prose, or Rust paths, and filter by crate
or declaration kind. Press `/` to focus search and Escape to clear it. Crate and
module pages describe the logical structure. Declaration pages link field and
result types, enum variants, base classes, and methods. Method pages expose the
envelope, possible direct calls (including known subclass dispatch), reverse
callers, overrides, and inherited prohibitions. Hash links support browser
Back/Forward and bookmarks. Click a Rust correspondence to reveal the Rust source pane, or use the header’s
**Rust source** button. The pane follows the selected declaration; methods show
their own annotated implementation or marker sites. Definitions are highlighted with file names and line numbers.
Use **Hide** to close the pane. Theme switching is available in the header.

The explorer is a snapshot of the validated model. Regenerate it after edits;
it does not watch files, execute compiler queries, or establish Rust conformance.

Browser verification uses the repository model:

```sh
cargo test -p sage-meta
cargo run -p sage-meta -- browse
node crates/sage-meta/web/browser.test.cjs
```

Rust tests cover valid/invalid exports, CLI output, and script-safe data embedding.
Node tests cover model navigation, every rendered internal link, query dispatch,
reverse callers, search filters, and HTML escaping. These are renderer tests,
not screenshot or browser-engine layout tests.


### Embedded Rust source

When the model lives under a Cargo workspace, the exporter uses offline Cargo
metadata and the Rust correspondence index. It embeds annotated files and
provides a selector for multiple implementation sites. Module markers and known
generator inputs are labeled. Crate/module headers and individual methods have
source links as well as type/class declarations.

The source snapshot does not evaluate cfg or expand arbitrary macros. Regenerate
the HTML after Rust edits. No source server is required. Deleting or misspelling
an anchor fails `check --correspondence`; checking a model alone remains
available with `check` or the legacy positional model-directory command.

### Contract infrastructure tests

`cargo test -p sage-meta` includes a small independent model and Rust/Salsa test
crate under `tests/fixtures/contracts/`. Expected traversals name architectural
operations and Rust sites independently of the source index. A valid test model
is also exercised by deliberately nonconforming Rust, including a forbidden
request already present in the query cache. The same checking engine records
both the undeclared edge and the ancestor prohibition.

This is infrastructure evidence. General compiler runtime enforcement,
ordinary-function instrumentation, async context propagation, and browser trace
replay remain work in progress. Inert annotations alone do not emit events.

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
