# RFD: Incremental Trait Results

**Status:** Draft

**Depends on:**

- [Trait Solver Scheduling](../trait-solver-scheduling/README.md) — fair
  polling, progress wakeups, and cancellation
- [Trait Solver Cycle Semantics](../trait-solver-cycle-semantics/README.md) —
  provisional recursive results and completion
- [Trait Solver Design](../../design/trait-solver.md) — sufficient-answer and
  necessary-envelope contract

## TL;DR

- Let candidate futures publish monotone necessary-condition envelopes before
  returning their final answer.
- Aggregate candidate progress into an optional frame-level provisional
  summary without turning frame futures into answer streams.
- Cancel pending alternatives when a completed sufficient answer is proven to
  subsume their entire possible-result envelope.
- Preserve order-independent final reduction and prohibit speculative progress
  from leaking into caller inference state.

## Motivation

The current atomic producer observes a candidate only when its future
completes. A candidate which has matched its head and accumulated useful
substitutions may spend substantial time proving nested goals while siblings
cannot use that information to establish dominance or hard common constraints.

Only an unconditional answer can currently cancel unfinished alternatives.
This is the special case where an entirely unknown pending branch has envelope
`true`, and `true` implies the unconditional answer's condition. Tighter
pending envelopes permit conditional answers to prove that unfinished work is
already redundant.

## Change in a nutshell

Each candidate owns a revisioned progress slot in its atomic producer. A
published envelope `P` guarantees that every final result `Q` still possible
from that candidate satisfies:

```text
Cond(Q) => Cond(P)
```

The producer derives a common aggregate envelope across completed and live
alternatives. It may expose that aggregate through the existing whiteboard
frame while the frame's future remains pending. The final response is still
published exactly once.

## Detailed plans

### Separate sufficient and necessary information

A conditional `Yes` is sufficient:

```text
Cond(A) => Goal
```

A progress envelope is necessary:

```text
Goal => Cond(P)
```

These are distinct types or are otherwise impossible to confuse in APIs. An
envelope cannot be treated as proof of the goal.

### Add candidate-local progress slots

Candidate alternatives do not receive independent whiteboard frames. The
owning atomic producer allocates progress slots keyed by stable candidate
identity. A candidate may replace its slot with a stronger revision after head
matching, applying hard nested information, simplifying residuals, or
eliminating possibilities.

Every replacement satisfies:

```text
P_new => P_old
```

Dropping or failing a candidate removes it from the live aggregate. Completion
replaces its progress with the final answer or final ambiguity envelope.

### Aggregate across every possible alternative

For live envelopes `E_i`, the producer computes a common generalization `P`
such that every `E_i => P`. Completed `Yes` answers and `Maybe` hints also
participate. Definitive `No` contributes no possible outcome.

All candidate sources must be enumerated before publishing a nontrivial
aggregate. An incomplete source contributes `true`, which prevents unsupported
knowledge from becoming a hard constraint.

The MVP anti-unifier already computes a restricted common necessary
substitution at finalization. This RFD determines whether progress remains
substitution-only or may carry residual goal structure.

### Use envelopes for pending-branch cancellation

A completed answer `A` may cancel live branch `B` only after proving:

```text
Envelope(B) => Cond(A)
```

Every final result from `B` is then subsumed by `A`. The check may be
conservative; a failure to prove implication retains work.

Because the current implication checker is intentionally incomplete, online
cancellation must not make precision arrival-order-dependent. The RFD will
compare:

- a proper order-independent answer semilattice;
- retained dominance certificates or tombstones for cancelled branches; and
- restricting early cancellation to implication classes with a transitive,
  complete recognizer.

### Keep progress advisory unless it is a hard fact

Candidate-specific speculative substitutions never mutate caller or sibling
inference state. The aggregator may publish only facts proven necessary across
every possible alternative. Applying such a hard hint remains transactional.

Frame subscribers may observe a revisioned provisional summary for scheduling
or propagation, but `ProofFuture::Output` remains the single final canonical
response.

### Treat recursive publication separately

Publishing an envelope for local pruning is not automatically equivalent to
feeding provisional answers into recursive table consumers. Recursive answer
subsumption can change a least fixed point unless its aggregation is compatible
with consequence. Integration with the cycle search graph therefore requires a
separate soundness argument.

## Frequently asked questions

### Is an envelope a partial `Yes`?

No. A partial `Yes` gives a sufficient condition. An envelope gives a necessary
condition and may describe states in which no candidate ultimately succeeds.

### Can an earlier envelope need retraction?

No. Candidate and aggregate updates strengthen monotonically. An update which
might later weaken is not publishable as hard progress.

### What if every currently possible candidate later fails?

The earlier envelope was still a necessary condition of satisfying the
obligation. The obligation then becomes definitively false. Hard propagation
must remain transactional and cannot turn that failed program into an accepted
one.

### Why not make each candidate a whiteboard frame?

Candidates are clause-local computations with isolated proof state, not
canonical goals shared by arbitrary requesters. Giving them frame identity
would couple alternative scheduling to goal caching and cycle semantics.

## Implementation

See [Implementation plan and status](./implementation.md).
