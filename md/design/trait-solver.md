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
survives inference and canonical solver boundaries. Associated projections now
have an operational normalization path; named-alias expansion and opaque reveal
remain planned.

The solver operation language includes input-only normalization,
`Normalize(alias) -> Type`, and caller-side alias relation without first
assuming that both aliases can be revealed. The completed first
associated-projection and body-integration slice is recorded by the
[Associated Type Normalization
RFD](../rfds/associated-type-normalization/README.md). The broader destination
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

<a id="sol-a1"></a>
> **SOL-A1 — Alias normalization preserves reveal context and narrow demand.**
> An alias retains its identity until an operation requests its value.
> Revealability participates in the canonical input, and projection
> normalization reads only the fixed trait, associated item, applicable impl
> headers, selected associated values, and predicates required for that value.
> It never reads unrelated impls or bodies. This is the solver consequence of
> [D13](./decisions.md#d13-named-associated-and-opaque-aliases-share-one-semantic-family)
> and [D15](./decisions.md#d15-cross-item-dependencies-stop-at-semantic-interfaces).
>
> **Required verification:** Named, associated, and opaque aliases survive
> folding and canonical round trips with reveal context intact; inside/outside
> opaque tests produce the appropriate different result; query traces for one
> projection include only relevant headers and its requested value and exclude
> sibling associated values, unrelated impls, and every body.

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

<a id="sol-a2"></a>
> **SOL-A2 — Solver completeness is groundness-sensitive and explicitly
> bounded.** Non-ground queries terminate with only sound knowledge but may be
> ambiguous; ground queries return the complete logical answer unless a named,
> deterministic resource limit is exhausted. Incomplete candidate sources,
> ambiguity, and each overflow class remain distinct and none becomes `No`.
> This is the solver consequence of
> [D9](./decisions.md#d9-trait-solving-is-groundness-sensitive-and-resource-bounded)
> and [D16](./decisions.md#d16-incompleteness-is-an-explicit-terminal-outcome).
>
> **Required verification:** A fixture matrix covers ground positive and
> negative answers, non-ground ambiguity despite an available instantiation,
> incomplete candidate sources, and every configured limit class. Repeated
> runs produce the same canonical outcome and no incomplete or exhausted case
> produces `No`.

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

These are the implemented canonical operation and output boundaries.
`Prove(P)` returns `Proven`; `Normalize(A)` returns `Type(T)`. Keeping
structural `ProofGoal` separate avoids assigning an arbitrary value to a
conjunction containing several value-producing operations. A normalization
operation may still return a `ProofGoal` residual which must hold for its type
result to be valid.

`GoalOutput` is intentionally extensible. Future operations such as callable
instance resolution or vtable construction may add purpose-built outputs; they
need not be forced into the type variant merely because function-item types or
vtable pointers can be represented in Rust's type system.

<a id="sol-a3"></a>
> **SOL-A3 — Goal-specific output is canonical solver knowledge.**
> `Prove(P)` returns `Proven`, while input-only `Normalize(Alias)` returns a
> `Type`. The output's variables participate in response binding, validation,
> caching, comparison, merging, and caller import; the caller's expected type
> is related only after complete candidate answers are merged. This is the
> solver consequence of
> [D14](./decisions.md#d14-solver-operations-return-goal-specific-semantic-outputs).
>
> **Required verification:** Operation/output mismatch is rejected; type
> outputs containing repeated response-local variables round-trip with sharing
> and universes intact; incompatible candidate outputs remain ambiguous even
> when the caller expects one of them; trait proof never exposes a selected
> impl identity.

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

This boundary is also incremental. Candidate order is deterministic but has no
semantic effect.

<a id="sol-a4"></a>
> **SOL-A4 — Candidate discovery is global, trait-keyed, conservative, and
> completeness-carrying.** A proof considers represented impls in the local
> crate and every reachable dependency, keyed first by the fixed trait. Any
> self-head refinement retains blanket and unclassifiable fallback candidates,
> and a missing or incomplete source prevents exhaustive `No`. This is the
> solver consequence of
> [D10](./decisions.md#d10-trait-impl-discovery-is-global-and-trait-keyed)
> and [D16](./decisions.md#d16-incompleteness-is-an-explicit-terminal-outcome).
>
> **Required verification:** Local, upstream, blanket, and unsimplifiable
> fixtures show that indexed discovery loses no exhaustive candidate; unrelated
> traits and provably disjoint rigid heads are absent; indexed and exhaustive
> enumeration produce identical canonical responses; incomplete sources never
> manufacture `No`.

#### Local impl-index incremental firewall

The destination local index is a crate-owned Salsa tracked struct with stable
identity and a private tracked contents field. The contents map fixed trait
identities to deterministic impl-identity buckets and record completeness
hazards separately. A keyed tracked method is the only API which reads the
private map:

```rust,ignore
#[salsa::tracked]
struct LocalImplIndex<'db> {
    #[id]
    krate: LocalCrateSymbol<'db>,

    #[tracked]
    #[returns(ref)]
    contents: LocalImplIndexContents<'db>,
}

#[salsa::tracked]
impl<'db> LocalImplIndex<'db> {
    #[salsa::tracked]
    fn for_trait(
        self,
        db: &'db dyn Db,
        trait_sym: TraitSymbol<'db>,
    ) -> LocalImplCandidates<'db>;
}
```

The example is a destination shape, not a claim that these exact definitions
are built. Rebuilding the index after an unrelated impl edit may reexecute
`for_trait(TraitA)`, because that method reads the changed map. If its result is
equal, Salsa backdates the lookup and the change stops there. The required
guarantee is that the edit does not reexecute `TraitA` impl signature lowering,
canonical solver evaluation, associated-value normalization, or dependent body
checking. This permits a cheap map lookup to repeat without turning the entire
index into dynamic per-key Salsa state.

Known impls are partitioned by their resolved trait. An unresolved construct
which might produce an impl of any trait is retained as a global completeness
hazard and legitimately changes every lookup. A hazard whose target trait is
known can remain within that trait's bucket. Inherent impls do not enter trait
buckets. A future self-head refinement nests specific rigid-head buckets and a
mandatory blanket/unclassifiable fallback within each trait entry; the same
backdating boundary applies.

The map stores signature-level stable impl identities, not lowered headers or
associated-item bodies. An impl-body-only edit should leave the index result
equal. If the current `LocalImplSym` lifecycle cannot preserve that property,
the symbol must be split or given stable header identity rather than weakening
the firewall.

<a id="sol-a5"></a>
> **SOL-A5 — The local impl index is an incremental firewall.** Its stable
> crate-owned identity contains a private tracked map, and keyed tracked lookup
> is the only reader. An unrelated edit may rebuild the map and reexecute a
> cheap lookup, but an equal trait bucket is backdated before impl-header
> lowering, solving, normalization, or body checking; impl-body edits do not
> alter signature-level buckets.
>
> **Required verification:** Persistent edit tests distinguish index rebuild,
> keyed lookup, header lowering, canonical solving, normalization, and body
> execution. Unrelated-trait and impl-body edits stop at an equal lookup;
> relevant-trait and global-hazard edits invalidate exactly their permitted
> consumers; identities remain stable across unrelated edits.

The current `local_impl_candidates(LocalCrateSymbol, TraitSymbol)` query is the
trait-keyed local boundary. It still linearly scans expanded local impl
identities, but resolves only each impl's trait identity before lowering a full
header for the requested trait. It also reports whether unresolved
item/attribute macros, ambiguous or unsupported derives, or unresolved trait
impl headers could hide a relevant impl. An impl with an unrepresented active
attribute transformation is not a
definite candidate, nor is an attributed containing module or macro expansion.
A derive attached to an item with another unexpanded active attribute is also
withheld rather than published from the pre-transformation input.
A uniquely resolved item macro with successfully parsed output remains
complete. Failed, ambiguous, or depth-limited expansion is omitted from
definite candidates and makes the source incomplete. The scan still depends on
the whole expanded module vector and has no stable private index/backdating
layer, so the local incremental firewall is explicitly not built yet. External
trait defining predicates are
available through the typed `TcxDb` boundary, so eligible local impls of
represented external traits can be proved.

For an external trait, `external_relevant_impls(trait, optional_self_head)`
imports the deterministic set of explicit impl identities visible in reachable
external crates. The optional rigid head filters only provably disjoint impl
heads; blanket and unclassifiable impls remain. A separate
`external_impl_signature(impl)` query loads only a binder-aware header, lowers
it into the local impl-signature shape, and feeds the same candidate
instantiation and proof machinery. Associated values and impl items are not
part of either proof operation. `Normalize` uses a separate
`external_associated_type_value(impl, associated_type)` query after header
matching; its local counterpart reads the requested type value without first
lowering the impl-item list. Unsupported headers or an incomplete source retain
uncertainty instead of manufacturing a negative answer. Before local
discovery, an exact orphan-rule check skips the local source for a foreign
trait with no trait arguments and a non-fundamental foreign nominal self type.
The fundamental marker is imported through its own structural metadata query,
separate from the external ADT signature. This pruning cannot omit a legal
local impl and prevents external/external goals
such as `IntoIter<Frame>: Iterator` from depending on unrelated local impls or
macro expansion. Structural compiler traits such as `Sized` and `MetaSized` have explicit Sage candidates because
they are not enumerable user impls. Rustc supplies their identities and type
structure but does not answer the Sage goal.

The external query trace is already isolated by trait and conservative rigid
self head and is reused unchanged on a warm proof. The remaining index work in
the [Trait Impl Candidate Discovery RFD](../rfds/trait-impl-candidate-discovery/README.md)
is finer local-source partitioning and its edit-invalidation matrix. Local
traits may use exhaustive local negative reasoning because upstream crates
cannot implement a downstream trait.

Incremental conformance is verified with edit-and-query traces. Tests
distinguish:

- index construction and keyed lookup, which may reexecute;
- Salsa tracked functions whose bodies actually execute;
- semantic candidate lookups with stable trait and self-type keys; and
- external `TcxDb` metadata requests.

The trace is cleared after fixture setup and before the operation under test.
After an unrelated-trait edit, the keyed lookup must return an equal value and
no unrelated signature, solver, normalization, or body query may execute. A
relevant edit must change the bucket and invalidate its consumers. Body-only
edits must not change signature-level index contents. Global completeness
hazards are tested separately because they intentionally affect every trait.
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

<a id="sol-a6"></a>
> **SOL-A6 — Alternative state is isolated and answer reduction is
> output-aware.** Every disjunct candidate publishes a canonical response from
> isolated inference state. Aggregation is order-independent, retains hard
> knowledge common to every live alternative, and merges value-producing
> answers only when their outputs agree or an explicit priority rule dominates.
> Conjunct cancellation is permitted only after reconciling shared hard
> information. This is the solver consequence of
> [D6](./decisions.md#d6-versioned-egraph-children-are-inference-transactions)
> and [D8](./decisions.md#d8-whiteboard-producers-own-isolated-proof-contexts).
>
> **Required verification:** Candidate-order permutations produce identical
> canonical responses; incompatible normalization outputs remain ambiguous;
> failed or cancelled candidates leak no equality, universe change, wake, or
> obligation; concurrent-conjunction tests reconcile sibling information before
> accepting an absorbing result.

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

<a id="sol-a7"></a>
> **SOL-A7 — One whiteboard frame has one final future output.** A frame owns
> the producer for one canonical atomic goal, and all requesters subscribe to
> that work. Candidate-local and frame-level progress are revisioned side
> information; they do not create extra future outputs or expose raw candidate
> state. Suspended continuations remain Rust futures, polled in bounded rounds.
>
> **Required verification:** Duplicate requesters share one producer, receive
> an identical final response, and cannot observe candidate-local state;
> progress snapshots strengthen monotonically; a self-waking pending future is
> deferred to a later polling round and cannot starve sibling work.

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

<a id="sol-a8"></a>
> **SOL-A8 — Recursive semantics follow canonical cycles, not polling order.**
> Repeated canonical goals use explicit inductive, coinductive, or unknown path
> semantics and provisional fixpoint evaluation. Infinitely growing distinct
> goals are overflow governed by deterministic structural limits, not ordinary
> cycle lookup.
>
> **Required verification:** Focused recursive clause families distinguish
> inductive failure, coinductive success, unknown-path ambiguity, converging
> and non-converging fixpoints, and non-repeating growth overflow; ready-queue
> perturbation never changes any result.

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

<a id="sol-a9"></a>
> **SOL-A9 — Scheduling cannot change the canonical return value.** For fixed
> semantic inputs and configured limits, polling order may change discovery,
> cancellation timing, trace order, elapsed work, and memory use, but not the
> answer class, output, substitutions, residuals, hints, response numbering,
> ambiguity/overflow distinction, or overflow kind.
>
> **Required verification:** Deterministically perturbed ready queues produce
> byte-identical stashed results across proof, normalization, residual,
> ambiguity, and every overflow case; only explicitly non-semantic trace order
> may differ.

## Current status

### Current frontier

The solver performs positive local and reachable-external explicit-impl proof,
associated-type normalization with a type output, isolated asynchronous
candidate evaluation, order-independent completed-answer reduction, final hard
hints, conservative candidate-source completeness, a provisional inductive
cycle cutoff, and a proof-depth limit.

### Implemented capabilities and evidence

| Anchor | State | Implemented claim and evidence |
|---|---|---|
| [SOL-A1](#sol-a1) | Partial | `aliases_round_trip_through_canonical_query_and_response_stashes` preserves the alias family, and `local_normalization_reads_one_keyed_value_without_impl_item_enumeration` verifies a narrow associated-value read; named expansion and opaque reveal are not implemented. |
| [SOL-A2](#sol-a2) | Partial | `unresolved_item_macro_prevents_ground_no`, `unresolved_trait_impl_head_prevents_ground_no`, and `proof_depth_limit_is_maybe` establish that the implemented incomplete/depth cases do not manufacture `No`; the destination groundness and resource matrix is not built. |
| [SOL-A3](#sol-a3) | Partial | `canonical_response_rejects_an_output_for_the_wrong_operation`, `local_associated_type_normalization_produces_a_type_output`, `response_local_type_output_round_trips_with_sharing`, and `incompatible_normalization_outputs_are_ambiguous_but_identical_outputs_merge` exercise operation pairing, response binding, and output-aware merging. The incompatible-output test constructs `SolverGoal::Normalize` directly, so it does not establish caller expected-type isolation. |
| [SOL-A4](#sol-a4) | Partial | `generic_impl_where_clause_is_proved_by_nested_impl` covers nested local candidate obligations. The `Parse::next` evidence in the [Mini-redis roadmap](../implementation/mini-redis.md#slice-2-parsenext) covers trait/head-keyed reachable-external discovery and warm reuse; the local index remains provisional. |
| [SOL-A5](#sol-a5) | Not implemented | No current evidence claims the destination private local index or its edit-invalidation firewall. |
| [SOL-A6](#sol-a6) | Partial | `incompatible_normalization_outputs_are_ambiguous_but_identical_outputs_merge` and the `final_answer_rules_are_order_independent` unit test cover completed answer reduction; concurrent conjunction and cancellation isolation remain planned. |
| [SOL-A7](#sol-a7) | Partial | `same_parent_requests_share_one_live_producer_and_result` verifies shared producer identity and one equal final response; bounded polling rounds and monotone intermediate publication have no current evidence. |
| [SOL-A8](#sol-a8) | Partial | `inductive_impl_cycle_is_no` and `proof_depth_limit_is_maybe` cover the provisional inductive cutoff and depth result only, not the destination path-kind/fixpoint semantics. |
| [SOL-A9](#sol-a9) | Partial | `final_answer_rules_are_order_independent` covers reducer input order; scheduler perturbation and deterministic resource accounting are not implemented. |

### Current limitations

- Local trait-keyed discovery is a provisional linear scan over expanded local
  impl identities; the stable private index and edit-invalidation firewall are
  not built.
- Groundness-sensitive ambiguity causes, term-size limits, deterministic work
  limits, polling rounds, and intentional yield points are not implemented.
- Conjunctions are sequential. Candidate and frame progress envelopes are not
  published.
- Cycle handling uses parent-chain inductive cutoff and depth overflow rather
  than the destination provisional fixpoint/coinductive path semantics.
- General negative reasoning, specialization, GATs, named-alias expansion,
  opaque reveal, and method/vtable output operations are outside the current
  frontier.
- SOL-A3 lacks an integration test in which the caller expects one of two
  incompatible normalization outputs and the solver nevertheless preserves
  ambiguity.

### Related roadmap slices

- [Trait-partitioned impl
  discovery](../implementation/roadmap.md#planned-slice-trait-partitioned-impl-discovery)
  owns the local incremental firewall.
- [Solver recursion, scheduling, and monotone
  progress](../implementation/roadmap.md#planned-slice-solver-recursion-scheduling-and-monotone-progress)
  coordinates the [Cycle Semantics](../rfds/trait-solver-cycle-semantics/README.md),
  [Scheduling and Fairness](../rfds/trait-solver-scheduling/README.md), and
  [Incremental Results](../rfds/incremental-trait-results/README.md) RFDs.
