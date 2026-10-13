# Architectural Contracts

Rust correspondence connects the `.sage` model to source declarations. Runtime
sessions use that correspondence to check observed architectural operations.
These rules complement the language's
[declared-call](../validation/architecture-language.md#meta.calls.declared) and
[transitive-prohibition](../validation/architecture-language.md#meta.calls.forbidden)
rules; the [RFD](../../rfds/sage-contracts/README.md) records the rationale and
rollout.

## Source correspondence

{anchor}`meta.contracts.correspondence`
Every Sage correspondence unit has one or more Rust locations declared through
`#[sage::item(...)]`. Every such annotation resolves to a compatible Sage unit
in its containing modeled crate. Crate and module headers, named declarations,
and explicit methods are correspondence units. Multiple source locations are
permitted; module markers provide source coverage but do not establish runtime
execution coverage. Cargo package and target metadata determine Rust ownership.

Verification exercises missing locations, dangling annotations, wrong crates,
incompatible operation kinds, multiple sites, abstract markers, Cargo library
and binary roots, and declarations produced by supported generators. Arbitrary
macro tokens do not establish a generated declaration's existence.

## Validated sessions

{anchor}`meta.contracts.validated_session`
A checking session uses one immutable, validated contract model and its source
correspondence index. Every event refers to that same snapshot. Enabled
enforcement fails if its model is missing or invalid, or if the indexed Rust
source cannot be paired with the running binary. Loading is cached for the
session; source edits take effect only in a subsequent session.

Verification exercises cached loads, parallel first use, explicit model roots,
missing and invalid files, source/build mismatch rejection, and a subsequent
session seeing model edits. Independently configured fixture sessions can use
different models without modifying an active session.

## Invocations and caching

{anchor}`meta.contracts.cache_invocations`
Every annotated source invocation is checked, including query cache hits and
same-operation recursion. A Salsa request and its execution represent one
architectural invocation. Internal validation, dependency reexecution, and
accumulator traversal remain distinguishable from source requests; reexecution
establishes the executing operation's context without attributing its work to
the suspended caller or inventing a source call.

Verification covers cold, reused, validated, invalidated/reexecuted, recursive,
cancelled, and panicking runs, calls through generated query APIs, and an
unannotated query beneath an annotated caller. Hand-reviewed traversals identify
the architectural operations, Rust declaration sites, parentage, and cache
outcomes. A warm forbidden request must fail even when its body does not execute.

## Observation coverage

{anchor}`meta.contracts.observation_coverage`
Incomplete observation cannot establish conformance. Unmapped requests,
unsupported receiver checks, and uninstrumented effects remain explicit gaps.
Strict acceptance requires complete checking of the declared scope, as well as
absence of violations. Diagnostics identify the checking scenario, relevant
model rule when one exists, and observed call path; an unknown mapping is not
silently attributed to the caller.

Verification injects each gap into an otherwise passing scenario and requires
an incomplete or failing result, including cached requests and unsupported
async boundaries. A call-only report does not claim that input reads, context
construction, ordinary uninstrumented helpers, or task propagation were checked.

## Current status

- `CorrespondenceIndex` implements bidirectional, crate-correct source checking.
  `correspondence.rs` tests cover Cargo ownership, multiple sites, abstract
  markers, supported generator inputs, unsupported placements, and source
  snapshot mismatch. Source inventory preserves cfg alternatives without
  asserting which declarations exist in the running binary.
- `ContractModel` rejects inconsistent models; `ContractSession` retains an
  immutable shared model. `SalsaObserver` verifies supplied compiled source
  snapshots and refuses to report conformance for observed unpaired files.
  Automatic runtime discovery, cached first-use loading, parallel initialization,
  and verification of model edits across subsequent loads remain **TODO**.
- The isolated real Salsa fixture checks cold, warm, validated/reexecuted,
  generated-API, forbidden, unmapped, cancellation, and panic behavior. Runtime
  engine tests check recursion and receiver-sensitive permissions. Ordinary
  function instrumentation and enforcement throughout compiler tests remain
  **TODO**; an inert annotation does not itself emit runtime events.
- Reports reject observed mapping, lifecycle, and receiver gaps and retain
  unchecked `reads`/`creates` effects separately. Current success is limited to
  supplied operation boundaries: automatic discovery of missing ordinary-call
  boundaries, effect hooks, async/task propagation, and comprehensive
  scenario-aware diagnostics remain **TODO**.

Existing behavioral tests carry references to the rules they exercise.
Additional implementation/test entries in `tests/contracts_plan.rs::todo`
record the unfinished portions explicitly; their passing placeholder tests do
not establish those behaviors. The
[implementation plan](../../rfds/sage-contracts/implementation.md) tracks the
remaining compiler rollout and broader evidence.
