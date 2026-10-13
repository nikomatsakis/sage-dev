# Architectural Contracts

Rust correspondence connects the `.sage` model to source declarations. Runtime
sessions use that correspondence to check observed architectural operations.
These rules complement the language's
[declared-call](language.md#meta.language.calls.declared) and
[transitive-prohibition](language.md#meta.language.calls.forbidden)
rules; the [RFD](../../rfds/sage-contracts/README.md) records the rationale and
rollout.

## Source correspondence

{anchor}`correspondence`
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

{anchor}`validated_session`
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

{anchor}`cache_invocations`
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

{anchor}`observation_coverage`
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

## Implementation and test evidence

The generated [anchor pages](../anchor/meta/contracts.html) report implementation entries,
annotated tests, explicit TODO/N/A gaps, and saved claim/test reviews. These
associations do not themselves establish that a test ran.

The [earlier coverage snapshot](../../rfds/implementation-sequencing/coverage-history.md#architectural-contracts)
preserves the former capability descriptions, edit experiments, and limitations
as historical evidence. Current coverage comes from the generated entries;
implementation plans and decision rationale belong in the relevant RFDs.
