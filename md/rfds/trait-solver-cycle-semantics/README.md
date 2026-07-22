# RFD: Trait Solver Cycle Semantics

**Status:** Draft

**Depends on:**

- [Trait Solving](../trait-solving/README.md) — canonical goals, responses,
  active frames, and the current inductive MVP
- [Trait Solver Design](../../design/trait-solver.md) — destination semantic
  contract

## TL;DR

- Replace the current parent-chain cutoff with explicitly specified inductive,
  coinductive, and unknown recursive-proof semantics.
- Preserve sound termination for non-ground queries and resource-bounded
  soundness and completeness for ground queries.
- Distinguish repeated canonical cycles from infinitely growing goal chains.
- Return explicit ambiguity and overflow causes rather than using one
  undifferentiated `Maybe`.

## Motivation

The MVP treats an atomic goal recurring in its parent chain as inductive `No`
and turns depth 64 into `Maybe`. This is sufficient for the positive,
inductive, type-only implementation, but it is not a destination model for
auto traits, normalization, shared provisional work, or concurrent
conjunctions.

The active whiteboard also makes call-stack ancestry operationally awkward.
A shared producer is query-owned, while cycle detection currently gives it the
parent selected by the request which created it. Broader sharing requires cycle
semantics based on proof dependencies rather than the lifetime of one
requester.

Finally, repeated-goal detection does not terminate strictly growing chains
such as `T: Foo -> Bar<T>: Foo -> Bar<Bar<T>>: Foo`. Resource limits are part
of the semantic interface, not merely an implementation assertion.

## Change in a nutshell

The solver will model recursive evaluation as a dependency graph of canonical
goals. A repeated goal receives a provisional result determined by the path's
semantic kind and is reevaluated toward a fixpoint when required. Non-repeating
growth is stopped by deterministic structural limits.

This RFD must preserve these externally visible requirements:

- non-ground evaluation terminates soundly but may be incomplete;
- ground evaluation is sound and complete unless a configured resource limit
  is exceeded;
- `No` is never inferred merely from lack of a useful instantiation;
- overflow is never reported as logical `No`; and
- fixed inputs and limits produce the same canonical result under every valid
  polling schedule.

## Detailed plans

### Define groundness at the canonical boundary

A query is ground when it has no `ExistentialInput` canonical variables. Rigid
placeholders count as ground. Variables introduced while opening candidate or
goal binders are internal witnesses and do not change entry-query groundness.

Cycle and resource policy may use this property to choose exact evaluation or
sound approximation, but all published constraints remain subject to the same
universe and substitution invariants.

### Classify recursive paths

Every recursive dependency edge needs a semantic kind sufficient to classify
a cycle as:

- **inductive**, initially assuming no proof;
- **coinductive**, provisionally allowing the recurrence for designated goals;
  or
- **unknown**, conservatively producing ambiguity.

The rule for mixed paths must be specified before implementation. It cannot
depend on which future happened to create a frame or poll first.

### Evaluate repeated goals to a fixpoint

The RFD will compare at least:

- rustc-style stack entries with provisional-cache dependencies and rebasing;
- SCC-oriented table completion; and
- a smaller per-query search graph adapted to Sage's one-result frames.

The selected design must define:

- the provisional value for every path kind;
- which dependent entries are invalidated when the value changes;
- how response substitutions and residual goals participate in convergence;
- how answer subsumption affects convergence; and
- when a cycle head is complete.

Fixpoint iteration is bounded. Exceeding the bound yields a fixpoint-overflow
result, never `No`.

### Bound non-repeating growth

At minimum, the solver needs deterministic limits for:

- proof depth;
- canonical goal or term size;
- logical work; and
- fixpoint iterations.

Term size is measured structurally with a specified sharing policy. Logical
work cannot be a shared counter consumed according to incidental poll order.
If limits are configurable, their effective values are part of the query's
cache configuration.

### Preserve absorbing logical results

Overflow in one alternative does not automatically determine its parent:

- an unconditional `Yes` can still prove a disjunction;
- a definitive `No` can still disprove a conjunction; and
- otherwise an exhausted alternative remains possible and prevents an
  exhaustive result.

Multiple overflow causes use a deterministic combination rule.

## Frequently asked questions

### Why not keep parent-chain cycle detection?

It is a useful MVP loop check, but it couples proof ancestry to producer
ownership and cannot express provisional coinductive or fixpoint semantics.
It also does nothing for infinitely growing non-repeating goals.

### Does tabling guarantee termination?

No. Tabling makes repeated canonical goals finite. A program may still
generate an unbounded sequence of distinct goals or answers, so structural
limits remain necessary.

### May a non-ground cycle return ambiguity despite a valid answer?

Yes. That is permitted incompleteness. It may not return a false `No`, and any
substitution or hard hint it publishes must remain sound.

### Is a ground query allowed to return ambiguity?

Not because the search algorithm declined to explore a finite proof. A ground
query may return an explicit resource-exhaustion result when a configured
limit prevents completion.

## Implementation

See [Implementation plan and status](./implementation.md).
