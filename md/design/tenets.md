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

**Semantic interfaces are narrow and lazy.** `sig()` is the primary
cross-item boundary: it exposes the owner binder, parameters, return type, and
predicates needed to use a definition. Language-required field, member, and
associated-value interfaces may be separate keyed queries so a consumer does
not load facts it did not request. Bodies and checking temporaries are not
interfaces.

<a id="ten-a1"></a>
> **TEN-A1 — Interfaces are the cross-item semantic boundary.** A symbol's
> checked signature is its primary interface; field, member, and other
> language-required interfaces may be separate keyed products. Bodies and
> checking temporaries are not interfaces. A caller may depend on a callee's
> interface, but never on its body. See
> [D15](./decisions.md#d15-cross-item-dependencies-stop-at-semantic-interfaces).
>
> **Required verification:** Query traces for signatures and callers require
> only the relevant interface products and forbid callee-body and
> unrelated-detail reads; interface-preserving edits do not propagate past the
> corresponding interface boundary.

**Generic parameters are minted exactly once.** The `sig()` query mints
`GenericParam` symbols via `cst.generics.check(db, cx, parent)` and
stores them in a `Binder`. All other queries (`fields`, `body`) open
that binder and bring the same param symbols into scope via
`ribs.add_generic_params`. No re-minting, no identity confusion.

<a id="ten-a2"></a>
> **TEN-A2 — A declaration has one set of generic-parameter identities.** The
> signature boundary creates the symbols once; every detail query opens the
> binder and reuses those identities rather than recreating equivalent-looking
> parameters. See
> [D5](./decisions.md#d5-symbols-form-the-uniform-semantic-identity-family).
>
> **Required verification:** Identity tests compare generic parameters reached
> through signatures, fields, predicates, and bodies, including after a warm
> query and an unrelated edit.

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
Equal rooted semantic output has the same fingerprint regardless of allocation
history, allowing Salsa to stop downstream propagation after recomputation.

<a id="ten-a3"></a>
> **TEN-A3 — Semantic outputs own their storage and have deterministic
> identity.** Checking reads source storage without mutating it, builds the
> result in a fresh stash, and returns a self-contained `Stashed<T>` whose
> fingerprint reflects semantic content rather than process allocation. See
> [D2](./decisions.md#d2-stash-for-type-level-interning).
>
> **Required verification:** Storage-isolation tests forbid source mutation or
> cross-stash pointers, deterministic rebuild tests compare fingerprints, and
> edit experiments show equal outputs are backdated.

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

<a id="ten-a4"></a>
> **TEN-A4 — Lexical scope has priority over module fallback.** Resolution
> consults namespace-aware lexical ribs before traversing local or external
> module membership, so a nearer binding shadows a module-level name without
> changing the module index.
>
> **Required verification:** Resolution tests cover shadowing, namespaces,
> `Self`, generics, and locals, plus local and external fallback when no rib
> entry applies.

## Incrementality

**Per-item CST stashes isolate parsing from checking.** Each item's CST
is stored as `Stashed<Ptr<FnCstData>>`. The CST uses `RelativeSpan`s so
that whitespace edits before an item do not change its content hash.

**Salsa tracked structs are per-item.** `LocalFnSym`, `LocalStructSym`
— one tracked struct per top-level item. Tracked fields store the CST,
absolute span, and other independently changing definition facts. Signature,
field/member, and body products are tracked functions keyed by the symbol; they
are not identity fields stored eagerly in it.

**Body changes do not propagate through unchanged signatures.** `sig()`
semantically uses only generics, parameters, return type, and predicates. A
coarse CST or module dependency may currently cause it to reexecute, but an
equal checked signature is backdated before downstream interface consumers.

<a id="ten-a5"></a>
> **TEN-A5 — Incremental reuse is judged at semantic boundaries.** Coarse
> upstream invalidation may reexecute a query, but an equal symbol, signature,
> or other public semantic product is backdated before unrelated downstream
> consumers are invalidated. See
> [D1](./decisions.md#d1-salsa-for-incremental-computation).
>
> **Required verification:** Persistent-database edit tests distinguish
> execution from downstream propagation for relevant, interface-preserving,
> and unrelated edits.

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

<a id="ten-a6"></a>
> **TEN-A6 — Conformance is exact shared-form identity.** Sage and rustc each
> adapt independently to the shared oracle schema; the comparator performs no
> semantic reconciliation, filtering, or paired normalization. See
> [D4](./decisions.md#d4-oracle-test-harness).
>
> **Required verification:** Harness tests demonstrate exact success, expose a
> one-field mismatch, and fail if either adapter or comparator attempts to
> erase, reorder, or normalize a semantic difference.

## Validation evidence

**Semantic evidence begins with Rust source.** When a behavior can be expressed
by a Rust program, integration and acceptance tests start from a checked-in
Cargo project and exercise the production parsing, expansion, metadata,
checking, inspection, and transport path that is in scope. Reviewed snapshots
record what that execution produced; snapshots are never loaded as semantic
inputs to manufacture the result under test.

Small constructed values remain appropriate for unit-testing representation
code, and narrowly scoped test doubles may inject failures at operating-system
or external-service boundaries. Neither establishes that Sage computed a
semantic result correctly. See
[D19](./decisions.md#d19-semantic-evidence-starts-from-source).

<a id="ten-a7"></a>
> **TEN-A7 — Semantic integration evidence starts from source.** A semantic
> anchor is established by running real Rust source through the production
> layers named by the claim. Reviewed snapshots and traces are outputs of that
> run, not alternate semantic inputs. Constructed values and test doubles can
> establish only the isolated boundary they directly exercise.
>
> **Required verification:** Each semantic integration test identifies its
> checked-in Rust project, production entry point, observed layers, reviewed
> snapshots or traces, and any deliberately excluded boundary. Replacing a
> production semantic layer with precomputed JSON or a scripted provider must
> make the test ineligible as evidence for that layer.

## Naming conventions

- `*Cst` — CST nodes (stash-allocated, per-item): `TypeCst`, `ExprCst`,
  `PathCst`, `FnCstData`, `StructCstData`
- `*Sym` — symbols (salsa-tracked or enum wrappers): `LocalFnSym`,
  `Symbol`, `FnSymbol`
- `Ty*` — typed-tree nodes: `TyExpr`, `TyStmt`, `TyPat`, `TyBody`
- `*Sig` — signature payloads: `FnSig`, `StructSig`, `StructFields`

## Current status

These tenets span several phases, so their claim-specific evidence lives in
the focused chapters rather than being duplicated here:

- [TEN-A1](#ten-a1) and [TEN-A2](#ten-a2): [Signature
  Checking](./pipeline/signature-checking.md), [Body Checking and Typed-IR
  Elaboration](./pipeline/body-checking.md), and [Symbols](./infrastructure/symbols.md);
- [TEN-A3](#ten-a3): [Stash](./stash.md);
- [TEN-A4](#ten-a4): [Name Resolution](./subsystems/name-resolution.md);
- [TEN-A5](#ten-a5): [Incrementality and Query
  Boundaries](./infrastructure/incrementality.md); and
- [TEN-A6](#ten-a6): [Oracle Test Harness](./oracle-test-harness.md); and
- [TEN-A7](#ten-a7): [Validation and Inspection](./validation/README.md) and
  [Semantic Inspector](./validation/semantic-inspector.md).

The principal current gap is [TEN-A5](#ten-a5): several coarse same-file and
module dependencies cause reexecution before an equal semantic result can be
backdated. The incrementality chapter distinguishes that extra execution from
downstream propagation and records the current edit evidence.
