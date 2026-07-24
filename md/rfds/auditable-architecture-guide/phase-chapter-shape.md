# Proposed phase chapter shape

This is the concrete chapter form proposed by the
[Phase-Oriented, Auditable Architecture Guide RFD](./README.md). It is a
review template, not yet part of the architecture contract.

# Phase name

State in one paragraph where this phase sits in the Rust compilation pipeline
and why it exists.

## Contract

### Granularity

Identify the unit of demand and memoization, such as workspace, crate, module,
item, function body, or solver goal.

### Input

Name the semantic inputs. Distinguish direct query parameters from information
read through supporting subsystems.

### Output

Describe the successful output and its representation. State whether ordering,
identity, provenance, elaboration, or determinism is part of the contract.

### Guarantees

List only properties downstream consumers may rely upon. Link cross-cutting
representation contracts rather than repeating them.

## Entry points

Name the small set of tracked queries or methods that define the phase
boundary. Include ezanchor excerpts for the most important entry points.

## Construction

Describe the load-bearing algorithm before helper-level detail. Examples
include fixed-point iteration, transactional inference, obligation quiescence,
canonicalization, deterministic allocation, and staged elaboration.

Use additional anchored excerpts for the key mechanism. The text should make
the excerpt intelligible without requiring the reader to reconstruct the
overall algorithm from calls.

## Failure and terminal incompleteness

Distinguish:

- invalid or ambiguous source input;
- unsupported Sage functionality;
- resource limits;
- unavailable external information; and
- internal failures.

State which partial information may be retained and which successful-output
guarantees no longer hold. Do not describe a terminal incomplete result as
work that merely needs more scheduling.

## Incremental dependencies

State:

- the query key and memoization boundary;
- the information this phase is allowed to read;
- important information it must not read;
- which relevant edits should invalidate it; and
- which unrelated edits should preserve or backdate its result.

## Worked example

Begin with a small Rust fragment. Follow it from phase input to phase output,
using readable semantic output and anchored implementation excerpts.

## Code map

Give a short list of the primary modules and responsibilities. This is an
entry guide, not a full source-tree inventory.

## Current status

This section is explicitly about the implementation today; the preceding
sections remain the destination contract.

### Current frontier

State the broadest coherent portion of the destination that works.

### Implemented capabilities and evidence

Map implemented claims to focused tests, snapshots, structured query traces,
edit experiments, Oracle results, Semantic Inspector commands, and relevant
code anchors. Each entry states:

- the claim being supported;
- the inspectable artifact;
- how to reproduce or inspect it; and
- the implementation entry point for deeper reading.

### Current limitations

For each concrete difference from the destination, state its consequence.
Distinguish user errors, Sage limitations, resource limits, and deliberately
deferred scope where applicable.

### Related roadmap slices

Link the cross-cutting slices whose implementation would change this status.
Do not duplicate their acceptance criteria or ordered implementation plans.
