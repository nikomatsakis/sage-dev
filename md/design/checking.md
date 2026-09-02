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

<a id="check-a1"></a>
> **CHECK-A1 — Public checking queries are symbol-keyed semantic boundaries.**
> A caller supplies the definition symbol and the query derives scope and
> environment from it. Signatures are the primary cross-item interface;
> field, member, and other language-required interfaces may be separate narrow
> products. Bodies and checking temporaries are not interfaces. This is the
> shared query design required by
> [D15](./decisions.md#d15-cross-item-dependencies-stop-at-semantic-interfaces).
>
> **Required verification:** Query traces show a signature request and a body
> request as distinct symbol-keyed roots, reject ambient caller parameters and
> callee-body reads, and demonstrate unchanged reuse across an
> interface-preserving edit.

<a id="check-a2"></a>
> **CHECK-A2 — Semantic identities have exactly one minting boundary.** An
> owning query creates each generic parameter or associated-item symbol once.
> Every dependent query reopens or references that identity instead of
> reconstructing it from its name or source position. The identity family is
> defined by
> [D5](./decisions.md#d5-symbols-form-the-uniform-semantic-identity-family).
>
> **Required verification:** Cross-query and persistent-edit tests compare
> owner, generic-parameter, `Self`, and associated-item identities across
> signatures, field/member details, and bodies, including shadowed equal names.

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

**Purpose-specific contexts.** `Check` lowers signatures and their parameter
environments into a target stash. `InferCtx` owns the body target stash plus
inference, obligations, diagnostics, and cooperative tasks. Both treat the
source CST stash as read-only and return self-contained semantic output.

**`Stashed<T>` is the memoization boundary.** Salsa compares fingerprints
(content hashes of the output stash) for change detection. Deterministic
allocation means same CST + same scope = same fingerprint = no downstream
re-execution.

<a id="check-a3"></a>
> **CHECK-A3 — Checked products own their semantic storage.** CST is immutable
> input; each checked signature or body allocates into a fresh output stash and
> returns a self-contained `Stashed<T>`. Its deterministic content fingerprint,
> rather than allocation address or source-tree lifetime, is the value used for
> backdating. [D2](./decisions.md#d2-stash-for-type-level-interning) owns this
> representation choice.
>
> **Required verification:** Structural traversal detects any source-stash or
> tree-sitter pointer in checked output, equal recomputation yields equal
> fingerprints, and a persistent edit test shows an equal recomputed product
> stops downstream reexecution.

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

<a id="check-a4"></a>
> **CHECK-A4 — A checked body cannot publish residual inference work.** Solver
> responses may add substitutions and residual goals during checking, but
> fallback and a mandatory terminal pass must discharge them or emit a
> diagnostic before `CheckedBody` is created. Incomplete solver results are
> terminal outcomes under
> [D16](./decisions.md#d16-incompleteness-is-an-explicit-terminal-outcome), not
> work silently left running after return.
>
> **Required verification:** Tests cover obligations solved immediately,
> woken after inference, deduplicated after canonicalization, rejected at the
> terminal pass, and exhausted by a resource limit; successful completion
> asserts no live obligations, tasks, wakes, or root branches.

The implemented external-inherent slice opens owner and method type generics
in one synchronous child egraph version. Receiver and argument compatibility
must all succeed before the child collapses into the body root. Its instantiated
parameter environment is accumulated in `StagedObligationBatch` and published
only after commit, so a rejected call cannot leak generic equalities, wakeups,
or obligations. This transaction does not cross an await point. The broader
method algorithm will extend the same boundary to result compatibility,
autoderef, conditional candidate responses, and local inherent methods.

<a id="check-a5"></a>
> **CHECK-A5 — Speculative checking changes commit atomically.** A candidate or
> multi-part compatibility check stages its inference changes and obligations
> in an isolated child version. The whole operation either commits to its
> direct parent or is discarded; no equality, wake, or obligation from a
> rejected alternative reaches the requester. The version invariants are
> defined by
> [D6](./decisions.md#d6-versioned-egraph-children-are-inference-transactions).
>
> **Required verification:** Unit tests reject a candidate after partial
> generic matching and observe the parent unchanged, accept the same shape and
> observe exactly one staged obligation publication, and exercise isolated
> sibling candidates without shared inference identities.

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

Current evidence maps to the anchors as follows:

- **[CHECK-A1](#check-a1), [CHECK-A2](#check-a2).** The [Signature Checking current
  status](./pipeline/signature-checking.md#current-status) links binder-identity,
  field-separation, and narrow external-dependency tests.
- **[CHECK-A3](#check-a3).** The [struct-signature](./examples/struct-signature.md) and
  [function-body](./examples/function-body.md) walkthroughs follow semantic
  values across fresh signature, field, and body output stashes.
- **[CHECK-A1](#check-a1), [CHECK-A4](#check-a4).** The [Body Checking current
  status](./pipeline/body-checking.md#current-status) links structural Typed-IR,
  terminal-obligation, exact-oracle, and cold/warm dependency evidence. A
  persistent same-file edit currently exposes a coarser invalidation boundary
  than CHECK-A1 requires.
- **[CHECK-A5](#check-a5).** `external_method_mismatch_discards_partial_generic_bindings`
  is the focused transaction test in `check/method.rs`; complete observable
  wake and staged-obligation evidence is not yet linked from a public review
  packet.
