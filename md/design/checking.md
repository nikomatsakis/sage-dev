# Checking

Design tenets for the CST checking layer — guides all new code in
`cst/`, `local_syms/`, and related modules.

## Code organization

**Methods live on the types they process.** Type-checking, name resolution,
and lowering are inherent methods on CST and symbol types — not standalone
functions, not visitor traits. `TypeCst::check(cx)`, `PathCst::resolve(cx, ns)`,
`ExprCst::check(bx)`, `LocalFnSym::sig(db)`, `LocalStructSym::fields(db)`.

**Master modules with submodules.** Each concern gets a top-level module
(`cst/`, `local_syms/`, `infer/`) subdivided by item kind or sub-concern.
Avoid monolithic files. One struct or closely-related cluster per file.

**Shared infra lives in pass-named modules.** Contexts, helpers, and traits
shared across item kinds live in a module named after the pass:
- `cst::check` — `CstLowerCtx`, `BodyCheckCtx`
- `resolve` — `Resolver`, `Namespace`
- `ribs` — `Ribs`, `RibEntry`

Item-specific logic imports from these; it doesn't redefine its own plumbing.

## Query design

**Single-keyed queries.** `sym.sig(db)`, `sym.body(db)` — the symbol knows
its scope, the query derives everything else. No ambient parameters threaded
from callers.

**`sig` is the cross-item boundary.** A signature query extracts exactly what
other items need to type-check against this one: generics, parameter types,
return type, field types. It is the minimal public surface.

**Detail queries are lazy.** `body()`, `fields()`, and similar queries compute
information that is either not needed from other items or only needed some of
the time. They depend on `sig()` but are not depended on by other items'
signatures.

**Generic parameters are minted exactly once.** The `sig()` query mints
`GenericParam` symbols via `cst.generics.check(db, cx, parent)` and stores
them in a `Binder`. All other queries (`fields`, `body`) open that binder and
bring the same param symbols into scope via `ribs.add_generic_params`. No
re-minting, no identity confusion.

**Sequential layering inside a query.** `body()` calls `sig()` → opens
binder → resolves names → runs inference. Each step builds on the prior.
From outside: one query, one result. Intermediates (`ResolvedBody`) are not
separately queryable.

## Data flow: the two-stash pattern

**CST is read-only input; output goes to a fresh stash.** The context bridges
`src: &Stash` (the CST) and `dst: Stash` (output types or resolved exprs).
At the end, `cx.finish(root)` wraps `dst` into `Stashed<T>`.

**Purpose-specific contexts.** `CstLowerCtx` for signatures (produces `Ty`
into `dst`). `BodyCheckCtx` for bodies (produces `CheckedExpr` into `out`).
Same ingredients (resolver, ribs, src/dst stash pair), different output
domain.

**`Stashed<T>` is the memoization boundary.** Salsa compares fingerprints
(content hashes of the output stash) for change detection. Deterministic
allocation means same CST + same scope = same fingerprint = no downstream
re-execution.

## Resolution model

**Ribs first, module-scope fallback.** Ribs capture lexically-scoped bindings
(generics, locals, `Self`). If the first path segment isn't found in ribs,
fall through to `resolver.resolve_segments()` for module-level names.

## CST representation

**Stash-allocated, relative-spanned, `AllocStashData`.** Per-item stashes
with content-addressed (hash-consed) allocation. CST type aliases follow the
pattern `type FnCst<'db> = Stashed<Ptr<FnCstData<'db>>>`. The CST captures
all syntactic detail needed for later phases. No back-pointers to tree-sitter
nodes.
