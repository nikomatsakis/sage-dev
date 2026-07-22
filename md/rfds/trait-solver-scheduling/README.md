# RFD: Trait Solver Scheduling and Fairness

**Status:** Draft

**Depends on:**

- [Async Type Checker](../async-type-checker/README.md) — scoped futures,
  wakeups, cancellation, and body quiescence
- [Trait Solver Cycle Semantics](../trait-solver-cycle-semantics/README.md) —
  recursive completion and deterministic resource outcomes
- [Trait Solver Design](../../design/trait-solver.md) — destination semantic
  contract

## TL;DR

- Retain Rust futures as proof continuations.
- Give intentional yields real fairness semantics through bounded polling
  rounds.
- Interleave candidate, producer, and eventually conjunct work without making
  completion order observable in the solver result.
- Define resource accounting so a polling schedule cannot change whether a
  query returns `Yes`, `Maybe`, `No`, ambiguity, or overflow.

## Motivation

The current runtime uses custom FIFO ready queues. `ScopedTasks::poll_next`
and the whiteboard driver drain their queues until empty. A future which wakes
itself and returns `Pending` can therefore be polled again during the same
drain. This is suitable when `Pending` always waits for another solver event,
but it does not give an intentional `yield_now().await` a scheduling boundary.

Trait proof search benefits from interleaving. A finite proof should not be
starved by an infinite or very deep sibling. Conjunct failure and unconditional
disjunct success should cancel irrelevant deep work. The scheduler must provide
these operational properties while preserving one deterministic query result.

## Change in a nutshell

The solver keeps futures and real wakers, but dispatches runnable work in
bounded rounds. Work made runnable during a poll is deferred to a later round.
Intentional yields can then rotate a continuation behind its peers.

Yield placement defines a logical work quantum. This RFD specifies the
available yield points, fairness guarantees, task priorities, cancellation
rules, and deterministic accounting for those quanta.

## Detailed plans

### Retain futures as continuations

The inference snapshot remains in the captured proof state, remaining work in
the compiler-generated future state, and suspension in the future itself. The
RFD does not introduce a first-order `ProofContinuation` state machine.

Scheduler-visible metadata may still record stable task identity, owning frame,
proof depth, work counters, or a progress revision. That metadata describes
the continuation without replacing it.

### Poll in rounds

A polling round observes the runnable set at its start and polls each selected
entity at most once. Self-wakes and newly runnable work are queued for a later
round. The rule applies consistently to:

- scoped candidate tasks;
- whiteboard producer frames; and
- the root proof future.

The coordinator waker must ensure a yielded child causes its parent future to
be polled in a later round rather than busy-looping inside `poll_next`.

### Define yield points

The RFD will evaluate intentional yields after:

- candidate head matching or expansion;
- creation of a nested atomic request;
- one atomic or conjunct step;
- publication of useful progress; and
- a bounded amount of unification or normalization work.

These are not equivalent to breadth-first proof-tree traversal. They define
interleaving distance in scheduler quanta, similar to suspension points in
miniKanren streams.

### State fairness guarantees

At minimum:

- every continuously runnable continuation is eventually polled;
- a self-waking continuation cannot monopolize one driver invocation;
- a finite sibling proof is not starved by an infinite sibling which yields;
- cancellation is observed and fully drained before borrowed proof state is
  dropped; and
- a query with no runnable work and no registered wake source is a diagnosed
  internal deadlock.

The RFD must state whether any priority classes weaken strict fairness and how
priority aging prevents starvation.

### Make results schedule-invariant

For fixed canonical inputs and configured limits, queue order may change
discovery order but not `Stashed<QueryResult>`. This includes response-variable
normalization, residual and hint ordering, and the reported uncertainty cause.

Consequences include:

- final reduction remains order-independent;
- early cancellation requires a logical absorbing or dominance certificate;
- an overflowed sibling does not suppress an independently sufficient result;
- debugging traces are separated from semantic return data; and
- global wall-clock or poll-order-consumed fuel is not a semantic limit.

### Prepare for concurrent conjunction

Conjunction concurrency is planned, not part of the initial scheduler change.
The scheduler must support it without assuming conjuncts are independent.
Concurrent conjunct results require transactional reconciliation and retry when
a result was computed from a stale inference snapshot.

## Frequently asked questions

### Why not use `FuturesUnordered`?

It may replace part of the scoped-task implementation, but it does not own
whiteboard frame identity, subscriptions, query-owned producer lifetime, or
solver-specific resource accounting. The RFD will compare it with the custom
queue rather than assuming either result.

### Is this breadth-first search?

No. It is fair interleaving measured in chosen yield quanta. A continuation may
perform several logical operations between yields.

### May a different scheduler return an equivalent but differently normalized answer?

No. The requirement is canonical-value equality, not merely logical
equivalence.

### Can a global work counter be used?

Only if its allocation is independent of incidental poll order. A simple
shared decrementing counter would allow one schedule to spend fuel on a deep
branch before another schedule discovers an absorbing answer.

## Implementation

See [Implementation plan and status](./implementation.md).
