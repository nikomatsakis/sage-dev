# Implementation plan and status

The general RFD remains incomplete. The `DbDropGuard::db` vertical slice has a
deliberately conservative external trait-method path: current-module and
all-supported-edition standard-prelude trait enumeration (with only the
intersection treated as definitely in scope), name-based
associated-item discovery, a read-only fixed-trait proof, Self-only signature
instantiation, and explicit shared-reference receiver elaboration. A source
completeness audit and a local-ADT inherent-provider scan prevent this partial
tier from selecting when omitted work could take priority or compete. Missing
metadata, imports, macros, attributes, inherent providers, additional method
type parameters, and competing possible candidates remain unknown rather than
being discarded. The checklist below still describes the complete design.
The next planned integration slice, `Parse::next`, is tracked by the
[Associated Type Normalization
RFD](../associated-type-normalization/README.md); that RFD owns its projection,
external impl, generic-default, and external inherent-method requirements.

### Step 1: Method candidate and result types

- [ ] Preserve local item `visibility_modifier` syntax in CST/symbol data and
  add checked `Visibility` plus `is_visible_from` for functions and traits,
  covering private, `pub`, `pub(crate)`, `pub(super)`, and `pub(in path)`.
- [ ] Add `MethodOrigin`, `ResolvedMethod`, `MethodResolution`, receiver-adjustment, and
  internal candidate types.
- [ ] Include `MethodResolution::Error(ErrorReported)` as a propagation state,
  distinct from lookup `NotFound`.
- [ ] Depend on the Trait System's checked receiver representation; never infer
  method-ness or receiver form from a `Ty::Infer` first parameter.
- [ ] Represent ambiguity and deferred lookup separately from not-found.
- [ ] Represent candidate evaluation as definite `Yes`, conditional `Yes`,
  unknown (`Maybe` or unsupported metadata), or `No`; do not apply candidate
  responses during classification.
- [ ] Retain a `NeedsMoreInfo` lookup as a body-checker lookup obligation keyed
  by the original receiver/name/scope and relevant inference dependencies; do
  not publish a competing candidate's residuals before selection.
- [ ] Add tests for deterministic candidate ordering and result-state transitions.
- [ ] Exhaustively test the resolution table: all `No`; a sole definite or
  conditional candidate; definite plus unknown/conditional; multiple
  conditionals; conditional plus unknown; multiple definite candidates; and
  non-exhaustive metadata.

### Step 2: Local inherent method discovery

- [ ] Filter `local_impls(db, LocalCrateSymbol)` to inherent impls with a matching
  self-type head and `SolverEligibility::Eligible`.
- [ ] Retain a potentially matching `Unsupported` inherent impl as an incomplete
  candidate source. In particular, do not try to open lifetime/const-generic
  impls with type inference variables and do not conclude `NotFound` from the
  remaining subset.
- [ ] Classify receiver heads as local provider, structurally no provider, or
  deferred external/builtin provider. The local scan is exhaustive for the
  first, `&T`/`&mut T` wrappers are empty and may proceed to built-in deref, and
  the last is incomplete while item metadata is unavailable.
- [ ] Load impl items and select functions by name.
- [ ] For dot calls, retain only functions with a supported `CheckedReceiver`;
  exclude associated functions and unsupported typed/explicit-lifetime
  receivers before applicability testing.
- [ ] Require `method_candidate_eligibility == Eligible`. A same-name function
  with lifetime/const method generics, an incomplete own parameter environment,
  or a deferred projection/signature form is unknown/incomplete, not `Found`
  from its represented subset.
- [ ] Filter inherent functions through `is_visible_from` using the call's
  lookup module. Inaccessible same-name functions do not contribute
  viability/ambiguity, but may support a targeted not-visible diagnostic.
- [ ] Test multiple impl blocks, nested modules, macro-produced impls, nonmatching heads,
  and duplicate applicable methods.
- [ ] Test a private method from the defining module and an inaccessible sibling
  module, including the diagnostic when it is the only name match.
- [ ] Test `self`, `mut self`, `&self`, and `&mut self` candidates, exclusion of
  a same-name associated function, and deferral of a typed receiver.
- [ ] Test unsupported method generic kinds, own where-predicates, and
  projection-bearing signatures contribute unknown even when the owner
  impl itself is eligible.
- [ ] Test that an external receiver head with unavailable inherent metadata is
  unknown/unsupported at the priority tier, not `NotFound` or a local-trait
  `Found`.
- [ ] Test that direct lookup on `&Local` is exhaustively empty at the reference
  wrapper and proceeds to the one built-in deref step instead of being blocked
  as an unknown external provider.

### Step 3: Transactional inherent matching and instantiation

- [ ] Open each impl's single binder freshly in an inference probe.
- [ ] Make every caller-egraph exploratory/final probe synchronous and
  non-suspending. Await receiver/argument information before branching and
  never retain a child version across `.await` while its caller parent is
  frozen.
- [ ] Match the full impl self type against the receiver and retain the same substitution
  for impl where-clauses and the method signature.
- [ ] Before solver integration, defer candidates with nonempty impl
  where-clauses or method `CheckedParameterEnv` predicates rather than treating
  either set as proven.
- [ ] Retain a branch-independent recipe/response for the unique candidate and
  discard every exploratory probe; final commit occurs with call compatibility
  in Step 4.
- [ ] Test generic impls, repeated impl generics, unsupported where-clause deferral, and
  rollback after partial unification failure.
- [ ] Instrument a probe to assert it owns no suspension/wait registration and
  leaves the caller version writable immediately after extraction/discard.

### Step 4: Receiver adjustments and body-checker integration

- [ ] Generate the canonical and one built-in-dereferenced `LookupSelfTy`
  sequence. Match impl/trait applicability on that type, then derive the
  permitted value/shared/mutable autoref from the candidate's checked receiver
  in deterministic order.
- [ ] Await a bare receiver inference variable and retry when its bound changes.
- [ ] If receiver normalization produces `Ty::Error`, propagate its existing
  `ErrorReported` into recovery call IR without candidate discovery or a
  secondary method diagnostic.
- [ ] Wire unique inherent methods into method-call argument and result checking.
- [ ] Re-open the selected inherent candidate in one transaction which covers
  its impl substitution, predicate-free signature instantiation, and
  argument/receiver/result compatibility. This pre-solver step accepts only
  the empty predicate sets gated in Step 3; Step 6 extends the same transaction
  with solver responses and staged obligations.
- [ ] Test owned, `&self`, `&mut self`, reference receiver, unknown receiver, not-found, and
  ambiguous calls.
- [ ] Test a receiver finalized to `Ty::Error` terminates lookup immediately and
  produces exactly the original diagnostic.
- [ ] Test that a late argument or result mismatch rolls back impl/method
  variables and wakeups and does not fall through to a losing method
  candidate.

### Step 5: Trait method-name discovery

- [ ] Enumerate explicit-bound traits and local traits visible in the lookup scope.
- [ ] Apply checked trait visibility from the lookup module independently of
  the still-deferred complete import/prelude candidate enumeration.
- [ ] Load trait items and retain fixed `(TraitSymbol, FnSymbol)` candidates matching the
  requested name.
- [ ] Union and deduplicate those pairs across explicit-bound, repeated-bound,
  import, and visible-local discovery paths before opening trait arguments or
  counting ambiguity.
- [ ] Apply the same checked-receiver gate as inherent dot-call lookup; a
  same-name trait associated function is not a method candidate.
- [ ] Apply the function's method-candidate eligibility independently of its
  trait owner's eligibility; partial method signatures are unknown.
- [ ] Open only `Eligible` local trait signatures. Track a relevant
  `Unsupported` signature as an unknown/incomplete candidate source rather
  than silently omitting it.
- [ ] Keep an explicit external-trait bound as an unknown name source when its
  item metadata is unavailable; do not silently omit it because local trait
  discovery cannot load its functions.
- [ ] Do not scan impls or construct an existential `?Trait` solver goal in this step.
- [ ] Test bound-provided methods, out-of-scope traits, same-name methods on multiple
  traits, and traits without the method.
- [ ] Test that the same trait item discovered from an explicit bound and local
  visibility is evaluated once, while genuinely different trait/function pairs
  remain distinct.

### Step 6: Fixed-trait solver integration

- [ ] For each trait candidate, open type arguments in an isolated probe and submit the
  fixed `LookupSelfTy: Trait<Args...>` goal, where `LookupSelfTy` is after the
  current built-in deref step but before method-specific autoref.
- [ ] Keep solver substitutions and residuals isolated until the outcome table
  selects exactly one candidate.
- [ ] Treat `Maybe` as unknown/`NeedsMoreInfo` and never expect the solver to
  return an impl or trait identity.
- [ ] Retain the unique response and residual for Step 7; do not mutate caller
  inference or the obligation store during candidate classification.
- [ ] Submit instantiated inherent-impl where-clauses through the same fixed-trait solver
  boundary before committing an inherent candidate.
- [ ] Extend the selected inherent-method transaction to instantiate and submit
  the method function's own parameter-environment predicates after
  method-generic substitution, staging all residuals until commit.
- [ ] Test environment-assumption proofs, local impl proofs, conditional proofs, multiple
  viable traits, solver ambiguity, and losing-candidate rollback.
- [ ] `s.foo()` selecting `fn foo(&self)` proves/matches impls for `S`, then
  records/checks an `&S` autoref; it never asks the solver for `&S: Trait`.
- [ ] Test an inherent impl/method predicate response followed by a late call
  mismatch; solver effects, wakeups, and staged obligations all roll back.
- [ ] In a default trait method body, the synthesized owner fact proves the
  fixed `Self: ThisTrait` goal needed to call another method from that trait;
  no recursive impl search or selected-impl evidence is required.

### Step 7: Trait method signature instantiation

- [ ] Import the selected function signature from the trait item.
- [ ] Substitute `Self` and trait arguments from the retained proof response.
- [ ] Instantiate method-level generics independently and check call arguments.
- [ ] Perform response application, signature instantiation,
  the method function's instantiated parameter-environment predicates,
  receiver/argument/result checking, and residual-obligation staging in one
  selected-method child transaction. Commit the egraph child before publishing
  its staged obligations; discard both on any failure.
- [ ] Test generic receivers, generic traits, explicit bounds, and final failure when a
  registered residual obligation cannot be discharged.
- [ ] Test a method-level `where U: Bound` predicate after method-generic
  substitution; it is staged on success and absent after rollback.
- [ ] Test rollback on a late argument/result mismatch and verify that no
  selected-candidate substitution, fresh method variable, wake, or residual
  obligation escapes.

### Step 8: Qualified and associated calls

- [ ] Resolve visible local inherent `Type::function` calls for functions with
  no checked receiver. Reuse impl-head classification/ambiguity and prove the
  selected impl predicates.
- [ ] Instantiate associated-function generics and its own parameter
  environment, check arguments/results, and stage obligations in one selected
  call transaction with full late-failure rollback.
- [ ] Return unsupported/unknown rather than exhaustive `NotFound` for an
  external type whose inherent associated-item metadata is unavailable.
- [ ] Add receiver-less fixed-trait `<Type as Trait>::function` lookup after
  qualified paths are lowered, including fixed-trait proof, signature
  instantiation, visibility, eligibility, and the same transaction.
- [ ] Preserve ambiguity and visibility diagnostics instead of selecting by source order.
- [ ] Test impl/function predicates, generics, ambiguity, inaccessible items,
  external incompleteness, argument/result mismatch rollback, and exclusion of
  receiver-taking functions until full UFCS.

### Deferred beyond the vertical slice

- [ ] General external and builtin method providers beyond represented trait
  items and structural `Sized`.
- [ ] Lifetime/const-generic method candidates.
- [ ] Complete trait visibility and prelude behavior.
- [ ] Custom `Deref` and associated type normalization.
- [ ] Full UFCS and method turbofish behavior.
- [ ] Specialization-aware method priority.
