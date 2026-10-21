# Name Resolution

Name resolution maps Rust paths and names in a scope and namespace to semantic
[symbols](symbols-guide.md). It is a subsystem rather than a
pipeline phase: macro expansion, signature checking, and body checking invoke
it with different lexical environments and completeness needs.

## Contract

A resolution request supplies a scope, path or name, and `Namespace`. The
namespaces are type, value, and macro; the macro namespace is subdivided into
bang, attribute, and derive forms. The result retains zero, one, or several
`Resolution` values so absence and ambiguity are not silently collapsed.

Lexical ribs take priority for unqualified names. They contain generic
parameters, `Self`, function parameters, locals, and other body-local
bindings. Module lookup then considers direct expanded symbols, explicit and
glob imports, module ancestry/path anchors, preludes, external module children,
and namespace rules.

The subsystem distinguishes a represented subset from an exhaustive result.
A caller that needs negative reasoning—such as determining that no unseen
method provider exists—must receive or compute a completeness fact. An
unresolved import, unsupported reexport, or incomplete macro expansion is not
proof that no matching symbol exists.

{spec}`crates.sage_ir.resolution.lookup.completeness`

Verification intent: Focused fixtures distinguish absent, uniquely
resolved, ambiguous, and incomplete lookup, including an incomplete empty
provider set, and demonstrate that only the exhaustive empty case permits a
definitive negative conclusion.

{spec}`crates.sage_ir.resolution.lookup.precedence`

Verification intent: Fixtures cover lexical shadowing, independent
type/value/macro namespaces, qualified versus unqualified lookup, renamed
imports, glob cycles, and edition-specific preludes.
Source-driven fixtures use `//# sym` to distinguish equal-typed definitions,
including lexical bindings versus module items, explicit imports versus globs,
and `self`, `super`, and `crate` paths. Combined `//# ty` observations expose
type-namespace selection; item-macro expansion exercises the independent macro
namespace. Edition fixtures pin their edition and assert both availability and
absence of newly introduced prelude names.

Macro-generated modules require a separate import-path case: if a glob brings
`path` into scope and expansion introduces a competing module named `path`,
`use path::*` must diagnose the expansion-sensitive ambiguity. The controls are
an explicitly qualified `use crate::path::*` and a competing module written
directly in source; both select the direct module. This distinction applies
even when the glob's imported contents are not subsequently used. It does not
justify replacing macro-phase combined lookup with normal named-before-glob
lookup.

{spec}`crates.sage_ir.resolution.lookup.expansion_import_ambiguity`

The final import audit compares generated named bindings with glob candidates
in the applicable namespace. A named import's origin is the `use` declaration,
not the origin of its target definition. Thus a source-written alias to a
generated module still has ordinary named-import precedence. Glob roots use
the type namespace, and routes to the same definition remain unambiguous.

## Entry points and callers

`Resolver::resolve_path` is the common path boundary. Its fast path checks
ribs for an unqualified one-segment path, then falls through to module lookup:

```{anchor}
architecture_resolve_path
```

`Resolver::new` creates a normal signature/body context;
`new_for_macro_expansion` creates a macro-phase context whose same-module
lookup can participate in the expansion fixed point. `resolve_name_from_scope`
serves single-name lookup. Method lookup uses a specialized provider query:

```{anchor}
example_traits_in_method_scope
```

## Construction

Module lookup searches the appropriate namespace in direct expanded items and
follows `use` edges. An `in_flight` stack of `(module, name, namespace)` tuples
cuts cycles through imports and globs. External module membership comes from
the keyed [external metadata](metadata-guide.md) boundary.

Resolution produces identities, not checked types. Operations such as
`T::Item`, receiver autoderef, associated-type normalization, and method
candidate applicability require cooperation with inference or the trait
solver and do not belong to path lookup alone.

{spec}`crates.sage_ir.resolution.lookup.identity_only`

Verification intent: A normalized query trace for representative path
lookups contains only the searched scopes, import edges, and symbol/metadata
identity operations, with no inference, trait-solver, candidate-signature, or
checked-body execution.

## Incremental boundary

`Resolver` is short-lived state inside a signature, expansion, or body query;
it is not itself one monolithic Salsa query. Its reads become dependencies of
the owning query: expanded items of the modules actually searched, import
syntax traversed, relevant prelude/module metadata, and the lexical bindings
constructed by the caller.

Resolution must not load checked bodies. Name-based method discovery should
enumerate identities before loading candidate signatures, so an unrelated
associated item does not become a body dependency merely because it shares an
owner.

{spec}`crates.sage_ir.resolution.lookup.dependencies`

Verification intent: Cold and warm query traces show the exact scopes
and external keys read for a lookup, exclude unsearched modules, unrelated
associated-item signatures, and all checked bodies, and show reuse when those
unrelated inputs change.

## Code map

| Path | Responsibility |
|---|---|
| `check/resolve/mod.rs` | resolver context, namespaces, module/import lookup, cycle detection, method-scope traits |
| `check/resolve/ribs.rs` | lexical scopes and non-module resolutions |
| `local_syms/mods.rs` | direct expanded local module members and completeness audits |
| `local_syms/uses.rs` | parsed import edges |
| `symbol/mod.rs` | module and definition identity wrappers |
| `tcx/mod.rs` | external module-child and identity metadata |

## Implementation and test evidence

The generated [anchor pages](../../anchor/crates/sage_ir/resolution.html) report implementation entries,
annotated tests, explicit TODO/N/A gaps, and saved claim/test reviews. These
associations do not themselves establish that a test ran.

The [earlier coverage snapshot](../../../rfds/implementation-sequencing/coverage-history.md#name-resolution)
preserves the former capability descriptions, edit experiments, and limitations
as historical evidence. Current coverage comes from the generated entries;
implementation plans and decision rationale belong in the relevant RFDs.
