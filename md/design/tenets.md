# Tenets

Design principles governing sage's architecture. These guide all new
code across every module.

## Code organization

**Methods live on the types they process.** Type-checking, name
resolution, and lowering are inherent methods on CST and symbol types.
Not standalone functions, not visitor traits.

```rust
TypeCst::check(cx)
PathCst::resolve(cx, ns)
ExprCst::check(bx)
LocalFnSym::sig(db)
LocalStructSym::fields(db)
```

**Master modules with submodules.** Each concern gets a top-level
module subdivided by item kind or sub-concern. Avoid monolithic files.
One struct or closely-related cluster per file.

- `cst/` — per-item CST data: `fns.rs`, `structs.rs`, `ty.rs`,
  `paths.rs`, `generics.rs`, `expr.rs`
- `local_syms/` — per-kind symbol definitions: `fns.rs`, `structs.rs`,
  `enums.rs`, `mods.rs`
- `check/infer/` — inference sub-concerns: `egraph.rs`, `skeleton.rs`,
  `version.rs`, `runtime.rs`, `bound.rs`

**Shared infra lives in pass-named modules.** Contexts, helpers, and
traits shared across item kinds live in a module named after the pass:

- `check/sig.rs` — `Check` (signature-lowering context)
- `check/infer_ctx.rs` — `InferCtx` and `Scope` (body-checking context)
- `check/resolve/` — `Resolver`, `Namespace`, and lexical ribs

Item-specific logic imports from these; it does not redefine its own
plumbing.

## Query design

**Single-keyed queries.** `sym.sig(db)`, `sym.body(db)` — the symbol
knows its scope, the query derives everything else. No ambient
parameters threaded from callers.

```rust
#[salsa::tracked]
pub fn sig(self, db: &'db dyn crate::Db) -> Stashed<Binder<'db, FnSig<'db>>> { ... }
```

**`sig` is the cross-item boundary.** A signature query extracts
exactly what other items need to type-check against this one: generics,
parameter types, return type, field types. It is the minimal public
surface.

**Detail queries are lazy.** `body()`, `fields()`, and similar queries
compute information that is either not needed from other items or only
needed some of the time. They depend on `sig()` but are not depended on
by other items' signatures.

**Generic parameters are minted exactly once.** The `sig()` query mints
`GenericParam` symbols via `cst.generics.check(db, cx, parent)` and
stores them in a `Binder`. All other queries (`fields`, `body`) open
that binder and bring the same param symbols into scope via
`ribs.add_generic_params`. No re-minting, no identity confusion.

**Sequential layering inside a query.** `body()` calls `sig()` -> opens
binder -> resolves names -> runs inference. Each step builds on the
prior. From outside: one query, one result. Intermediates are not
separately queryable.

## Data flow: the two-stash pattern

**CST is read-only input; output goes to a fresh stash.** The context
bridges `src: &Stash` (the CST) and `target_stash: Stash` (output types
or resolved exprs). At the end, `cx.finish(root)` wraps `target_stash`
into `Stashed<T>`.

```rust
pub struct Check<'a, 'db> {
    pub resolver: Resolver<'db>,
    pub src: &'a Stash,
    pub target_stash: Stash,
}
```

**Purpose-specific contexts.** `Check` lowers signatures into its target
stash. `InferCtx` owns a body target stash plus inference, obligations,
diagnostics, and cooperative tasks. Both keep source CST storage read-only and
return self-contained semantic output.

**`Stashed<T>` is the memoization boundary.** Salsa compares
fingerprints (content hashes of the output stash) for change detection.
Deterministic allocation means same CST + same scope = same fingerprint
= no downstream re-execution.

## Resolution model

**Ribs first, module-scope fallback.** Ribs capture lexically-scoped
bindings (generics, locals, `Self`). If the first path segment is not
found in ribs, fall through to `resolver.resolve_segments()` for
module-level names.

```rust
if let Some(entry) = cx.resolver.ribs.lookup(first.name, ns) {
    // found in rib — generic param, local, or Self
} else {
    // fall through to module-level resolution
    cx.resolver.resolve_path(stash, path, ns)
}
```

Module-level resolution walks direct expanded module symbols for local modules
and queries `TcxDb::module_children` for external crates.

## Incrementality

**Per-item CST stashes isolate parsing from checking.** Each item's CST
is stored as `Stashed<Ptr<FnCstData>>`. The CST uses `RelativeSpan`s so
that whitespace edits before an item do not change its content hash.

**Salsa tracked structs are per-item.** `LocalFnSym`, `LocalStructSym`
— one tracked struct per top-level item. Tracked fields store the CST,
the absolute span, and the computed sig/body.

**Body changes do not propagate through unchanged signatures.** `sig()`
semantically uses only generics, parameters, return type, and predicates. A
coarse CST or module dependency may currently cause it to reexecute, but an
equal checked signature is backdated before downstream interface consumers.

## Oracle conformance

**Emit a common form, then compare exact text.** The rustc and Sage emitters
independently translate their native IR into the shared reference schema and
serialize it deterministically. The conformance decision is byte-for-byte
identity of those serialized outputs.

Adapters perform only the representation changes required by the shared
schema. The comparison path never applies paired normalization, erases a
difference, strips unsupported content, reorders output, or attempts semantic
equivalence. Rich diffs may explain a mismatch after exact comparison fails;
they cannot turn it into a pass. See [Oracle Test
Harness](./oracle-test-harness.md#thin-adapters-and-exact-comparison).

## Naming conventions

- `*Cst` — CST nodes (stash-allocated, per-item): `TypeCst`, `ExprCst`,
  `PathCst`, `FnCstData`, `StructCstData`
- `*Sym` — symbols (salsa-tracked or enum wrappers): `LocalFnSym`,
  `Symbol`, `FnSymbol`
- `Ty*` — typed-tree nodes: `TyExpr`, `TyStmt`, `TyPat`, `TyBody`
- `*Sig` — signature payloads: `FnSig`, `StructSig`, `StructFields`
