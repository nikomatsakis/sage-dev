# Shared Checking Design

This chapter records mechanisms shared by the [signature-checking
phase](./pipeline/signature-checking.md) and [body-checking
phase](./pipeline/body-checking.md). Begin with those phase contracts for
inputs, outputs, failure modes, incremental boundaries, and evidence; use this
page as the deeper reference for their common query and stash design.

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
- `check::sig` — `Check`, the signature-lowering context
- `check::infer_ctx` — `InferCtx` and `Scope`, the body-checking context
- `resolve` — `Resolver`, `Namespace`, and lexical ribs

Item-specific logic imports from these; it doesn't redefine its own plumbing.

## Query design

**Single-keyed queries.** `sym.sig(db)`, `sym.body(db)` — the symbol knows
its scope, the query derives everything else. No ambient parameters threaded
from callers.

**`sig` is the cross-item boundary.** A signature query extracts exactly what
other items need to type-check against this one: generics, parameter types,
return type, field types. It is the minimal public surface.

**Associated-item identity is a separate narrow boundary.**
`LocalTraitSym::items` and `LocalImplSym::items` return stable owner-linked
symbols. A consumer then queries only the selected item's signature; item
enumeration does not check any associated body.

**Detail queries are lazy.** `body()`, `fields()`, and similar queries compute
information that is either not needed from other items or only needed some of
the time. They depend on `sig()` but are not depended on by other items'
signatures.

**Symbols and generic parameters are minted exactly once.** An owner `sig()`
query mints its `GenericParam` symbols via
`cst.generics.check(db, cx, parent)` and stores them in a `Binder`.
`LocalTraitSym::items` and `LocalImplSym::items` mint stable owner-linked item
symbols; an associated function's `sig()` reuses the owner binder before
minting method-level generics. Detail queries such as `fields()` and `body()`
open those binders and bring the same parameter symbols and owner `Self` type
into scope. No query remints an existing identity.

**Sequential layering inside a query.** `body()` calls `sig()` → opens
binder → resolves names → runs inference → elaborates the completed typed
tree. Each step builds on the prior. From outside: one query, one result.
Intermediates (`ResolvedBody`, method candidates, adjustment recipes, and
partially inferred expressions) are not separately queryable.

## Completed body output

The result of successful body checking is the [elaborated typed
IR](./typed-ir.md), not a typed copy of source syntax. Resolved definitions,
substitutions, borrows, dereferences, and coercions are materialized in the
tree. A method resolver may describe an autoref or autoderef while selecting a
candidate, but body elaboration consumes that description before returning
`CheckedBody`.

Checking one body may query callee signatures, associated items, impl headers,
and trait-solver results. It does not query callee bodies. This is both a
layering rule and an incremental dependency requirement.

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

## Trait obligations

Trait proof and type-producing normalization operations are handled by the
`check::solve` subsystem under
the [trait-solver semantic contract](./trait-solver.md). The
body checker keeps an obligation registry rather
than treating a conditional solver answer as final: substitutions are applied,
residual goals remain registered, and every residual must either be discharged
or produce a diagnostic before body checking finishes.

The Salsa query boundary canonicalizes the caller's local inference state. A
canonical variable records whether it represents a rigid caller parameter or a
bindable inference variable, together with its kind and relative current
universe ceiling. The
query records the current universe relative to a caller-retained absolute base,
so closed queries and nested binders can reopen response existentials safely.
The query also includes the local crate and parameter environment, so local
impl discovery and memoization do not depend on ambient state.

Proof-local equality changes are transactional. A short-lived operation runs in
a child egraph version and collapses that child into its direct parent only
after the whole operation succeeds. A failed operation discards the child.
Concurrent impl candidates own isolated proof contexts and match within a
local child version; they produce canonical responses and are dropped rather
than being merged into a requester. Within each egraph, inference-variable IDs
are globally unique and carry an owning version, so branch-local types cannot
be reinterpreted outside their owning ancestry.

The body environment contains opened function predicates and the deduplicated
defining predicates of referenced local traits. Generic function uses and
struct/enum construction or explicit type uses submit their instantiated
parameter environments. Once trait method selection is complete, associated
projections in its instantiated signature are replaced by caller inference
variables and registered as input-only normalization operations; the caller's
expected type is related only after the solver output is imported. Obligations
retain source provenance, proof obligations deduplicate after canonicalization,
retry only after relevant inference wakes, and receive a mandatory terminal
pass after inference fallback. `CheckedBody` creation asserts that the
obligation registry, runtime, wake queue, and root egraph have no live work.

The implemented external-inherent slice opens owner and method type generics
in one synchronous child egraph version. Receiver and argument compatibility
must all succeed before the child collapses into the body root. Its instantiated
parameter environment is accumulated in `StagedObligationBatch` and published
only after commit, so a rejected call cannot leak generic equalities, wakeups,
or obligations. This transaction does not cross an await point. The broader
method algorithm will extend the same boundary to result compatibility,
autoderef, conditional candidate responses, and local inherent methods.

## Deferred lifetime and borrow semantics

Lifetime syntax remains in the CST, but checking currently maps every
explicit, elided, universal, existential, external, and synthesized lifetime
directly to `Lifetime::Dummy`. No lifetime inference variables are introduced.
The only lifetime relation is `Outlives(Dummy, Dummy)`, which succeeds.

References and dereferences remain ordinary typed operations. Sage does not
currently validate liveness, uniqueness, overlap, or any other borrow
property. This deliberate soundness hole avoids committing to a separate
region-inference subsystem before Sage's unified type-and-lifetime inference
design is settled.

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

## Current status

The single-keyed signature/body queries, two-stash boundary, transactional
inference, obligation finalization, elaborated output, and Dummy-lifetime
policy described above are implemented for the Rust subset recorded in the
two phase chapters. Their **Current Status** sections own concrete capability,
limitation, evidence, and roadmap information so this shared mechanism page
does not duplicate it.
