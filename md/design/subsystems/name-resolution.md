# Name Resolution

Name resolution maps Rust paths and names in a scope and namespace to semantic
[symbols](../infrastructure/symbols.md). It is a subsystem rather than a
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
the keyed [external metadata](./external-metadata.md) boundary.

Resolution produces identities, not checked types. Operations such as
`T::Item`, receiver autoderef, associated-type normalization, and method
candidate applicability require cooperation with inference or the trait
solver and do not belong to path lookup alone.

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

## Code map

| Path | Responsibility |
|---|---|
| `check/resolve/mod.rs` | resolver context, namespaces, module/import lookup, cycle detection, method-scope traits |
| `check/resolve/ribs.rs` | lexical scopes and non-module resolutions |
| `local_syms/mods.rs` | direct expanded local module members and completeness audits |
| `local_syms/uses.rs` | parsed import edges |
| `symbol/mod.rs` | module and definition identity wrappers |
| `tcx/mod.rs` | external module-child and identity metadata |

## Current status

### Current frontier

Normal type/value lookup, lexical generic/local lookup, local and external
module paths, named imports, transitive globs with cycle termination, edition
prelude traits, and the method-scope subset needed by the completed mini-redis
slices are operational.

### Implemented capabilities and evidence

- `transitive_local_globs_resolve_and_glob_cycles_terminate` verifies both a
  transitive reexport and termination of a glob cycle.
- `a_trait_from_another_editions_prelude_is_not_a_provider` verifies that
  prelude lookup uses the current crate edition.
- `an_explicitly_imported_renamed_external_trait_is_a_provider` verifies a
  renamed external trait import in method lookup.
- `unresolved_item_macro_keeps_method_scope_incomplete` verifies that partial
  expansion cannot justify exhaustive provider selection.

### Current limitations

- Qualified lookup through a type, such as general `T::Item`, is not integrated
  into `resolve_path`; it requires type/trait-directed associated lookup.
- Local glob-provider completeness is conservative because all recursive
  export edges are not yet represented as an exhaustive indexed result.
- General visibility, hygiene, active attributes, macro rules, and Rust's full
  namespace edge cases are incomplete.
- Resolution results and completeness do not yet have a standalone readable
  Semantic Inspector view.

### Related roadmap slices

[Semantic inspector and persistent edit
testing](../../implementation/roadmap.md#planned-slice-semantic-inspector-and-persistent-edit-testing)
will expose path results and their query trail. [Shutdown::recv](../../implementation/roadmap.md#next-application-slice-shutdownrecv)
is the next application slice likely to extend method and associated lookup.
