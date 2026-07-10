# Implementation plan and status

Each step must leave the workspace building and must add focused tests for the
behavior it introduces. A checkbox is complete only when its implementation,
tests, and any documentation affected by that implementation land together.

The steps are ordered by dependency. In particular, multi-candidate proof search
does not begin until inference mutations are transactional, egraph versions are
explicit, scoped task cleanup exists, and the answer algebra has been tested in
isolation.

## MVP invariants

The first implementation is a positive, inductive, type-only solver. The
following invariants are requirements, not follow-up improvements:

- A failed unification, failed substitution application, failed candidate, or
  cancelled task leaves its parent egraph and parent wake state unchanged.
- Every egraph operation names an explicit version. No future may select a
  process-wide or context-wide `current` version and then suspend.
- Caller generic parameters are rigid placeholders. Caller inference variables
  are flexible existential inputs. Canonicalization preserves that distinction
  and their relative universes.
- Only flexible existential inputs may be substitution keys. A solver response
  must never bind a rigid placeholder.
- A definite `Yes` response preserves all repeated-variable sharing. The same
  fork-local variable encountered in the substitution or residual is
  represented by the same response-bound existential.
- Candidate completion order does not affect the final canonical result. Only
  an empty-substitution, trivially unconditional `Yes` may end candidate search
  early.
- A returned residual remains under every environment needed to prove it.
- Every residual obligation is either later discharged or reported during type
  checker finalization. It may not be silently dropped.

## Code organization

The implementation stays in `sage-ir` and extends the existing checking and
inference infrastructure:

```text
crates/sage-ir/src/check/
  infer/
    egraph.rs       explicit-version operations and child probes
    version.rs      version ownership, globally unique variables, universes
    runtime.rs      scoped tasks, joins, cancellation, version-aware wakes
    skeleton.rs     existing type decomposition/recomposition
    unify.rs        transactional, diagnostic-free structural unification
    obligations.rs  body-checker obligation manager and final discharge
  solve/
    mod.rs          public solver surface
    goal.rs         Goal, Atom, Assumption, Clause, canonical variable metadata
    result.rs       QueryResult, GoalResult, Subst, response binders
    canonical.rs    input canonicalization and response extraction/instantiation
    ctx.rs          proof-local stash, egraph, environment, depth, and scopes
    whiteboard.rs   per-query proof frames, deduplication, and cycle checks
    prove.rs        structural proving and atomic candidate execution
    clauses.rs      environment and local-impl candidate assembly
    merge.rs        order-independent answer-set reduction
    anti_unify.rs   conservative hint construction
    subsume.rs      directional conservative subsumption
```

`check/mod.rs` grows `pub mod solve;` when the solver module is introduced.
Narrow unit tests live beside the module they exercise. End-to-end solver and
type-checker tests live under `crates/sage-ir/tests/`.

## Step 0: Freeze the MVP and land prerequisites

This step removes design ambiguity before solver code begins. Foundations in
Steps 1-8 may be developed before all trait syntax is available, but Step 9 is
blocked on the Trait System prerequisite.

- [ ] Freeze the MVP atom language to types only:
  - [ ] `TraitImpl { self_ty: Ptr<Ty>, trait_ref: TraitRef }`
  - [ ] `Equals(Ptr<Ty>, Ptr<Ty>)`
- [ ] Freeze the MVP structural goal language to `Atom`, `All`, `Exists`,
  `Implies`, and `Maybe`.
- [ ] Explicitly defer `ForAll`, normalization/projections, outlives proving,
  higher-ranked lifetimes, auto-trait structural candidates, external impls,
  coherence, specialization, and negative reasoning.
- [ ] Land the [Trait System](../trait-system/README.md) data model needed by the
  solver: `TraitRef`, checked function/owner parameter environments, trait
  defining predicates, `ImplSignature`, and `SolverEligibility` gates.
- [ ] Ensure an impl's generic parameters bind its self type, trait reference,
  and where-clauses as one coherent scope. The solver must be able to open all
  three with one fresh candidate substitution.
- [ ] Land the Trait System's deterministic
  `local_impls(db, LocalCrateSymbol)` query. The solver linearly scans that
  list; indexing is deferred.
- [ ] Use `LocalCrateSymbol` as explicit query context. Candidate discovery must
  never infer the local crate from ambient state.
- [ ] Set `MAX_PROOF_DEPTH` to a named MVP default of 64. Reaching the limit
  yields `Maybe`, while an inductive ancestor cycle yields `No`.
- [ ] Reconcile the trait-solving README and implementation sketches with these
  frozen types before using them as coding references.

Readiness tests and fixtures:

- [ ] An impl fixture can open one binder and observe the same generic identity
  in its self type, trait reference, and where-clause body.
- [ ] Two local crates containing different impl sets enumerate different
  candidates for the same trait atom.
- [ ] No MVP Rust enum contains a placeholder variant for a deferred feature.

## Step 1: Explicit-version inference transactions and structural unification

First remove the branch-relative identity and implicit-version assumptions from
the egraph; the transaction API depends on those changes. Then factor
structural equality out of `InferCtx::require_eq`, making the new primitive
atomic from its first use. An operation against parent version `P` creates a
child probe `C`, performs all recursive work in `C`, rebuilds and validates `C`,
and then does exactly one of the following:

- success: explicitly collapse/commit `C` into `P`, publish the committed wake
  effects, and invalidate `C`;
- failure: discard `C` and all of its staged egraph and wake effects.

Candidate alternatives use a different operation: they keep their child branch
isolated, extract a response, and discard it. They never collapse their state
into the shared candidate parent.

- [ ] Make all egraph reads and writes take an explicit `Version`, or operate
  through an `EGraphView`/branch handle that permanently names one. Remove
  `set_current_version` from solver-facing code.
- [ ] Make inference-variable IDs globally unique across sibling versions and
  record each variable's owner version. Sibling allocation must not reuse the
  same `InferVarIndex`.
- [ ] Keep version ownership generation-safe: either do not reuse a `Version`
  identity during one egraph lifetime or include a generation in the handle.
  A discarded variable's owner must never become a valid new version.
- [ ] Enforce visibility: a version sees ancestor variables plus its own, but
  never variables owned by a sibling or descendant.
- [ ] Enforce leaf-only per-version mutation. Creating any child freezes the
  parent against path compression, variable allocation, equality/bound
  updates, rebuild publication, revisions, and inference wakes until all
  children are discarded or its sole child is atomically collapsed. Read-only
  lookup and append-only global stash allocation may continue.
- [ ] Add explicit `branch_from(parent)`, `collapse_into(child, parent)`, and
  `discard(child)` operations with assertions for parentage, live descendants,
  stale handles, and the MVP rule that a collapsed child is the target
  parent's only live child.
- [ ] Add a child-probe helper that cannot return without committing or
  discarding its child.
- [ ] Implement private in-probe structural unification over `Ty` using the
  egraph and `skeleton::{decompose, recompose}`.
- [ ] Expose `try_unify(parent, lhs, rhs)` as the transactional,
  diagnostic-free API. Do not expose a partially mutating public variant.
- [ ] Make a batch of structural equalities one transaction, not one child per
  equality. Step 4 uses this helper for `Subst` application after the solver IR
  exists.
- [ ] Perform an occurs check before commit, including indirect cycles after
  following egraph representatives. Reject `?X = Vec<?X>` and mutually
  recursive forms.
- [ ] Perform universe/leak validation before commit. A variable may not be
  bound to a placeholder from an inaccessible universe. Variables legitimately
  created by the committed child are atomically re-owned by the parent;
  references to an uncommitted descendant or discarded version are rejected.
- [ ] Store each variable's immutable creation universe separately from a
  versioned current universe ceiling. During equality, lower every flexible
  variable nested in a structural binding to the minimum ceiling of the
  containing eclass, iterate to a fixed point, and reject an inaccessible rigid
  placeholder. Stage and roll back these ceiling changes with the probe.
- [ ] Treat a committed ceiling decrease as a semantic update: advance the
  affected variable/dependency revision and publish its wake exactly once.
  Discarded lowering publishes neither.
- [ ] Reuse the same accessibility validator for versioned `Bound` writes.
  Reject an `AtLeast`/`Exactly` concrete type containing a rigid placeholder
  above the target variable's current ceiling before publishing the bound.
- [ ] Make collapse re-own every committed child variable before invalidating
  the child handle, preserving its globally unique ID and universe.
- [ ] Stage rebuild worklists, dependent updates, bound changes, and variable
  wakes in the probe. Publish them exactly once on commit and never on discard.
- [ ] Route `InferCtx::require_eq` through `try_unify` and preserve its current
  diagnostics and `Ty::Error` behavior.
- [ ] Keep alias expansion and associated-type normalization out of this helper.

Tests:

- [ ] Equal and unequal concrete leaves.
- [ ] Inference-variable binding and coalescing.
- [ ] Nested structural unification and skeleton/arity mismatches.
- [ ] Congruence after rebuild.
- [ ] A late mismatch rolls back earlier child equalities from the same
  recursive unification.
- [ ] A failed batch of equalities rolls back every earlier entry.
- [ ] Direct and indirect occurs-check failures leave the parent unchanged.
- [ ] Universe leaks and child-local-variable escapes are rejected.
- [ ] `?X@U0 = Vec<?T@U1>` transactionally lowers `?T` to `U0`; a later attempt
  to bind it to `P@U1` fails. Discarding the original probe restores `?T`'s old
  ceiling, while extracting a response variable records the lowered ceiling.
- [ ] Committing that lowering changes canonical input metadata and wakes a
  stalled obligation once; discarding it leaves the revision, wake queue, and
  later canonical query unchanged.
- [ ] `set_bound(?X@U0, AtLeast/Exactly(P@U1))` fails without changing the
  bound or waking consumers; an accessible placeholder succeeds.
- [ ] A discarded probe neither wakes a parent waiter nor leaves queued rebuild
  work; a committed probe wakes each affected parent waiter once.
- [ ] Lifetime and const skeleton fields continue to compare structurally even
  though solver substitutions are type-only in the MVP.
- [ ] `require_eq` retains its diagnostic and error-type behavior.

## Step 2: Scoped runtime tasks and interleaved forks

The detached `'static` task API and no-op wakers are not a safe substrate for
interleaved candidate futures. Refactor the runtime after Step 1 establishes
explicit, stable branch handles and before running alternatives concurrently.

- [ ] Add a scoped runtime API whose child futures may borrow the proof context
  and whose scope cannot finish until every child is joined or cancelled.
- [ ] Replace no-op polling wakers with real task wakers whose `wake` and
  `wake_by_ref` operations idempotently enqueue the owning suspended task.
  Whiteboard futures and inference waits must work through ordinary `Future`
  polling rather than a side channel known only to one wait primitive.
- [ ] Make cancellation explicit. Dropping sibling futures after an
  unconditional winner must run their cleanup, discard their branches, remove
  their waits, and release all whiteboard subscriptions owned by those tasks.
- [ ] Key inference waits/wakes by enough version/scope information that a
  speculative branch update cannot wake an incompatible sibling or parent.
- [ ] Do not discard or collapse a branch while a task, waiter, or descendant
  branch still references it.
- [ ] Provide a completion stream for scoped candidate tasks, but do not make
  scheduler arrival order part of answer identity.

Tests:

- [ ] Sibling branches allocate distinct variables and cannot inspect each
  other's variables or diffs.
- [ ] A semantic write to a parent with one or more live descendants is rejected
  and no child can observe a fact added after its branch point. After the last
  child is discarded, the parent becomes writable again.
- [ ] Two futures can interleave reads, writes, rebuilds, and waits on different
  branches without changing their selected versions.
- [ ] A normal pending future registers its runtime waker, is polled again after
  `wake`/`wake_by_ref`, and is enqueued at most once until that poll occurs.
- [ ] Committing one child does not import a sibling's changes.
- [ ] Cancelling a pending branch discards its egraph state and registrations
  and wakes/finishes the join scope.
- [ ] Cancelling an early, middle, or late sibling produces identical surviving
  parent state.
- [ ] A scope cannot return while a child still owns a branch.
- [ ] Branch-local wakes stay local; committed wakes become visible to the
  parent; discarded wakes disappear.

## Step 3: Solver IR and query context

Add the data model without candidate search or body-checker integration.

- [ ] Define `GoalQueryData` with exactly these contextual fields:
  `local_crate`, `canonical_universe`, `canonical_vars`,
  `next_response_param`, `assumptions_complete`, `assumptions`, and `goal`.
  The absolute caller universe base stays in the caller-side mapping rather
  than the cached key.
- [ ] Define canonical variable metadata as
  `{ param, kind, role, relative_universe }`, where `role` is either
  `RigidPlaceholder` or `ExistentialInput`.
- [ ] Define the type-only `Atom` variants frozen in Step 0. Keep `self_ty`
  explicit instead of hiding it in trait arguments.
- [ ] Define MVP `Goal`, `Assumption`, binder-owned `Clause`, `ClauseSource`,
  `GoalResult`, `QueryResult` with explicit response-bound variables, and
  `Subst`. Both MVP atoms prove only truth, so do not add a separate
  proof-value field yet.
- [ ] Make assumption heads statically trait-only. `Equals` is an intrinsic
  goal but cannot be placed in `Assumption`; hypothetical equality environments
  are deferred rather than silently ignored by atomic dispatch.
- [ ] Give every clause one binder covering its head and body. Opening a clause
  must produce one fresh mapping shared by both.
- [ ] Restrict `Subst` keys to canonical variables with role
  `ExistentialInput` and kind `Type`; reject duplicate keys deterministically.
- [ ] Give response-bound existentials explicit kind and relative-universe
  metadata rather than storing an untyped count.
- [ ] Define constructors that flatten nested conjunctions, preserve binder
  scope, and use `All([])` as the canonical true residual.
- [ ] Restrict MVP `Exists` binders to type parameters and keep their
  alpha-equivalent IDs in the query parameter space distinct from free inputs
  and response-bound variables.
- [ ] Keep trait-atom dispatch separate from equality-atom dispatch; generic
  orchestration must not assume every atom has a trait symbol or self type.

Tests:

- [ ] Empty and nested conjunction normalization.
- [ ] Clause head/body references remain in the same binder scope.
- [ ] Duplicate or rigid substitution keys are rejected.
- [ ] Response binders cover `subst` and `modulo` together.
- [ ] Structurally identical goal data with different `local_crate` values is
  not the same query context.
- [ ] Identical represented assumptions with complete versus incomplete source
  environments have distinct query identities and failure certainty.
- [ ] Deferred atom and goal forms cannot be constructed through the MVP IR.
- [ ] An equality assumption cannot be constructed; `Implies` with a trait fact
  works, while hypothetical equality support remains an explicit deferred
  feature.

## Step 4: Canonicalization, response extraction, and instantiation

Implement both directions of the solver boundary before adding proof search.

- [ ] Canonicalize the goal and assumptions in one deterministic traversal.
- [ ] Carry the source `CheckedParameterEnv` completeness/eligibility bit into
  `GoalQueryData::assumptions_complete`; never lose it while copying only the
  represented positive assumptions.
- [ ] Choose and retain an absolute caller universe base. Record the caller's
  current universe relative to it in the query; for an input-free query, use
  the current caller universe as base and relative universe zero.
- [ ] Canonicalize nested `Exists` binders with a push/pop binder map. Rename
  declarations and bound occurrences capture-avoidantly, exclude them from
  input metadata/reverse mappings, and reserve their alpha-parameter IDs before
  assigning response IDs.
- [ ] Extend the canonical copying/folding path to visit lifetime and const
  parameters embedded in a type. Canonicalize them as rigid placeholders;
  do not introduce lifetime/const inference or substitution keys in the MVP.
- [ ] Map caller generic parameters to `RigidPlaceholder` variables and caller
  inference variables to `ExistentialInput` variables. Inference variables
  appearing only in assumptions remain existential; they are not generalized.
- [ ] Preserve kind and universe as relative canonical metadata and retain the
  reverse mapping to the caller's original variables/placeholders.
- [ ] For every existential input, canonicalize its current versioned universe
  ceiling rather than creation universe. Ceiling changes must alter canonical
  query identity and cannot reuse a result computed with broader access.
- [ ] Instantiate rigid canonical variables as non-bindable placeholders and
  existential inputs as fresh inference variables in the query egraph.
- [ ] Import the query at `canonical_universe`; when applying a response, add
  each relative universe to the absolute base retained by that caller mapping.
- [ ] Extract substitutions only for existential inputs whose relation to the
  input changed.
- [ ] Project egraph classes to a deterministic response normal form before
  extraction. Pure flexible-variable classes prefer the lowest canonical
  existential query input; classes with a rigid placeholder or
  concrete/structured member retain a canonical fully substituted
  rigid/structural form. Do not let union-find root choice select the response
  orientation.
- [ ] Eliminate proof-local variables through query inputs when possible and
  fold the same projection through the residual. For example,
  `exists<T>. ?X = T && T: Bound` must extract residual `?X: Bound`, not a
  vacuous `?X = fresh ?T` alias.
- [ ] Canonicalize every definite response with one memo table from fork-local
  variables to response-bound existentials. Reuse that table across `subst`
  and `modulo` so repeated variables remain repeated.
- [ ] Preserve each response existential's relative universe and reject a
  response that would leak a local variable or placeholder into an inaccessible
  caller universe.
- [ ] Use each variable's lowered current universe ceiling, not immutable
  creation universe, in response metadata. Instantiating the response and
  applying its structural equalities must reproduce ceiling lowering for
  caller inputs transactionally.
- [ ] Validate the extracted boundary object contains no raw egraph
  `InferVarIndex`, version handle, or uncanonicalized binder reference before a
  candidate/frame version can be discarded.
- [ ] Open a response binder with a fresh mapping on every instantiation, then
  apply the entire response substitution to the caller in one child probe.
- [ ] Instantiate residual goals with a separate capture-avoiding push/pop
  binder remapper. Freshen every nested `Exists` declaration and occurrence;
  do not confuse response-bound variables or requester inputs with a
  numerically overlapping residual-local alpha parameter.
- [ ] Reserve shallow, non-repeating freshening for ambiguous anti-unification
  hints in Step 5. It must not be used for a definite `Yes` response.

Tests:

- [ ] Deterministic first-encounter indexing and stable canonical output.
- [ ] Repeated occurrences map to one canonical variable; distinct variables
  with the same display name remain distinct.
- [ ] Rigid caller generics cannot be bound, while flexible caller inference
  variables can be bound by the same equality or trait-head shape.
- [ ] An eclass containing flexible input `?X` and rigid placeholder `P`
  extracts `?X = P`; it never chooses `?X` as a representative which appears
  to bind or erase `P`.
- [ ] Rigid and flexible variables with the same kind and universe do not share
  a canonical identity.
- [ ] Assumptions participate in canonicalization and retain flexible inputs.
- [ ] Relative universes round-trip and higher-universe placeholders do not
  leak into lower-universe variables.
- [ ] Lowering a caller input's universe ceiling and re-canonicalizing produces
  the corresponding distinct query metadata/cache key; the earlier broader
  result is not reused.
- [ ] A closed query with no free canonical variables can return and re-open a
  response-local existential at the caller's current universe.
- [ ] The same free inputs queried at two different relative current-universe
  depths do not share a cache entry when fresh proof-local scope could matter.
- [ ] Alpha-equivalent nested `Exists` goals canonicalize identically despite
  different source `GenericParam` identities; shadowed binders neither capture
  free inputs nor collide with response parameters.
- [ ] Alpha-equivalent lifetime and const parameters embedded in otherwise
  identical types produce the same canonical query, while distinct parameters
  remain distinct.
- [ ] A result for `impl<T> Trait for (T, T)` preserves one repeated response
  variable in both tuple positions; it does not extract `(A, B)`.
- [ ] The same local variable shared between `subst` and `modulo` remains shared
  after extraction and instantiation.
- [ ] A generic head which only equates `?X` with a fresh candidate variable
  extracts an empty substitution and can be recognized as unconditional.
- [ ] Two query inputs equated through one candidate variable use the lowest
  canonical input as representative, independent of union/find and matching
  order.
- [ ] A candidate-local variable used by a residual is rewritten to an equated
  query input; a genuinely structural equality such as `?X = Vec<?T>` retains
  the constructor and one response-bound variable.
- [ ] Opening the same response twice produces fresh caller variables each time.
- [ ] A residual containing nested/shadowed `Exists` binders is instantiated
  twice without capture, including when its canonical indices overlap the
  requester's input/response index values.
- [ ] Failed response application rolls back all bindings and wake effects.
- [ ] Discarding the source branch after extraction leaves the response fully
  usable and a structural scan finds no source-branch IDs in it.

## Step 5: Answer algebra, anti-unification, and subsumption

Implement and exhaustively test result reduction before any code launches more
than one candidate. The accumulator retains:

- every `Yes` answer until final pairwise reduction;
- `saw_maybe`;
- an optional nonempty collection of binder-aware hint inputs, each containing
  response-variable metadata plus its substitution (`None` means no possible
  answer has been seen; an empty substitution inside the collection is a real,
  maximally weak hint, not an identity); and
- whether any alternative remains outstanding.

After all answers arrive, compare the complete `Yes` set pairwise and keep
those for which no strictly more-general answer exists. Do not discard answers
incrementally: the conservative recognized subsumption relation need not expose
every transitive edge, so streaming removal could make the result depend on
arrival order. Use a stable canonical tie-break for alpha-equivalent or
mutually-subsuming answers.

Finalization rules:

- empty-substitution `Yes { modulo: All([]) }` -> unconditional `Yes`
  regardless of `Maybe`, and the only permitted early return;
- all `No` -> `No`;
- one non-dominated `Yes` and no `Maybe` -> that `Yes`;
- any `Maybe` in the absence of an unconditional `Yes`, or more than one
  incomparable `Yes` -> `Maybe` with the common hints;
- one or more `Maybe` and no `Yes` -> `Maybe`, never `No`;

- [ ] Implement type-only anti-unification independently of the accumulator.
  Preserve shared outer structure, use fresh shallow witnesses where answers
  differ, and retain only substitution keys present in every possible answer.
- [ ] Carry each input's response binder through hint accumulation.
  Alpha-rename binders apart, canonically order all inputs, and perform one
  n-ary anti-unification so completion order cannot affect the result.
- [ ] Give every shallow divergent witness the relative universe of its
  substitution key, then leak-check the merged value against that key. Do not
  choose a universe from whichever alternative happens to arrive first.
- [ ] Normalize the merged hint: remove a substitution entry whose entire
  value is one otherwise-unused fresh response existential, then prune and
  deterministically renumber unused response variables. Such an entry is
  tautological and must not create a new alias/revision on every retry.
- [ ] Implement `A.subsumes(B)` directionally: assume/freeze `B` (the
  antecedent), instantiate only `A`'s response-bound existentials as flexible
  witnesses, and check that `B` implies `A`.
- [ ] Never use symmetric unification that can bind antecedent variables.
- [ ] Preserve response-variable universe ceilings during comparison.
  Consequent witnesses may be lowered as permitted, but subsumption must not
  satisfy a lower-universe condition with an inaccessible higher-universe
  placeholder or witness.
- [ ] Compare substitutions structurally, then compare canonical flattened
  residual conjunctions by subset for the conservative MVP implication check.
- [ ] Treat cases requiring trait reasoning as non-subsuming. A false negative
  may produce `Maybe`; a false positive could discard a valid alternative and
  is not allowed.
- [ ] Implement complete-set reduction and the finalization rules above.

Tests:

- [ ] All alternatives returning `Maybe` produce `Maybe`, including when every
  hint is empty.
- [ ] `Maybe` arriving before an unconditional `Yes` does not terminate search
  and the later unconditional answer wins.
- [ ] A `Yes` with a nonempty substitution and trivial residual does not
  override `Maybe`; only the exact empty-substitution unconditional form wins.
- [ ] Two early incomparable answers followed by a late answer that subsumes
  both finish as the late general `Yes`.
- [ ] Every permutation of the same `No`/`Maybe`/`Yes` multiset produces the
  same alpha-canonical result.
- [ ] All `No` produces `No`; a lone conditional `Yes` remains `Yes`; a
  conditional `Yes` plus unresolved `Maybe` produces `Maybe`.
- [ ] Empty hint versus no possible hint input exercises the optional-nonempty
  accumulator distinction.
- [ ] `?X = u32` and `?X = i32` become `Maybe` with an empty common hint; the
  anti-unified bare fresh witness is recognized as tautological and pruned.
- [ ] Shared constructors are retained and differing tuple components receive
  independent shallow hint witnesses.
- [ ] Anti-unifying values containing the highest placeholder accessible to a
  key assigns a usable witness universe; an inaccessible placeholder is
  rejected rather than hidden by freshening.
- [ ] Reapplying an empty or tautology-pruned hint does not allocate a caller
  variable, advance the semantic revision, or wake the obligation again.
- [ ] Unconditional answers subsume conditional answers; residual subset
  subsumes residual superset; binder alpha-renaming does not change the result.
- [ ] An unconstrained `X` subsumes `X = u32`, while `X = u32` does not subsume
  unconstrained `X`. The latter check must not bind/freeze the antecedent into
  making the implication appear true.
- [ ] Definite-answer repeated variables remain shared during subsumption;
  shallow hint freshening does not alter the definite response.

## Step 6: Per-query whiteboard

Implement the whiteboard as proof-tree coordination, not as a global cache.

- [ ] Create exactly one whiteboard per actual execution of the tracked
  `GoalQuery::prove` function and keep completed entries until that execution
  and all of its scopes finish. A Salsa cache hit creates no whiteboard.
- [ ] Give the query an immutable producer-arena root and put the top-level
  request in its own child version. Every frame producer gets a separate child
  of the arena root, so a live frame is never a child of the request version on
  which its response will be applied.
- [ ] Define `AtomKey` from the canonical atom, canonical environment,
  `local_crate`, and remaining depth. Key an entry by `(AtomKey, parent_frame)`.
- [ ] Deduplicate an in-progress request only when the full entry key, including
  parent frame, matches.
- [ ] Before insertion, walk the parent chain and compare canonical atom,
  environment, and crate while deliberately ignoring depth. A match is an
  inductive cycle and returns `No`.
- [ ] Return `Maybe` before spawning when the depth limit is exhausted.
- [ ] Give each frame a stable ID, explicit pending/cancelling/ready/abandoned
  state, one write-once result, subscriptions, a producer handle, and its own
  result stash.
- [ ] Allocate one subscription when each `ProofFuture` is created, update that
  subscription's real waker on every pending poll, and remove it on ready/drop.
  No subscription may remain registered after its requesting scope exits.
- [ ] Start a new frame only in the query-owned producer scope. Import the
  canonical `AtomKey` into the frame's fresh arena-root child; do not clone or
  inherit the creating requester's candidate context, variables, or version.
- [ ] Keep the producer alive when its creator is cancelled but another
  subscription remains. The producer handle belongs to the frame/query scope,
  not to any one requester.
- [ ] When the last subscription to an incomplete frame disappears, remove its
  key before requesting cancellation so a later lookup can create fresh work.
  Cancel and join all nested producer tasks, remove their subscriptions, then
  discard the frame version and mark the old frame abandoned.
- [ ] On normal completion, stash a branch-independent response, join every
  nested task, and discard the frame version before storing the ready result and
  waking subscribers. Completed entries remain reusable until query teardown.
- [ ] Make the query scope join or cancel-and-drain every producer before the
  top-level request version, runtime, egraph, or whiteboard is dropped.
  On query cancellation, cancel/join requester tasks first so their
  subscriptions drop, then drain producers and their nested subscriptions.
- [ ] Reject double completion and stale frame access with assertions in debug
  builds.

Tests:

- [ ] Same-parent duplicates share a frame; different-parent requests do not.
- [ ] Alpha-equivalent requests from sibling candidate versions share one
  canonical producer, but each applies the response through its own mapping;
  the producer cannot observe either requester's branch-local IDs or facts.
- [ ] Direct and indirect ancestor recursion are rejected even when the repeated
  request has a different depth value.
- [ ] Equal completed non-ancestor frames do not count as cycles.
- [ ] Depth exhaustion returns `Maybe` without allocating a frame.
- [ ] Multiple subscribers transition from pending to the same completed result.
- [ ] Cancelling the candidate which created a frame unregisters only its
  subscription; a surviving subscriber in another candidate still receives the
  producer's result, and the creator's version can be discarded safely.
- [ ] Cancelling one non-creator subscriber leaves the independently needed
  producer running.
- [ ] Cancelling all subscribers removes the key, cancels and joins the
  producer, unregisters nested waits, and permits a later lookup to start a new
  frame rather than observe a permanently pending entry.
- [ ] Success and cancellation both stop every task which can access the frame
  version before discarding it; success publishes/wakes only after that discard.
- [ ] Cancelling the producing query scope resolves or removes every owned
  entry and leaves no task or branch behind.
- [ ] Frame IDs and results remain stable as later frames are appended.

See the [whiteboard implementation sketch](./impl-sketches/whiteboard.md) for
the proof-frame mechanics. Reconcile the sketch with the rules above before
copying pseudocode from it.

## Step 7: Structural goals and conjunction fixpoint

Implement goal structure using a sequential conjunction fixpoint. Conjunction
concurrency is outside the MVP.

- [ ] Prove `Equals` directly with transactional `try_unify`; do not send it
  through trait candidate assembly.
- [ ] Implement `All([])`, `All`, `Exists`, `Implies`, and `Maybe`.
- [ ] Give a conjunction its own child version. Collapse it into its caller for
  a possible/successful result and discard it on `No`, so a failed conjunction
  cannot leak earlier sibling equalities.
- [ ] Make cancellation of a conjunction cancel and join its nested scoped work
  before discarding the child; collapse only after the child has no live
  descendants and is again its parent's sole live child.
- [ ] Open `Exists` variables as fresh local existentials with explicit
  universe metadata.
- [ ] Extend the environment only while evaluating `Implies`. If the inner
  proof returns a nontrivial residual `R`, return
  `Implies(original_assumptions, R)`; do not return bare `R` to an outer scope.
- [ ] Normalize/substitute each pending conjunct before attempting it. Track its
  last normalized residual and the egraph revision that affected it.
- [ ] Replace a conjunct with a returned residual and immediately mark that
  normalized residual as attempted at the post-proof semantic revision.
  Neither an unchanged residual nor an ever-changing residual chain may be
  retried at the same state/depth; only a later relevant semantic change
  enables another attempt.
- [ ] Retry stalled goals only after their normalized form or a relevant
  inference revision changes. At fixpoint, return the canonical conjunction of
  remaining residuals.
- [ ] Make `Maybe` retain the substituted goal as a residual.

Tests:

- [ ] Empty conjunction, equality success, equality failure, and failure
  rollback after an earlier successful conjunct.
- [ ] A sibling equality pins a variable and causes a previously stalled atom
  to be retried and proven.
- [ ] An atom that repeatedly returns the same conditional residual reaches a
  stable fixpoint instead of looping.
- [ ] A rule whose residual grows on every proof (`R<T>` -> `R<Vec<T>>`) is
  retained after one attempt at a revision rather than bypassing depth checks
  and looping forever.
- [ ] Equivalent residuals with different nesting/order normalize to the same
  progress key.
- [ ] An `Implies` assumption does not leak to a sibling.
- [ ] A residual that still needs an `Implies` assumption is returned wrapped
  in that same scope and succeeds when retried later.
- [ ] Nested `Exists` variables remain local and preserve sharing in an
  extracted response.
- [ ] A stable `Maybe` is not busy-polled without a relevant egraph change.

## Step 8: Environment clauses and atomic orchestration

Add the first trait candidates only after Steps 1-7 are complete.

- [ ] Build the body solver environment from the opened function parameter
  environment plus its opened trait/impl owner environment. Preserve their
  shared generic mappings and combine their eligibility into
  `assumptions_complete`; unsupported-status diagnostics remain attached.
- [ ] Include the Trait System's deduplicated elaboration of eligible local
  trait defining predicates. Keep supertrait elaboration deferred and mark an
  unavailable defining-predicate source incomplete.
- [ ] Lower direct and implication assumptions into binder-owned clauses.
- [ ] Short-circuit a `TraitImpl` atom containing `Ty::Error` to recovery
  `Yes { subst: [], modulo: All([]) }` before candidate assembly. Preserve the
  existing error sentinel/diagnostic and emit no secondary trait failure.
- [ ] Assemble matching environment clauses before considering impl clauses,
  including when the self type is a bare flexible variable.
- [ ] Open each candidate binder once in a fresh isolated egraph branch; use the
  same mapping for its head and body.
- [ ] Match the head transactionally, prove the body as a conjunction in that
  branch, extract one canonical response, and then discard the branch.
- [ ] Launch alternatives through a scoped runtime, feed completed responses to
  the Step 5 accumulator, and join every task before returning.
- [ ] On an unconditional winner, explicitly cancel and clean up siblings
  before returning the result.
- [ ] Submit nested trait atoms through the Step 6 whiteboard.
- [ ] After environment assembly, treat a still-unproven bare flexible self
  type as an incomplete impl search: skip impl enumeration and contribute an
  empty-hint `Maybe`. Step 11 derives its retry set conservatively from the
  retained obligation's existential inputs; do not invent a separate stall
  field in `QueryResult`.

Tests:

- [ ] A function generic bound and an owning trait/impl bound both become
  direct environment facts; implication assumptions also lower correctly.
- [ ] With identical known assumptions, a complete environment may return
  exhaustive `No` while an incomplete environment returns `Maybe`; a known
  unconditional environment fact still returns `Yes` in both.
- [ ] `T: LocalTrait<U>` exposes the local trait's instantiated defining
  predicates (for example `U: Bound`) without recursively looping.
- [ ] Candidate-head matching can bind flexible goal variables but cannot bind
  rigid placeholders.
- [ ] Candidate bodies return scoped residuals when not yet proven.
- [ ] Multiple matching assumptions exercise the full answer algebra.
- [ ] Non-matching trait symbols and self types are ignored.
- [ ] Error in the self type or a trait argument terminates the obligation
  without scanning candidates, binding inputs, or adding another diagnostic.
- [ ] Candidate binders are fresh per alternative and one binder variable used
  repeatedly in the head remains shared.
- [ ] Interleaved candidate completion and every completion order produce the
  same answer.
- [ ] An unconditional environment candidate cancels pending siblings without
  leaking their bindings, waits, or wakes.
- [ ] A direct `?X: Trait` environment assumption proves the identical goal
  even though `?X` is bare. Without such a proof, the atom retains `?X` as a
  retry dependency and does not scan impls speculatively.

## Step 9: Local impl clauses

This step starts only after the Trait System gates in Step 0 are complete.

- [ ] Enumerate local trait impls using `GoalQueryData.local_crate`; do not scan
  another crate or use an ambient source root.
- [ ] Start with a deterministic linear scan of the Trait System's
  `local_impls(db, LocalCrateSymbol)` result and filter by trait symbol and
  known self-type head where possible. Module traversal remains owned by that
  query.
- [ ] Convert each `ImplSignature` into one clause whose binder covers the impl
  generics in the self type, trait reference, and where-clause body.
- [ ] Expose that clause only when both the impl signature and referenced local
  trait signature are `SolverEligibility::Eligible`. A potentially matching
  `Unsupported` signature marks candidate assembly incomplete and contributes
  `Maybe`; it is not silently filtered into an exhaustive search.
- [ ] Open impl generics freshly for every candidate attempt and convert trait
  where-predicates into body goals.
- [ ] Add the referenced local trait signature's instantiated where-predicates
  to the candidate body as well. A trait-level applicability condition must not
  disappear merely because it is absent from the impl's source where-clause.
- [ ] Defer a local impl of an external trait until checked metadata exposes
  that trait's defining predicates; never assume the missing predicate set is
  empty.
- [ ] Preserve environment-first candidate assembly. Final meaning remains
  independent of enumeration and completion order.
- [ ] Keep external impl discovery, auto-trait candidates, specialization, and
  coherence out of this step.
- [ ] Permit an external trait goal to succeed from an environment assumption,
  but return `Maybe`/unsupported rather than `No` when proof would require
  deferred external impl or trait-predicate metadata. Reserve `No` for an
  exhaustive enabled candidate set.

Tests:

- [ ] Concrete and generic impl proofs.
- [ ] Generic impl where-clauses become residuals and later discharge.
- [ ] Local trait-level where-predicates become candidate-body obligations and
  cannot be bypassed by an otherwise matching impl.
- [ ] A relevant unsupported/lifetime/const-generic impl or trait signature
  cannot be opened with type variables, cannot become an unconditional clause,
  and prevents definitive `No` from the remaining candidate subset.
- [ ] A candidate such as `impl<T> Trait for (T, T)` accepts `(u32, u32)`,
  rejects `(u32, i32)`, and returns one shared witness when the caller contains
  inference.
- [ ] Candidate generics are fresh across two impl alternatives and never leak
  through a discarded branch.
- [ ] Non-matching trait and self-type heads are ignored.
- [ ] Multiple applicable impls use the tested order-independent merge policy.
- [ ] The same canonical atom queried with two `LocalCrateSymbol`s sees only the
  impls from the selected crate and has distinct salsa identity/results.
- [ ] An external-trait environment assumption proves its goal; the same goal
  without an assumption does not become definitive `No` while external
  candidate metadata is unavailable.

## Step 10: Public `GoalQuery` and end-to-end solver

Expose the solver only after its internal result and lifecycle contracts are in
place.

- [ ] Define an interned `GoalQuery` wrapping stashed `GoalQueryData`. Because
  that data contains `local_crate`, structurally identical goals in different
  crates must not share a query.
- [ ] Add the tracked synchronous `GoalQuery::prove` boundary returning a
  `Stashed<QueryResult>`.
- [ ] Validate that public query inputs are canonical and contain no caller
  egraph IDs or noncanonical binder references.
- [ ] For each execution, create a root proof context, result stash,
  explicit-version egraph, whiteboard, and runtime scope.
- [ ] Run proof search to completion, extract the response with sharing and
  universes preserved, and verify all child scopes/branches are gone before
  stashing the result.
- [ ] Provide caller helpers that canonicalize a goal and retain its reverse
  mapping, then instantiate/apply a returned response transactionally.

End-to-end tests:

- [ ] Equality-only, assumption-only, impl-only, and implication-body proofs.
- [ ] All-`Maybe`, `Maybe` before unconditional success, late general answer,
  and all-`No` alternatives through the real scheduler.
- [ ] Randomized or enumerated candidate completion orders yield the same
  canonical stashed result.
- [ ] Rigid-versus-flexible inputs and relative universes survive the complete
  caller -> query -> caller round trip.
- [ ] Repeated candidate variables remain shared in substitutions and
  residuals after stashing.
- [ ] Direct/indirect cycles and the depth cap produce the specified `No` and
  `Maybe` outcomes.
- [ ] Query crate context controls local impl discovery and salsa identity.
- [ ] Early cancellation leaves the root egraph and runtime quiescent.

## Step 11: Obligation manager and mandatory final discharge

Integrate with body checking through an explicit obligation lifecycle instead
of storing bare residual goals.

- [ ] Add an obligation record containing the canonical goal and environment,
  `LocalCrateSymbol`, canonical-to-caller mapping, provenance/span, current
  state, stalled-on variables, and the inference revision last attempted.
- [ ] Expose ordinary and transaction-local obligation submission APIs. Lower
  each represented type predicate to a fixed `TraitImpl` goal and allow a
  consumer to publish a staged batch only after its inference transaction
  commits; never treat an ineligible `CheckedParameterEnv` as empty.
- [ ] Wire the body checker's ordinary free-function calls and ADT
  construction/use to those APIs. Method Resolution Steps 4/7 own wiring the
  selected method-function predicates through the same staging contract.
- [ ] Deduplicate obligations by canonical goal, environment, crate, and caller
  mapping while retaining all diagnostic provenance that matters.
- [ ] On `Yes`, apply the substitution atomically and enqueue nontrivial
  `modulo` under its preserved environment and mapping. Initialize its
  last-attempted dependency revision to the current post-application revision,
  so a changed residual is not immediately re-proved without new information.
- [ ] On `Maybe`, apply only its necessary hints in one transaction, retain the
  original obligation, and register wakes for the flexible variables on which
  it is stalled. Because a hard hint is common to every still-possible answer,
  a conflict disproves the original goal without leaving partial state.
- [ ] For the MVP, derive the conservative stalled-on set from every
  `ExistentialInput` occurring in the retained goal and environment. A later
  result field may narrow this set as an optimization.
- [ ] On `No`, record a trait-obligation failure at the original provenance.
- [ ] Recanonicalize and retry an obligation only when a relevant variable or
  egraph revision changes. Repeated processing without a change must be
  idempotent.
- [ ] Replace one-shot `block_on`/finalize ordering with a body-completion state
  machine. Keep the root scope open and jointly drain runnable expression and
  constraint tasks, committed wakes, and ready obligations until stable
  quiescence; do not require a task suspended in `await_concrete` to complete
  before obligations or recovery run.
- [ ] At stable quiescence, finalize unresolved variables to their final bound
  or `Ty::Error`, publish the resulting wakes, and resume the joint drain.
  Repeat because resumed source tasks may allocate variables or obligations.
- [ ] When all variables are final but stable obligations or lookup waits
  remain, run their terminal proof/diagnostic pass and wake waiting expression
  tasks with error recovery before requiring the root scope to join. Treat a
  pending task with no registered variable/obligation wait as an internal
  deadlock. Finish only when every scope has joined and every obligation is
  terminal.
- [ ] At final fixpoint, turn every remaining `Maybe` or nontrivial residual
  into a diagnostic/error result with its provenance. No unresolved obligation
  may disappear because the runtime became quiescent.

Tests:

- [ ] A deferred obligation converges after later type information and updates
  the caller egraph.
- [ ] A generic free-function call and generic ADT use each submit their
  instantiated declared bounds; violating either fails even when
  argument/field types otherwise match.
- [ ] A transaction-local fixture proves staged obligations are invisible
  before commit and discarded on rollback; Method Resolution tests the real
  selected-method producer.
- [ ] Residuals retain their canonical-to-caller mapping and `Implies`
  environment across multiple retries.
- [ ] Two equivalent submissions deduplicate while diagnostics retain useful
  provenance.
- [ ] Unrelated inference changes do not re-run a stalled obligation; a
  relevant wake does.
- [ ] Re-proving without an egraph change is idempotent and terminates.
- [ ] Re-enqueuing an ever-changing residual at the same dependency revision
  does not start a new proof chain; a genuine relevant revision permits retry.
- [ ] Conflicting `Yes` substitution or `Maybe` hint application rolls back.
- [ ] Finalization discharges newly enabled residuals before diagnosing the
  remainder.
- [ ] A body task suspended on a variable whose only resolution is fallback
  finalization wakes, finishes its remaining source traversal, and allows any
  newly submitted obligations to run before body completion.
- [ ] A body task awaiting a permanently unsupported/ambiguous obligation with
  no inference dependencies receives its final diagnostic outcome, wakes, and
  joins instead of leaving the runtime quiescent-pending forever.
- [ ] Permanently ambiguous, depth-limited, and unprovable obligations produce
  diagnostics rather than silently completing body checking.
- [ ] Finalization leaves no live solver task, whiteboard waiter, branch, or
  obligation record in a pending state.

## Step 12: Documentation and verification

Documentation is part of every implementation step; this final step is the
cross-check before declaring the RFD complete.

- [ ] Tick landed steps in this file as they land.
- [ ] Keep the trait-solving README and both implementation sketches consistent
  with the implemented IR, version lifecycle, whiteboard key, answer algebra,
  and obligation lifecycle.
- [ ] Update the relevant pages under `md/design/` when the solver, runtime, or
  checking pipeline becomes part of the intended architecture.
- [ ] Add or revise a decision in `md/design/decisions.md` if implementation
  changes a cross-cutting, load-bearing decision.
- [ ] Update RFD status/navigation and `md/implementation/roadmap.md` only at
  the lifecycle transitions required by
  [Maintaining This Book](../../contributing/maintaining-the-docs.md).
- [ ] Run `cargo fmt` after every Rust edit.
- [ ] Run focused tests for each step, then the full relevant test suite and
  `cargo build`.
- [ ] Run `mdbook build` with link checking available and inspect the rendered
  trait-solving pages and internal links.
- [ ] Confirm the final implementation has tests for rollback, occurs/leak
  checks, branch interleaving/cancellation, answer order independence,
  canonical roles/universes, response sharing, residual scoping, crate context,
  and mandatory final discharge.

## Owned by other RFDs

Method lookup and trait-method dispatch are not implementation steps for this
RFD. The [Method and Trait Resolution RFD](../method-resolution/README.md)
owns `(TraitSymbol, FnSymbol)` enumeration and submits fixed-trait goals to the
solver. The MVP trait solver proves propositions; it neither quantifies over an
unknown trait symbol nor returns a selected impl.

## Deferred beyond MVP

- [ ] Solver-backed subsumption instead of residual-set subset approximation.
- [ ] Associated type representation, `NormalizesTo`, and alias relation.
- [ ] Hypothetical equality assumptions with a scoped egraph environment.
- [ ] `ForAll`, higher-ranked lifetimes, and meaningful outlives proving.
- [ ] Structural auto-trait candidates and coinductive cycle handling.
- [ ] External-crate impl discovery.
- [ ] Coherence, overlap, negative reasoning, and specialization.
- [ ] Cross-query/global solver caching.
- [ ] Impl indexing by trait and self-type head.
- [ ] Concurrent conjunction solving.
