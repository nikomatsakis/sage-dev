# Architecture decisions

Cross-cutting decisions that shape the overall system. Feature-local decisions stay in
their RFD and are linked from there.

Each entry has a short code (`D<n>`) for easy cross-reference.

## D1: Salsa for incremental computation

We use [salsa](https://github.com/salsa-rs/salsa) as the incremental computation
framework. All "interesting" computations are salsa tracked functions; inputs are salsa
inputs or interned structs.

## D2: Stash for type-level interning

Types and other IR nodes are interned into a per-database `Stash` (a custom arena with
hash-consing) rather than using salsa interning. This gives us pointer-sized handles
(`Ptr<T>`) with zero-copy decomposition for compound types.

## D3: Tree-sitter for parsing

We use tree-sitter-rust for parsing rather than writing a hand-rolled parser. This gives
us error recovery, incremental re-parsing, and avoids duplicating grammar maintenance.

## D4: Oracle test harness

End-to-end correctness is validated by an "oracle" harness that compiles test programs
with both sage and rustc, then compares diagnostics. This ensures sage's behavior
converges with the reference compiler.

## D5: Symbols as the uniform IR unit

Every named entity (function, type, field, variant, generic parameter, etc.) is
represented as a `Symbol` with per-kind data. This gives a uniform shape to queries
and avoids proliferating distinct tracked structs per entity kind.

## D6: Versioned egraph children are inference transactions

Operations that may partially constrain inference state run in a child egraph
version. On complete success, a short-lived probe may collapse its child into
the direct parent; on failure, it discards the child, so partial equalities,
bounds, and wake effects cannot escape. The probe API permits collapse only
when the probe is the parent's only live child, so no sibling can observe its
parent changing underneath an in-progress computation.

Concurrent alternatives are different: each candidate owns an isolated proof
context and performs its speculative matching in a child version of that
context. Candidate versions are never merged into a requester. Each candidate
extracts a canonical response before its isolated context is dropped, so no
alternative-specific choice can become another candidate's ancestor state.

Because descendants read sparse state through their ancestry, per-version
writes target leaf versions only. Creating a child freezes its parent until
every child is discarded or the sole child is atomically collapsed; read-only
lookup from the frozen parent is allowed, but path compression, variable
allocation, equality/bound changes, rebuild publication, semantic revisions,
and wake publication are not. Append-only global stash allocation is the sole
non-version fact exempt from this rule. This gives each branch a stable
ancestor snapshot without copying the parent maps.

## D7: Inference-variable identities are unique across egraph versions

An `InferVarIndex` identifies one inference variable throughout an egraph; two
sibling versions never reuse the same index for different variables. Metadata
records the version which owns each variable, and an explicit-version egraph
operation may access it only from that version or a descendant. Version handles
are not reused without a generation, so a discarded variable's recorded owner
cannot become valid again accidentally.

This uses more stash entries than branch-relative index reuse, but prevents a
`Ty::InferVar` allocated in one concurrent candidate from being reinterpreted
as a sibling's variable. It also makes response extraction, cancellation, and
exclusive child-to-parent commits enforceable at the type-identity boundary.

Variable metadata keeps immutable creation universe separate from a versioned
current universe ceiling. Equality may transactionally lower that ceiling to
prevent delayed leaks through nested flexible variables; committed lowering is
a semantic wake/revision, and canonical query identity uses the current ceiling.

## D8: Whiteboard producers own isolated proof contexts

Every in-progress trait-solver frame imports its canonical query into a fresh
proof stash and egraph owned by the frame producer. Candidate alternatives do
the same before opening their local child transaction. The per-query
whiteboard, rather than a shared egraph root, is the common coordination point:
it owns producer futures, subscriptions, parent links, and branch-independent
stashed responses.

This costs additional per-frame instantiation, but it prevents a suspended
producer from borrowing a shared mutable egraph and makes requester
cancellation independent from producer lifetime. Raw inference-variable and
version identities cannot cross contexts because every published response is
validated and canonicalized first.

## D9: Trait solving is groundness-sensitive and resource-bounded

A canonical trait query is ground when it has no flexible existential inputs;
rigid generic placeholders still count as ground. Non-ground queries must
terminate and publish only sound proofs, substitutions, hard hints, and
negative results, but they may return ambiguity despite a valid answer. Ground
queries are sound and complete unless an explicit deterministic resource limit
is exceeded.

Ambiguity and resource exhaustion are distinct results. Depth, canonical term
size, logical work, and fixpoint limits never become logical `No`. For fixed
canonical inputs and configured limits, polling order may change discovery and
resource use but not the final canonical solver response. Sage therefore aims
for broad rustc compatibility without requiring exactly the same source type
annotations in every non-ground inference case.

The detailed contract lives in [Trait Solver Design](./trait-solver.md).

## D10: Trait impl discovery is global and trait-keyed

Trait candidate discovery covers every impl visible in the current compilation
through local source and reachable external-crate metadata. The fixed trait is
the mandatory primary query key; the solver does not enumerate impls for every
trait and filter them only during proof execution.

A simplified self-type key may refine lookup when its rigid outer shape is
known. Such an index is conservative: it may return extra candidates but never
omit an applicable specific, blanket, or unclassifiable impl. Indexed and
exhaustive discovery have identical solver meaning. The trait key, and
eventually compatible self-type partitions, also form the incremental
invalidation boundary so unrelated impl changes do not reexecute the query.

The current `local_impls(LocalCrateSymbol)` scan is an MVP source. The
destination and required conformance tests live in
[Trait Solver Design](./trait-solver.md) and the
[Trait Impl Candidate Discovery RFD](../rfds/trait-impl-candidate-discovery/README.md).
