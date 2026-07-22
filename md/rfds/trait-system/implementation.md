# Implementation plan and status

Each step leaves the workspace building and adds focused tests.

### Step 1: Checked trait data types

- [x] Add type-only `TraitRef`, `WherePredicate`, `SolverEligibility`,
  `TraitSignatureData`, `ImplSignatureData`, `TraitItemDef`,
  `MethodReceiver`, `CheckedReceiver`, and their binder-wrapped aliases.
- [x] Extend the CST/parser before checked lowering so it preserves trait
  supertraits, `auto`/`unsafe`, trait generic defaults, impl
  negative/default/unsafe/const markers, and the exact receiver/associated-fn
  distinction. Eligibility must not infer absence from syntax the CST dropped.
- [x] Add stash allocation, copying, hashing, folding, and display support.
- [ ] Test that trait arguments preserve order and that unsupported const and
  associated-type syntax is reported rather than omitted.
- [ ] Round-trip or structurally inspect every preserved header/receiver marker
  so token emission and checked eligibility see the same source facts.

### Step 2: Trait signature lowering

- [x] Implement `LocalTraitSym::sig` as the sole minting point for trait generics.
- [x] Synthesize the trait's `Self` type parameter, store it as `self_param`, and place it
  before the explicit type parameters in the signature binder.
- [x] Set `solver_eligibility` to `Eligible` only after every defining
  predicate is represented and every source generic is a supported type or
  `Dummy` lifetime parameter. Mark const-generic or otherwise unsupported
  trait signatures `Unsupported` without silently dropping their syntax.
- [x] Mark auto-trait declarations `Unsupported`; coinductive structural
  candidates are not ordinary positive impl clauses.
- [x] Mark supertrait syntax and trait type-parameter defaults `Unsupported`
  until elaboration/default substitution are implemented; neither may appear
  as an eligible empty/short argument contract.
- [x] Lower generic-parameter trait bounds and `where` predicates into the same
  `WherePredicate` representation.
- [x] Validate exact explicit type-argument arity for every eligible `TraitRef`
  (with `Self` excluded). Trait defaults are deferred, so missing/extra args
  are diagnosed rather than truncated, freshened, or silently defaulted.
- [x] Reuse existing `CheckGenerics`, resolver ribs, and the two-stash checking pattern.
- [ ] Test multiple bounds, nested type arguments, unknown traits, and stable generic
  identities across repeated query execution.
- [ ] Test that `TraitRef::args` excludes `Self` while trait where-clauses and item
  signatures reuse the recorded `self_param`.
- [ ] Test zero/exact/missing/extra trait arguments in bounds and impl headers;
  only the exact no-default form becomes eligible.
- [ ] Test that an unsupported defining predicate or const trait
  generic makes the signature ineligible and cannot be observed as an empty,
  complete predicate set.

### Step 3: Impl signature lowering with one binder

- [x] Implement `LocalImplSym::sig` returning one `Binder<ImplSignatureData>`.
- [x] Resolve the optional trait path, self type, and every where-clause under one shared
  generic substitution.
- [x] Distinguish inherent impls (`trait_ref: None`) from trait impls.
- [x] Mark an impl `Eligible` only when its complete header and applicability
  conditions can be opened by the type-only consumers. Const-generic impls and
  impls with unsupported predicates remain represented but are
  ineligible candidates.
- [x] Lower explicit, elided, and `'static` reference lifetimes to
  `Lifetime::Dummy`; skip lifetime binders during candidate instantiation and
  treat lifetime predicates as trivially true.
- [x] Recognize and gate declaration polarity/kind while lowering: negative,
  const, and default/specializing impls are `Unsupported` and can never be
  reinterpreted as ordinary positive clauses. A separate checked polarity enum
  is deferred because consumers cannot open ineligible signatures.
- [ ] Test `impl<T: Clone> Trait<T> for Pair<T, T>` and assert that all occurrences of
  `T` have the same identity.
- [ ] Test that a failed or unsupported predicate prevents the impl from being exposed as
  an unconditional solver clause.
- [x] Test that lifetime-generic trait and inherent impls are opened without
  creating lifetime inference variables; const-generic forms remain ineligible.
- [x] Test that `impl<T> Trait for &T`, an explicit lifetime, and a `'static`
  spelling all use `Dummy` without making the candidate source incomplete.
- [ ] Test that negative/const/default impl syntax never enters the positive
  local impl candidate stream; test the same separately for an auto-trait
  declaration.

### Step 4: Function and ADT parameter environments

- [x] Add reusable `CheckedParameterEnv { where_clauses,
  solver_eligibility }` data and embed it under the existing function and ADT
  signature binders.
- [x] Use the same lowering helper for generic-parameter bounds and explicit
  `where` predicates on functions, structs, and enums; do not leave the Symbol
  Signatures where-clause slots permanently empty.
- [ ] Expose a data-only helper which opens a function's predicates plus its
  trait/impl owner predicates with one owner-generic substitution. It returns
  `WherePredicate` facts and eligibility; it does not invoke the solver.
- [ ] Have that helper include the instantiated owner fact
  `Self: ThisTrait<Args>` for a trait/default method or the opened impl-head
  fact for a trait-impl method. Inherent impl bodies add no synthetic trait
  fact.
- [x] Expand each eligible local-trait fact into its instantiated defining
  `WherePredicate`s using a deduplicating data worklist to a fixed point. Do not
  perform deferred supertrait elaboration or invoke proof search, and do not use
  a partial external/unsupported defining-predicate set.
- [x] Retain ADT predicates for well-formedness obligations at construction/use
  sites even if that obligation integration lands with body checking.
- [ ] Mark an owner unsupported when any required const,
  higher-ranked, projection, or otherwise unrepresented predicate remains, and
  prevent the MVP from finalizing that owner as successfully checked.

Tests:

- [ ] `fn f<T: Clone>(x: T)` and the equivalent explicit `where T: Clone`
  produce the same body solver assumption with the original `T` identity.
- [ ] The opened data for a generic method contains both function and owner
  predicates without reminting either binder.
- [ ] The opened data for a default trait method contains its own
  `Self: Trait` fact; a trait-impl method contains the analogous opened head,
  while an inherent method does not.
- [x] Given `trait Tr<U> where U: Bound`, a body assumption `T: Tr<U>` also
  exposes `U: Bound`; recursive defining predicates terminate after
  deduplication and supertraits are not synthesized.
- [x] Struct/enum predicates remain attached to the ADT signature and are not
  lost when fields/variants are queried separately.
- [ ] An unsupported non-type predicate is diagnosed and cannot masquerade as
  a complete empty parameter environment.

### Step 5: Trait and impl items

- [x] Give local function items a stable owner relation to their trait or impl so signature
  checking can distinguish owner generics from method-level generics.
- [x] Implement `LocalTraitSym::items` and `LocalImplSym::items`.
- [x] Open the owner signature's binder and reuse its generic parameters while checking
  function, type, and const item identities.
- [x] Bind the trait signature's recorded `Self` parameter while checking trait items.
- [x] Bind `Self` to the opened, substituted `ImplSignatureData::self_ty` while
  checking impl items; impl signatures do not have a trait `self_param`.
- [x] Lower `self`, `mut self`, `&self`, and `&mut self` to a separate
  `CheckedReceiver` using the opened owner `Self` type. Preserve the absence of
  a receiver for associated functions and erase explicit receiver lifetimes to
  `Dummy`.
- [ ] Diagnose/defer typed receivers instead of admitting them as method
  candidates or lowering them to `Ty::Infer`.
- [ ] Compute `method_candidate_eligibility` for every trait/impl function.
  Require a supported receiver/associated form, type-instantiable method
  generics, a complete function parameter environment, and projection-free
  supported parameter/return types.
- [ ] Test that impl generics and method-level generics occupy distinct scopes and that
  item queries do not remint owner generics.
- [ ] Test method signatures that reference `Self`, an impl generic, and a method generic
  in the same type.
- [ ] Test `Self` independently in a trait method and an impl method, including
  a generic impl whose opened self type contains its owner generic.
- [ ] Test all four supported receiver forms, an associated function, an
  explicit-lifetime reference receiver, and a rejected typed receiver.
  Dot-call consumers can distinguish them without guessing from an inferred
  first parameter.
- [ ] Test lifetime method generics use `Dummy`; const method generics,
  unsupported method predicates, and
  associated-type/projection occurrences make only the method-consumer view
  ineligible; none can be exposed as a partially checked candidate.

### Step 6: Local impl enumeration

- [x] Implement `local_impls(db, LocalCrateSymbol)` by recursively walking expanded local
  modules.
- [ ] Include inline modules, file modules, and macro-produced impls exactly once in
  deterministic order.
- [ ] Add tests for nested modules, multiple impls for one trait, inherent impls, and
  macro-expanded impls.
- [x] Keep consumer filtering as a linear scan for the MVP.

### Step 7: Consumer boundaries

- [ ] Expose helpers that open an impl binder freshly for one solver or method-resolution
  candidate.
- [x] Verify separate candidates never share inference variables while repeated uses of
  one impl generic remain correlated within a candidate.
- [x] Add integration fixtures consumed by the trait-solving and method-resolution RFDs;
  do not implement proof search or method selection in this RFD.
- [x] Expose to the MVP solver only impls whose trait signature and defining
  predicates are available and whose impl and trait eligibility markers are
  both `Eligible`. Keep local impls of external traits out until the external
  metadata boundary supplies those predicates.
- [x] Treat a relevant `Unsupported` signature as an incomplete candidate
  source, never as an eligible signature with an empty predicate set.
- [ ] Give method resolution the same eligibility gate for type-only inherent
  and trait openings.

### Deferred beyond the MVP

- [ ] External and builtin impl enumeration.
- [ ] Meaningful lifetime semantics, const predicates, and higher-ranked predicates.
- [ ] Const-generic trait and impl candidate exposure.
- [ ] Associated type and associated const definitions.
- [ ] Supertrait elaboration.
- [ ] Coherence, overlap, negative impls, auto traits, and specialization.
- [ ] Impl indexes by trait and self-type head.
