# RFD: Method and Trait Resolution

**Status:** Draft

**Depends on:**

- [Trait System](../trait-system/README.md) — checked trait and impl signatures and local impl enumeration
- [Trait Solving](../trait-solving/README.md) — proof of a fixed trait goal
- [Type Inference](../type-inference/README.md) — `InferCtx`, probes, and inference variables
- [Per-kind symbol data](../per-kind-symbol-data/README.md) — function, impl, and trait symbols

## Problem

The type checker currently gives method calls a fresh result type without finding a method.
Associated functions on type paths and trait-provided methods are likewise unresolved. Sage
needs a lookup algorithm that discovers methods by name, tests candidate receiver types
without leaking speculative equalities, and records any trait obligations selected by the
call.

## Scope

The MVP covers:

- methods from local inherent impls;
- methods from explicit type-parameter bounds and local traits in lookup scope;
- one built-in reference dereference step and receiver auto-reference;
- checked `self`, `mut self`, `&self`, and `&mut self` receiver forms, with
  associated functions excluded from dot-call lookup;
- deferral while the receiver or a trait proof remains ambiguous;
- transactional instantiation of the unique method signature;
- registration of residual trait obligations with body checking.

External methods, prelude and import-complete trait visibility, custom `Deref`, associated
type normalization, specialization, and full UFCS are follow-up work. Coherence remains a
separate concern; lookup must preserve ambiguity instead of choosing an overlapping method.

Local privacy is not deferred. Before lookup, this RFD preserves
`visibility_modifier` syntax on local function/trait CST and symbols, lowers it
to a checked `Visibility` (`private`, `pub`, `pub(crate)`, `pub(super)`, and
`pub(in path)`), and provides `is_visible_from(defining_module,
lookup_module)`. Import/prelude completeness controls which traits are brought
into the candidate set; visibility separately rejects inaccessible traits and
inherent functions already discovered.

## Solver boundary

Method-name discovery does not belong in the trait solver. A solver goal always names one
fixed trait:

```text
LookupSelfTy: CandidateTrait<TraitArgs...>
```

There is no existential `?Trait` solver query. The resolver invokes the
goal-specific proof operation, whose successful semantic output is `Proven`
alongside substitutions and residual obligations; it does not return an impl or
trait identity. The method resolver already knows the candidate trait and
obtains the function symbol from that trait's items. This keeps solver caching
and clause selection independent of name lookup. Future callable-instance or
vtable operations may return other output variants, but their representation
and code-generation role are outside this RFD.
`LookupSelfTy` is the receiver after the current built-in deref step but before
the selected method's value/shared/mutable autoref adjustment.

Trait-method integration is implemented by this RFD after the core solver is available. It
is not a final step of the trait-solving RFD.

## Resolution result

Inherent methods retain their owning impl because the impl binder is needed to instantiate
the method signature. Trait methods retain the already-selected trait, not a selected impl.

```rust
pub enum MethodOrigin<'db> {
    Inherent {
        impl_sym: ImplSymbol<'db>,
    },
    Trait {
        trait_sym: TraitSymbol<'db>,
    },
}

pub struct ResolvedMethod<'db> {
    pub fn_sym: FnSymbol<'db>,
    pub origin: MethodOrigin<'db>,
    pub adjustments: ReceiverAdjustments,
}

pub enum MethodResolution<'db> {
    Found(ResolvedMethod<'db>),
    NeedsMoreInfo,
    Ambiguous,
    NotFound,
    /// Propagate an existing receiver error without a secondary lookup error.
    Error(ErrorReported),
}
```

`ReceiverAdjustments` is an internal candidate-selection and commit recipe. A
successful method call consumes it while constructing the [elaborated typed
IR](../../design/typed-ir.md): dereferences, borrows, and coercions become
ordinary expression nodes, and the completed body contains a resolved call
target rather than `MethodCall` plus an adjustment list.

`Found` means the unique candidate's inference effects have been applied transactionally
and all nontrivial residuals have been registered with the caller's obligation store. It
does not mean those residuals may be forgotten; body finalization must discharge them.

If receiver normalization yields `Ty::Error(existing)`, lookup immediately
returns `Error(existing)`. It does not enumerate methods or emit
`NotFound`/ambiguity; the checked call becomes recovery IR carrying the original
error.

Candidate evaluation distinguishes three non-`No` states at one receiver step
and priority tier:

- **definite** — `Yes` with a trivial residual;
- **conditional** — `Yes` with a nontrivial residual retained with that
  candidate; and
- **unknown** — solver `Maybe`, incomplete external/provider metadata, or a
  potentially relevant owner/method signature whose eligibility is
  `Unsupported`.

No candidate response or residual is applied while constructing this set. The
rows below are considered in order, making the resolution result deterministic:

| Definite | Conditional | Unknown | Result |
|---:|---:|---:|---|
| 0 | 0 | 0 | `NotFound` (only when candidate sources are exhaustive) |
| 1 | 0 | 0 | `Found` |
| 0 | 1 | 0 | `Found`, with that candidate's residual staged for commit |
| 2+ | any | any | `Ambiguous` |
| 0-1 | any | 1+ | `NeedsMoreInfo` |
| 1 | 1+ | 0 | `NeedsMoreInfo` |
| 0 | 2+ | 0 | `NeedsMoreInfo` |

A sole conditional candidate may be selected because failure of its residual
makes the method call fail; there is no alternative method choice to recover.
A definite candidate plus a conditional or unknown candidate, multiple
conditional candidates, or a conditional candidate plus an unknown candidate
remains `NeedsMoreInfo`, because a competitor may become applicable or
disappear. `NeedsMoreInfo` retains the original method-lookup request and its
stall dependencies as a body-checker lookup obligation; it does not publish any
one candidate's residual obligations. The lookup is rerun after a relevant
inference change. At final body-checker fixpoint, an unresolved lookup becomes a
diagnostic rather than an arbitrary selection.

## Candidate discovery

### Inherent candidates

The crate, rather than a source-file collection, is the lookup boundary:

```rust
fn inherent_method_candidates<'db>(
    db: &'db dyn Db,
    krate: LocalCrateSymbol<'db>,
    receiver_head: Symbol<'db>,
    name: Name<'db>,
) -> Vec<InherentMethodCandidate<'db>>;
```

Candidate discovery linearly scans `local_impls(db, krate)`, keeps signatures with
`trait_ref: None` and the requested self-type head, then finds function items with the
requested name which satisfy `is_visible_from` for the call's lookup module. An inaccessible
function is not an applicable or unknown candidate, though diagnostics may
mention it after lookup otherwise returns `NotFound`. Each candidate opens its
single `Binder<ImplSignatureData>` independently; the self type, where-clauses,
and method signature share that opening.

Dot-call discovery keeps only functions with a supported `CheckedReceiver`.
The receiver's opened `owner_self_ty` and `MethodReceiver` form determine
whether the current deref/autoref step can call it. A function with no receiver
is an associated function, not a dot-call candidate; typed or explicit-lifetime
receivers remain unsupported rather than being guessed from `Ty::Infer`.

Classify each post-deref lookup self-type head by inherent provider. A local
nominal head uses the exhaustive local scan. A structural head on which Rust
permits no inherent provider (notably the `&T`/`&mut T` wrapper before the
built-in deref step) is exhaustively empty and may continue to the next adjustment. An
external nominal, primitive, slice/`str`, or other builtin provider whose item
metadata is unavailable makes the inherent tier unknown/incomplete. Because
inherent methods have priority, that last case returns
`NeedsMoreInfo`/unsupported rather than falling through to a local trait method
or concluding `NotFound`.

Only `SolverEligibility::Eligible` impl signatures can be opened by the MVP
method resolver. A potentially matching inherent impl marked `Unsupported`
(including a lifetime/const-generic impl) contributes an unknown/incomplete
candidate source; it is not silently omitted in a way which permits
`NotFound` or a competing `Found` result.

The function item must also have eligible method-candidate metadata. A
same-name function with unsupported receiver syntax, lifetime/const
method-level generics, an incomplete own parameter environment, or a deferred
projection in its signature contributes an unknown candidate; the represented
subset must never become `Found`.

Matching the full impl self type against the post-deref lookup self type is
speculative. It runs in an inference probe and commits only after the resolver
selects one candidate. A failed or discarded candidate cannot leave receiver
or impl-generic equalities behind.

### Trait candidates

The resolver enumerates traits that provide the requested method name from:

1. explicit bounds in the current parameter environment; and
2. local traits visible from the call's lookup scope.

Candidate discovery returns `(TraitSymbol, FnSymbol)` pairs. It does not inspect trait
impls. For each pair, the resolver opens the trait generics with fresh type inference
variables in an isolated probe and asks the solver the fixed goal
`LookupSelfTy: Trait<Args...>`.

The pair is the canonical discovery identity and is deduplicated before any
opening or outcome counting. Explicit bounds and the visible-local-trait scan
are two ways to discover the same trait item, not two method candidates. Trait
arguments are opened once for the deduplicated pair; the solver environment can
then constrain that opening from an explicit bound.

As with inherent lookup, the matching trait function must have a supported
checked receiver. Associated functions are excluded from dot syntax even when
their names match.

Trait functions use the same method-candidate eligibility gate. An eligible
trait owner does not make a partially represented method signature eligible.

An explicit bound naming an external trait is still a trait in lookup scope.
If external trait-item metadata is unavailable, the requested name may exist,
so that bound contributes an unknown/incomplete method source. It cannot be
silently omitted in favor of `NotFound` or a competing local trait.

The resolver opens only an `Eligible` local trait signature. A relevant
`Unsupported` signature is tracked as an unknown candidate, since its complete
generic and defining-predicate contract is unavailable to the type-only
resolver.

Solver outcomes are interpreted as follows:

- `No` removes that trait candidate;
- `Maybe` contributes an unknown candidate and therefore yields
  `NeedsMoreInfo` unless two already-definite candidates establish
  `Ambiguous` independently;
- `Yes` makes the trait a viable candidate, but its substitution remains in the probe;
- `Yes` with residuals is conditional and remains viable only together with those
  residual obligations.

The resolver classifies every candidate using the table above before selecting
one. It retains, but does not yet apply, each response and conditional residual.
Applying responses while enumerating would make candidate completion order
affect inference.

An explicit bound such as `T: Iterator` uses the same path: `Iterator` is a discovered
trait candidate, and the solver proves the fixed goal from its environment assumption.

## Lookup order and receiver adjustments

The MVP first builds a deterministic lookup-self sequence:

1. the canonical receiver type;
2. one built-in dereference from `&T` or `&mut T` to `T`.

Impl-head matching and the fixed trait goal always use that step's
`LookupSelfTy`. Only after a candidate is viable does its `CheckedReceiver`
choose the call adjustment: value receiver, shared autoref `&LookupSelfTy`, or
mutable autoref `&mut LookupSelfTy`. The adjusted call receiver is checked
against the method signature but is never substituted for `LookupSelfTy` in
the impl/trait proposition. Thus `s.foo()` for `fn foo(&self)` proves
`S: Trait`, not `&S: Trait`, while recording an `&S` autoref.

At each receiver step, inherent methods are considered before trait methods. A
lower-priority tier is considered only after the higher-priority tier is
exhaustively `No`. Search stops at the first receiver step whose tier produces
`Found` or `Ambiguous`; `NeedsMoreInfo` defers that step without falling through.
Multiple definitely applicable inherent methods or multiple definitely
applicable traits at that step produce `Ambiguous`; lookup does not use source
order as a tie-breaker. Conditional and unknown combinations follow the table
above.

A bare inference-variable receiver yields `NeedsMoreInfo` and is retried when its bound
changes. The MVP does not enumerate every method in the crate to infer an unconstrained
receiver type.

Custom `Deref` requires associated-type normalization and is deferred. Its eventual
receiver sequence will be built by repeatedly proving the fixed `T: Deref` goal and
normalizing `Deref::Target`; the method resolver still owns the sequence and name lookup.

## Instantiating method signatures

### Inherent methods

1. Open the owning impl's single binder with fresh variables.
2. Unify the opened impl self type with the selected `LookupSelfTy` in a probe.
3. Prove the opened impl where-clauses and retain any response/residual locally.
4. Import the function signature using the same impl-generic substitution.
5. Instantiate method-level generics from explicit arguments or fresh variables.
6. Instantiate the method function's own checked parameter environment with
   those method generics and prove/stage every resulting predicate.
7. Check argument, receiver, and result compatibility in the same selected-method
   transaction.
8. Commit inference effects and publish staged residual obligations only after
   every check succeeds.

### Trait methods

1. Retain the trait-generic substitution from the unique fixed-trait proof.
2. Load the function signature from the trait item, not an impl item selected by the
   solver.
3. Substitute `Self` and trait arguments, then instantiate method-level generics.
4. Instantiate and prove/stage the method function's own checked parameter
   environment after method-generic substitution.
5. In one selected-method transaction, apply the solver response and check the
   receiver, call arguments, and result against the instantiated signature.
6. Stage all trait-proof and method-predicate residual obligations inside that
   transaction. Publish them only when the inference child commits; discard
   them with every failed compatibility check.

The selected-method transaction covers both inference state and obligation
registration, including the selected function signature's own predicates.
Obligation records are first accumulated in a transaction-local
buffer; the global obligation store is not versioned by the egraph and must not
be mutated before commit. A signature/argument mismatch reports a call error and
discards the selected candidate's response, fresh method variables, wakeups,
and staged obligations together. Method arguments are not used as overload
resolution tie-breakers: once name/receiver lookup selected a method, an
argument mismatch does not fall through to a losing candidate.

Caller-egraph probes are synchronous and non-suspending. Receiver/argument
expression tasks and any `await_concrete` wait finish (or return
`NeedsMoreInfo`) before a child version is created; Salsa solver calls execute
synchronously and return canonical responses inside the same poll. No method
future may hold a child version across `.await`, because leaf-only version
mutation freezes the caller parent while that child exists.

Associated type occurrences in trait method signatures remain unsupported until
normalization is implemented.

## Associated functions and qualified paths

For the MVP, `Type::function` considers visible local inherent functions with
no `CheckedReceiver`; receiver-taking functions through associated syntax are
part of deferred UFCS. Candidate impl-head matching, ambiguity, impl
where-predicate proof, function-generic/parameter-environment instantiation,
argument/result compatibility, and obligation staging use the same isolated
classification plus single selected-call transaction as dot calls.

Fully qualified `<Type as Trait>::function` already supplies a fixed trait. Once
qualified-path lowering exists, the MVP supports receiver-less associated
functions by proving `Type: Trait<Args>`, loading the trait item, and performing
the same transactional call checks. Visibility and unsupported/incomplete
metadata use the ordinary outcome table. Unqualified trait-associated function
lookup and complete UFCS behavior remain deferred.

External receiver-bearing dot calls now have a narrow rigid-ADT metadata path.
External `Type::function` lookup remains unsupported/unknown because that path
deliberately excludes receiver-less associated functions; the absence of local
impls is not an exhaustive `NotFound` result.

## Deferred work

The first planned follow-up which combines an external trait method,
associated-type normalization, and an external inherent method is the
mini-redis [`Parse::next` vertical slice](../associated-type-normalization/README.md).

- General external inherent and trait methods beyond the implemented rigid-ADT
  inherent and fixed-trait vertical slices.
- Lifetime/const-generic impl, trait, and method candidate instantiation.
- Complete import, visibility, and prelude handling for traits in lookup scope.
- Custom `Deref` chains and associated type normalization.
- Full UFCS, trait-associated functions, and explicit method turbofish handling.
- Primitive, slice, `str`, and other builtin method providers.
- Specialization-aware priority after coherence support exists.

See [Implementation plan and status](./implementation.md) for the staged work.
