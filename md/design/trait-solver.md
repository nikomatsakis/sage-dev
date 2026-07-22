# Trait Solver Design

For a code-first introduction, read [A direct trait
obligation](./examples/direct-trait-obligation.md) and then [A nested trait
proof](./examples/nested-trait-proof.md). This page specifies the semantic and
architectural contract those examples illustrate.

This page records the destination-level semantic contract for trait solving.
The existing positive, type-only implementation is described by the
[Trait Solving RFD](../rfds/trait-solving/README.md). Search scheduling,
recursive proof semantics, and incremental progress publication are planned
extensions with separate draft RFDs.

## Tenets

- **Soundness takes priority over inference power.** The solver may decline to
  infer a type, but it may not publish a false proof, substitution, hard hint,
  or negative result within the represented type-and-trait domain. Lifetimes
  are the explicit temporary exception described below.
- **Completeness depends on groundness.** A non-ground query terminates soundly
  but may return ambiguity despite having a valid answer. A ground query is
  sound and complete modulo explicit resource exhaustion.
- **Resource exhaustion is explicit.** Ambiguity, proof-depth overflow,
  term-size overflow, work exhaustion, and fixpoint-iteration exhaustion are
  distinguishable outcomes. Exhaustion never becomes `No`.
- **Answers and progress have dual meanings.** A conditional `Yes` supplies a
  sufficient condition for the goal. A progress envelope supplies a necessary
  condition shared by every result still possible.
- **Solver operations have semantic outputs.** Proving a proposition produces
  `Proven`; normalization produces a type. The response representation is
  extensible to later non-type operations without encoding their outputs as
  caller-supplied inference variables.
- **`No` requires exhaustive failure.** Failure to discover a useful
  instantiation is ambiguity, not evidence that no instantiation exists.
- **Results are schedule-invariant.** For a fixed canonical query, program,
  environment, and solver-limit configuration, every valid polling schedule
  produces the same canonical return value.
- **Futures represent suspended proof continuations.** Scheduling policy may
  change without reifying the entire proof continuation as a separate
  first-order state machine.
- **Compatibility permits different inference boundaries.** Sage aims to
  accept Rust trait relationships and remain broadly compatible with rustc,
  but may require more or fewer source type annotations in particular
  non-ground cases.

## Semantic contract

### Temporary lifetime boundary

Every lifetime currently lowers to `Lifetime::Dummy`, and
`Outlives(Dummy, Dummy)` succeeds unconditionally. Borrow validity and
meaningful lifetime predicates are outside the represented domain. This is a
known temporary soundness hole recorded by D12, not solver ambiguity.

The exception is narrow: it does not permit choosing an otherwise ambiguous
type or trait candidate. A future unified type-and-lifetime inference design
will remove `Dummy` and extend the solver contract.

### Alias types and normalization

The type model distinguishes non-normalizable [rigid types from alias
types](./typed-ir.md#rigid-and-alias-types). Named type aliases,
associated-type projections, and opaque types are three variants of the same
alias concept. They retain definition identity and arguments even when they
cannot or need not be normalized. That structural representation is built and
survives inference and canonical solver boundaries; the normalization
operations described next remain planned.

The solver's destination operation language includes input-only normalization,
conceptually `Normalize(alias) -> Type`, and a way to relate aliases without
first assuming that both can be revealed. Their planned internal decomposition
and first associated-projection slice are specified by the
[Associated Type Normalization
RFD](../rfds/associated-type-normalization/README.md); the semantic
requirements are:

- a named type alias normalizes infallibly to its substituted right-hand side;
- an associated type normalizes by trait matching and associated-value
  selection from the environment or an impl, with the resulting obligations
  and uncertainty preserved;
- an opaque normalizes only inside its definition boundary, or in a future
  code-generation mode;
- identical alias applications can be related structurally; and
- declared associated-type and opaque bounds can prove predicates about an
  unnormalized alias.

Failure or inability to normalize is not automatically `No`. An unavailable
opaque hidden type is intentionally unrevealed, while a projection may be
blocked on inference, trait ambiguity, or resource exhaustion. Conversely,
normalization is not required merely to use a bound declared for the alias.

Revealability is semantic input. Any cached normalization or alias-relation
query includes enough typing context to distinguish an opaque's definition
boundary from an outside use. Projection normalization depends on the fixed
trait and associated item, relevant impl candidates, selected associated
values, and their predicates; it must not read unrelated impls or callee
bodies.

### Ground and non-ground queries

A canonical query is **ground** when it has no flexible existential inputs.
Rigid generic placeholders count as ground: they are constants which the
solver cannot bind. Variables introduced locally while opening an impl or goal
binder do not make the caller-facing query non-ground.

For a non-ground query, the solver must:

- terminate, subject to the ordinary process-level failure model;
- return only sound `Yes`, `No`, substitution, residual, and hard-hint data;
- return `No` only when no admissible instantiation can prove the goal; and
- be allowed to return ambiguity even when some valid answer exists.

For a ground query, the solver must, unless an explicit resource limit is
exceeded:

- return `Yes` exactly when the goal follows from the represented program and
  environment; and
- return `No` exactly when it does not.

Search-induced ambiguity is not a final ground result. Unsupported or
incomplete candidate sources are part of the represented-program boundary and
must be reported rather than silently interpreted as exhaustive failure.

This contract lets body inference defer an ambiguous non-ground obligation,
learn more type information elsewhere, and retry the obligation when it has
become ground. If inference never supplies enough information, Sage may ask the
user for a type annotation at a different point than rustc would.

### Resource-bounded completeness

Unrestricted Rust-style clauses can generate an infinite sequence of distinct
ground goals:

```text
T: Foo
Bar<T>: Foo
Bar<Bar<T>>: Foo
Bar<Bar<Bar<T>>>: Foo
...
```

Tabling repeated canonical goals does not terminate this sequence because no
goal repeats. Ground completeness is therefore conditional on not exceeding
configured structural limits. The intended limit classes are:

- proof depth;
- canonical term or goal size;
- logical work budget; and
- cycle-fixpoint iterations.

Limits are measured deterministically. Term size is a structural measure, not
stash allocation size. A work budget must not be distributed according to
incidental polling order. If limits become configurable, their effective
configuration participates in the cached query boundary.

## Knowledge returned by the solver

The destination distinguishes a value-producing solver operation from the
proposition language used for assumptions, conjunction, implication,
quantification, and residual conditions:

```rust,ignore
enum SolverGoal<'db> {
    Prove(ProofGoal<'db>),
    Normalize(AliasTy<'db>),
}

enum GoalOutput<'db> {
    Proven,
    Type(Ptr<Ty<'db>>),
}
```

The names are conceptual until the normalization RFD lands. `Prove(P)` returns
`Proven`; `Normalize(A)` returns `Type(T)`. Keeping structural `ProofGoal`
separate avoids assigning an arbitrary value to a conjunction containing
several value-producing operations. A normalization operation may still return
a `ProofGoal` residual which must hold for its type result to be valid.

`GoalOutput` is intentionally extensible. Future operations such as callable
instance resolution or vtable construction may add purpose-built outputs; they
need not be forced into the type variant merely because function-item types or
vtable pointers can be represented in Rust's type system.

Let `Cond(A)` mean the conjunction of a response's substitution and residual
proof goal, with its response-local variables existentially bound. A successful
answer also contains a canonicalized `GoalOutput`; response-local variables in
that output participate in binding, occurs and universe checks, caching, and
caller import just like variables in substitutions and residuals.

### Definite answers are sufficient

A conditional answer `A` for operation `G` and output `V` promises:

```text
Cond(A) => G evaluates to V
```

For `Prove(P)`, this reduces to `Cond(A) => P` because the only successful
output is `Proven`. An empty substitution with a trivially true residual is
unconditional. Such an answer is absorbing for a proof disjunction: no sibling
can change the truth of the proposition.

An unconditional value-producing answer is not automatically absorbing. A
normalization candidate returning `Type(A)` cannot cancel a live candidate
which may return `Type(B)` unless candidate priority proves the first dominates
the second or the complete answers are known to agree. Output agreement is part
of answer merging, not an incidental property of substitutions.

### Progress envelopes are necessary

A progress envelope `P` promises that every final answer still possible
implies its conditions. For a proof operation this can be written:

```text
Prove(Goal) succeeds => Cond(P)
```

For any operation `G`, including normalization, the general form is:

```text
G evaluates to any V => Cond(P)
```

Thus `P` constrains every possible completion without claiming a final output.
If a future progress channel constrains the output itself, that constraint must
also be necessary for every value still possible; it is not a successful
`GoalOutput` until the operation has a definite answer. If each live alternative
has an envelope `E_i`, an aggregate envelope must be a sound common
generalization:

```text
E_1 => P
E_2 => P
...
```

As alternatives make progress or fail, the aggregate envelope may strengthen:

```text
P_new => P_old
```

Previously published necessary information therefore never requires
retraction. An incomplete or not-yet-enumerated candidate source contributes
the unconstrained envelope `true`.

The current `Maybe::hints` value is a restricted, final form of this channel:
it contains only hard substitution facts necessary across every still-possible
alternative and no semantic goal output. Planned incremental publication may
expose candidate-local and frame-level envelopes before the owning future
completes.

### Subsumption

The directional convention is:

```text
A subsumes B  iff  Cond(B) => Cond(A)
```

Thus `A` is at least as general as `B`. For a value-producing operation,
subsumption additionally requires output compatibility under those conditions;
`Proven` is trivially compatible with `Proven`, while two type outputs must be
provably equal or ordered by an operation-specific candidate rule. A completed
answer can cancel a pending alternative only when it is proven to subsume the
pending alternative's entire envelope and possible output. Conservative
failure to prove subsumption may retain redundant work; a false-positive
subsumption result would be unsound.

### Ambiguity and overflow

Ambiguity means the solver intentionally cannot choose a unique sound result
from the available non-ground information. Overflow means a configured
resource bound prevented completion. Both are possible outcomes, but they have
different diagnostics, retry behavior, and caching implications.

An absorbing result still wins over an exhausted sibling when logic permits
it:

- an unconditional `Yes(Proven)` wins a proof disjunction;
- a definitive `No` wins a conjunction; and
- otherwise an exhausted branch prevents a result which requires exhausting
  all possibilities.

## Proof-search model

### Global, indexed candidate discovery

Trait impls are global within the compilation world visible to rustc: candidate
discovery includes applicable impls from the local crate and every reachable
external crate represented by compiler metadata. It does not mean scanning
unrelated or downstream crates which are absent from the current compilation.

The semantic candidate query is keyed at least by the fixed trait. It must not
enumerate impls for every trait and filter them only inside proof execution.
An eventual simplified self-type key may further partition candidates by a
rigid outer head such as an ADT, reference, tuple, slice, scalar, or function
type. That refinement is conservative:

- every actually applicable impl is returned;
- blanket and otherwise unclassifiable impls remain in a fallback bucket; and
- indexed and exhaustive discovery produce the same canonical solver result.

This boundary is also incremental. Adding or changing an impl for an unrelated
trait must not invalidate candidate discovery for the queried trait. A
self-type refinement should eventually prevent unrelated rigid-head changes
from invalidating a narrower query. Candidate order is deterministic but has
no semantic effect.

The current `local_impl_candidates(LocalCrateSymbol, TraitSymbol)` query is the
trait-keyed local boundary. It still linearly scans expanded local impls,
but also reports whether unresolved item/attribute macros, ambiguous or
unsupported derives, or unresolved trait impl headers could hide a relevant
impl. An impl with an unrepresented active attribute transformation is not a
definite candidate, nor is an attributed containing module or macro expansion.
A derive attached to an item with another unexpanded active attribute is also
withheld rather than published from the pre-transformation input.
A uniquely resolved item macro with successfully parsed output remains
complete. Failed, ambiguous, or depth-limited expansion is omitted from
definite candidates and makes the source incomplete. The scan still depends on
the whole expanded module vector, so local unrelated-trait invalidation
isolation is explicitly not built yet. External trait defining predicates are
available through the typed `TcxDb` boundary, so eligible local impls of
represented external traits can be proved.

For an external trait, `external_relevant_impls(trait, optional_self_head)`
imports the deterministic set of explicit impl identities visible in reachable
external crates. The optional rigid head filters only provably disjoint impl
heads; blanket and unclassifiable impls remain. A separate
`external_impl_signature(impl)` query loads only a binder-aware header, lowers
it into the local impl-signature shape, and feeds the same candidate
instantiation and proof machinery. Associated values and impl items are not
part of either operation. Unsupported headers or an incomplete source retain
uncertainty instead of manufacturing a negative answer. Structural compiler
traits such as `Sized` and `MetaSized` have explicit Sage candidates because
they are not enumerable user impls. Rustc supplies their identities and type
structure but does not answer the Sage goal.

The external query trace is already isolated by trait and conservative rigid
self head and is reused unchanged on a warm proof. The remaining index work in
the [Trait Impl Candidate Discovery RFD](../rfds/trait-impl-candidate-discovery/README.md)
is finer local-source partitioning and its edit-invalidation matrix. Local
traits may use exhaustive local negative reasoning because upstream crates
cannot implement a downstream trait.

Incremental conformance is verified with query traces. Tests distinguish:

- Salsa tracked functions whose bodies actually execute;
- semantic candidate lookups with stable trait and self-type keys; and
- external `TcxDb` metadata requests.

The trace is cleared after fixture setup and before the operation under test.
Because solver work may complete in different orders, assertions compare
normalized event sets or multisets unless a particular count or dependency
order is itself part of the contract.

### Disjunction and conjunction

Candidate alternatives form disjunctions. Each candidate owns speculative
inference state and eventually produces a canonical response. The aggregate
retains non-dominated definite answers and necessary information from every
still-possible alternative. Proof candidates all return `Proven`; candidates
for a value-producing operation also merge their outputs. Incompatible
normalization types remain ambiguous unless coherence or specialization gives
one candidate semantic priority.

Conjuncts share a logical inference problem. A definitive `No` is absorbing,
while sound constraints learned by one conjunct may advance another. Planned
concurrent conjunction must account for stale isolated snapshots: a sibling
failure is cancellative only when it remains definitive after all hard sibling
information is reconciled.

### Whiteboard frames

An atomic whiteboard frame represents shared canonical work for one goal. A
query-owned producer computes the frame; requesters subscribe through futures.
Candidate alternatives inside that producer are not separate whiteboard
entries. Candidate-local progress belongs to the producer's aggregation scope.

The planned progress design has two layers:

- candidate-local slots used for sibling comparison and cancellation; and
- an optional frame-level provisional summary derived from all candidates and
  visible to the frame's subscribers.

The frame still has one final return value. Intermediate summaries are
revisioned side information, not additional `Future::Output` values.

### Futures and intentional yields

Rust futures remain the representation of suspended continuations. The solver
may add intentional yield points at candidate expansion, nested-goal descent,
atomic-goal boundaries, or bounded unification work. Yield placement defines
the effective search distance: round-robin execution measured in yield quanta
is interleaving search, not literal breadth-first traversal by proof-tree
depth.

The scheduler must poll runnable work in bounded rounds. A future which wakes
itself and returns `Pending` is deferred to a later round rather than being
immediately drained forever.

## Recursive proof search

Cycle semantics are logical, not a property of which future happened to poll
first.

- An inductive progress-free recurrence does not establish itself.
- A designated coinductive recurrence may assume the cycle provisionally.
- A cycle involving unknown path semantics remains conservative.
- Repeated canonical goals may be evaluated through provisional results and a
  fixpoint.
- Infinitely growing non-repeating goals are handled by deterministic resource
  limits rather than ordinary cycle lookup.

The precise search-graph, fixpoint, and path-kind rules remain under discussion
in the [Trait Solver Cycle Semantics RFD](../rfds/trait-solver-cycle-semantics/README.md).

## Scheduling and deterministic results

For fixed semantic inputs and configured limits, scheduling may change only:

- discovery and intermediate-publication order;
- cancellation timing;
- elapsed work and peak memory; and
- debugging traces which are explicitly outside the solver response.

It may not change:

- `Yes`, `Maybe`, or `No`;
- the successful goal output;
- substitutions or response-variable numbering;
- residual goals or hard hints;
- ambiguity versus overflow; or
- the reported overflow kind.

This requires order-independent answer reduction, canonical response
normalization, monotone intermediate information, logically justified early
cancellation, and deterministic resource accounting. Tests should perturb
ready-queue ordering and assert an identical `Stashed<QueryResult>`.

## Current and planned state

| Area | State |
|---|---|
| Positive, type-only local proving | Built |
| Isolated candidate futures and active atomic frames | Built |
| Order-independent completed-answer reduction | Built |
| Final hard substitution hints | Built |
| Trait-keyed local impl discovery with conservative expansion/header completeness | Built, provisional linear scan |
| External trait signatures and local/external explicit impl headers | Built |
| Unrelated-trait invalidation isolation and query-trace proof | External queries are trait/head keyed with cold/warm trace; edit-invalidation proof and local partitioning planned |
| Global trait-keyed impl discovery | Built for explicit impls in the visible local and reachable-external world |
| Conservative simplified-self-type index | Built for external metadata; local refinement planned |
| Parent-chain inductive cycle cutoff and depth limit | Built, provisional |
| Groundness-sensitive result causes | Planned |
| Term-size and deterministic work limits | Planned |
| Polling rounds and intentional yields | Planned |
| Concurrent conjunction | Planned |
| Candidate and frame progress envelopes | Planned |
| Provisional cycle fixpoints and coinductive paths | Planned |
| Goal-specific outputs (`Proven` and `Type`) | Planned by Associated Type Normalization RFD |

The planned work is split across the
[Cycle Semantics](../rfds/trait-solver-cycle-semantics/README.md),
[Scheduling and Fairness](../rfds/trait-solver-scheduling/README.md), and
[Incremental Results](../rfds/incremental-trait-results/README.md) RFDs.
Candidate enumeration and its incremental boundary are specified separately by
the [Trait Impl Candidate Discovery RFD](../rfds/trait-impl-candidate-discovery/README.md).
