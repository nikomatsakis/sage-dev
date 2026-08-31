# Architecture decisions

Cross-cutting decisions that shape the overall system. Feature-local decisions stay in
their RFD and are linked from there.

Each entry has a short code (`D<n>`) for easy cross-reference.

Architecture chapters express these choices as local design anchors with
required verification. A decision records the cross-cutting choice and
rationale; an anchor states the auditable consequence for one phase,
subsystem, or representation. Feature-local choices remain in their RFD until
they acquire a cross-cutting consequence.

## D1: Salsa for incremental computation

We use [salsa](https://github.com/salsa-rs/salsa) as the incremental computation
framework for stable semantic demand boundaries. Source and configuration enter
through Salsa inputs; definitions have tracked or interned identity as
appropriate; and reusable semantic products are tracked functions keyed at the
granularity consumers request.

Temporary inference state, speculative candidates, partially checked
expressions, and formatting helpers are not promoted to tracked queries merely
because they perform substantial work. They remain inside the semantic query
which owns their transaction and publishes their completed result. D2 covers
the separate Stash representation used for type and IR storage.

## D2: Stash for type-level interning

Types, CST, and other tree-shaped IR nodes are allocated in an owning `Stash`
(a custom arena with hash-consing) rather than making every node a Salsa
identity. A `Stashed<T>` query result owns its stash and root as one semantic
value; checking reads source stashes and constructs output in a fresh stash.
Solver proof contexts likewise own the stashes in which their temporary values
are meaningful.

This gives Sage compact handles (`Ptr<T>` and `Slice<T>`) with zero-copy
decomposition inside their owning stash, deterministic rooted fingerprints for
Salsa backdating, and an explicit structural-copy boundary between owners. In
release builds a handle is the four-byte entry index. Debug builds add a
four-byte stash identity, making the handle eight bytes and allowing indexing
to diagnose cross-stash use. Release builds rely on the ownership invariant and
the explicit structural-copy APIs: a handle never acquires database-global
meaning and must not be interpreted by another stash merely because its
numeric index matches.

## D3: Tree-sitter for parsing

We use tree-sitter-rust for parsing rather than writing a hand-rolled parser.
It supplies the Rust grammar and error-recovering concrete tree without Sage
duplicating grammar maintenance. Sage immediately lowers the required syntax
into per-item, stash-owned CST; tree-sitter nodes and their identities do not
cross the parse query boundary.

Tree-sitter can support incremental reparsing, but Sage's semantic incremental
contract is expressed through module inputs, stable item symbols, per-item CST
content, and Salsa backdating. This decision does not require downstream phases
to observe or depend upon tree-sitter's internal node reuse.

## D4: Oracle test harness

End-to-end correctness is validated by an "oracle" harness that compiles test programs
with both Sage and rustc. Each side independently emits the same shared,
deterministically serialized reference IR. The conformance decision is exact
textual identity of those outputs.

The per-side adapters perform only representation changes required to reach
the shared schema. The comparator does not normalize a pair of outputs, erase
known differences, reorder values, strip bodies, or otherwise attempt semantic
equivalence. Validation may reject incomplete output before comparison, and a
diagnostic diff may explain an exact mismatch afterward, but neither changes
the equality rule. Stable external identity includes the actual definition kind
of every path segment; an adapter may not reconstruct ancestor kinds from the
leaf kind or from namespace alone. The detailed contract is [Oracle Test
Harness](./oracle-test-harness.md#thin-adapters-and-exact-comparison).

## D5: Symbols form the uniform semantic identity family

Top-level and associated definitions represented as symbols—functions, types,
impls, modules, constants, enum variants and constructors, and so on—use
kind-specific local or external identities. Erased `Symbol` values permit
heterogeneous ownership and membership, while wrappers such as `FnSymbol` and
`TraitSymbol` recover the static kind required by semantic operations.

Not every semantic identity is an erased `Symbol`. Generic parameters use the
separate `GenericParam` identity family; fields are identified by owner and
index; locals have body-local IDs. Those representations are deliberately
scoped to the structure which owns them and are not converted to `Symbol`
merely for surface uniformity.

Uniformity therefore applies to definitions in the symbol family and their
conversions, not to every identity-bearing IR node, one storage representation,
or one untyped query surface. Kind-specific tracked structs and methods are
intentional: they expose the fields and operations valid for that definition
kind while types and Typed IR refer to each entity through its appropriate
typed identity.

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
and wake publication are not. Append-only allocation in the proof context's
shared `Stash` is the sole non-version fact exempt from this rule. This gives
each branch a stable ancestor snapshot without copying the parent maps.

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

This soundness contract is modulo the explicit temporary lifetime and borrow
checking omission in D12. Type-only and trait-selection uncertainty must still
be represented soundly; D12 does not permit a non-lifetime ambiguity to be
guessed.

## D10: Trait impl discovery is global and trait-keyed

Trait candidate discovery covers every impl visible in the current compilation
through local source and reachable external-crate metadata. The fixed trait is
the mandatory primary query key; the solver does not enumerate impls for every
trait and filter them only during proof execution.

A simplified self-type key may refine lookup when its rigid outer shape is
known. Such an index is conservative: it may return extra candidates but never
omit an applicable specific, blanket, or unclassifiable impl. Indexed and
exhaustive discovery have identical solver meaning. The trait key, and
eventually compatible self-type partitions, also form the semantic lookup
boundary.

The local incremental firewall has two layers. A crate-owned tracked index has
stable identity and a private tracked map containing deterministic trait
buckets plus explicit completeness hazards. Keyed tracked lookup methods are
the only readers of that map. An unrelated impl edit may rebuild the index and
reexecute a cheap lookup for another trait; when that lookup returns the same
bucket, Salsa backdates it. The edit must not propagate into unrelated impl
signature lowering, solver evaluation, normalization, or body checking.
Unknown expansion which could emit an impl for any trait is a global hazard and
legitimately affects every lookup.

The current `local_impl_candidates(LocalCrateSymbol, TraitSymbol)` query is a
trait-keyed local source backed by a provisional linear module-tree scan. It
resolves each impl's trait identity first and lowers a full header only for the
requested trait. Its result carries conservative expansion/header completeness,
and it excludes impls with unrepresented active attribute transformations from
definite candidates. Exact orphan-rule pruning also skips local discovery for
a foreign trait with no trait arguments on a non-fundamental foreign nominal
self type. Because the backing identity scan still depends on the expanded
module tree, the destination incremental firewall is not yet built: an
unrelated trait impl edit can propagate through the current candidate query
despite producing the same candidates.
Reachable external-crate coverage and conservative external self-head
partitioning are built; trait-partitioned local source dependencies and their
edit-invalidation matrix remain outstanding. The destination and required
conformance tests live in
[Trait Solver Design](./trait-solver.md) and the
[Trait Impl Candidate Discovery RFD](../rfds/trait-impl-candidate-discovery/README.md).

## D11: Completed bodies are elaborated typed trees

`LocalFnSym::body` returns a fully typed, fully resolved, tree-structured IR.
Implicit type-directed operations such as receiver dereference, autoref,
coercion, and unsizing are materialized as nodes. Method syntax, unresolved
field names, inference variables, and adjustment lists do not survive in a
successful completed body.

Structured control flow, closures, and async bodies remain above MIR: the IR
does not introduce basic blocks, drop schedules, or coroutine state-machine
layout. Candidate selection may use source-shaped nodes and adjustment recipes
internally, but those are consumed inside the body query. The full destination
contract is [Typed IR](./typed-ir.md).

## D12: Lifetimes collapse to `Dummy` and borrow checking is deferred

Every explicit, elided, universal, existential, imported, and synthesized
lifetime immediately becomes `Lifetime::Dummy` during checking. Sage does not
create lifetime inference variables. `Outlives(Dummy, Dummy)` is true, and
lifetime relationships cannot reject a program.

Borrow and dereference operations remain explicit in typed IR because they
affect ordinary types and calls, but their validity is not checked. This is a
known temporary soundness hole. A dedicated variant is used instead of
`'static` or post-analysis erasure so the omission is visible and its eventual
removal is mechanically enforced. A future unified type-and-lifetime
inference design supersedes this decision.

## D13: Named, associated, and opaque aliases share one semantic family

The type representation distinguishes rigid types from alias types. Rigid
constructors such as `Vec` are structural and do not normalize. `AliasTy` has
three semantic variants: `Named` for a user-defined type alias, `Associated`
for an associated-type projection, and `Opaque` for an opaque type. Every
alias retains its definition identity and generic arguments.

Normalization is a semantic operation, not an eager erasure pass. Named aliases
reveal their substituted right-hand sides infallibly. Associated types normalize
through trait matching. Opaques reveal only within their definition boundary
or in a future code-generation mode. The typing/reveal context is consequently
part of the normalization query's semantic input. D14 specifies how that
operation returns its result.

An unrevealed alias is not devoid of facts. Bounds declared on associated and
opaque types may prove predicates without normalization, and identical alias
applications may be related structurally. The complete destination contract
is split between [Typed IR](./typed-ir.md#rigid-and-alias-types) and [Trait
Solver Design](./trait-solver.md#alias-types-and-normalization).

## D14: Solver operations return goal-specific semantic outputs

The solver distinguishes value-producing operations from the proposition
language used for assumptions and residual conditions. A proof operation
returns `GoalOutput::Proven`; normalization takes only an alias as input and
returns `GoalOutput::Type`. A successful response carries that output alongside
its substitution and residual proof goal.

Normalization is therefore modeled as `Normalize(alias) -> Type`, not as the
predicate `NormalizesTo(alias, caller_output)` and not as a convention where a
fresh output inference variable must be recovered from the response
substitution. The caller relates the returned type to any expected type only
after candidate answers have been merged, so its expectation cannot select an
otherwise ambiguous candidate.

The output is part of the canonical response: its response-local variables are
bound, copied, occurs-checked, universe-checked, cached, compared, and merged.
An unconditional `Proven` answer can absorb sibling proofs, but an unconditional
type result cannot absorb a sibling which may return a different type unless
candidate priority or output equivalence justifies it.

`GoalOutput` is extensible for future non-type operations such as callable
instance resolution or vtable construction. Those representations remain
unsettled; this decision only prevents them from being forced through the type
variant. Trait implementation proof continues to return `Proven`, never a
selected impl identity. See [Trait Solver Design](./trait-solver.md#knowledge-returned-by-the-solver)
and the [Associated Type Normalization RFD](../rfds/associated-type-normalization/README.md).

## D15: Cross-item dependencies stop at semantic interfaces

A definition's symbol is the stable key for its semantic products. Its checked
signature is the primary interface other definitions consume; field, member,
and other language-required interfaces may be separate narrow keyed products.
Its body and checking temporaries are not interfaces. A caller may read the
callee's signature and relevant trait or metadata facts, but never the callee
body. Signature checking likewise does not eagerly check the definition's
body.

This boundary applies to dependency meaning even when a provisional coarse
input causes extra execution. If reexecution produces an equal public semantic
value, Salsa backdating stops propagation before unrelated downstream
consumers. Narrower future inputs should reduce that extra execution without
changing the semantic boundary.

This choice lets Sage type-check one body without checking the crate eagerly,
makes interface-preserving edits reusable, and gives query-trace tests a clear
set of forbidden dependencies. Its chapter-level consequences are
[ARC-A2](./architecture.md#arc-a2), [TEN-A1](./tenets.md#ten-a1), and
[TEN-A5](./tenets.md#ten-a5).

## D16: Incompleteness is an explicit terminal outcome

A phase or semantic subsystem distinguishes a complete result from a
conservative result limited by invalid or ambiguous source, unsupported Sage
functionality, unavailable external facts, or an explicit resource bound.
Such incompleteness is terminal for the current inputs and limits: continuing
to poll or schedule the same computation is not expected to make it complete.

An incomplete result may retain partial information for diagnostics or
conservative recovery, but it must identify which completeness guarantee is
absent and must not be presented as a successful complete value. Resource
exhaustion and ambiguity also remain distinct from a logical negative answer.
Repeating the operation may produce a different outcome only after an input,
environment, or configured limit changes.

This decision separates semantic recovery from asynchronous progress and
prevents downstream consumers from treating missing work as negative
evidence. The pipeline-wide consequence and verification contract is
[ARC-A3](./architecture.md#arc-a3); solver-specific resource and groundness
semantics remain in [D9](#d9-trait-solving-is-groundness-sensitive-and-resource-bounded).

## D17: Nested spans are relative to stable item provenance

Each source-root item carries an absolute range in a `ParseSource`. A nested
definition carries its placement relative to its immediate owner, and syntax
or Typed IR inside any represented item carries byte ranges relative to that
item's own start. Resolving a nested range composes the owner chain to the
source root and preserves the same parse-source identity. For example, an
associated method is relative to its trait or impl, while the method's body is
relative to the method. Generated parse sources use stable occurrence identity
while their generated text and current origin coordinates remain tracked facts.

This split keeps position lookup and diagnostics accurate while preventing an
offset-only edit before an otherwise unchanged item, including an edit to an
earlier sibling associated item, from changing every span inside its semantic
content. Queries which need source positions may observe the composed absolute
span; ordinary signature and body semantics should depend on the relative
content instead.

The representation and edit-verification requirements are
[SPAN-A1](./spans.md#span-a1), [SPAN-A2](./spans.md#span-a2), and
[SPAN-A3](./spans.md#span-a3). The historical rationale is recorded in the
[Relative Span Model RFD](../rfds/relative-span-model/README.md).

## D18: External providers supply facts, not Sage semantic answers

Rustc-backed providers may authoritatively expose facts owned by reachable
dependency crates and the compilation environment: definition identity,
ownership, signatures, predicates, associated items and values, impl headers,
and other exported metadata. Sage lowers those facts into its own symbol,
type, stash, and query representations at narrow keyed boundaries.

The provider does not resolve Sage-local source, select a method or impl for a
Sage body, prove a Sage trait goal, normalize an alias on Sage's behalf, or
produce Sage's checked body. Those operations remain in Sage even when their
inputs include authoritative external facts. Likewise, the rustc side of the
oracle is an independent comparison producer, not a hidden semantic service
used to manufacture Sage's result.

This boundary makes external metadata reusable without masking gaps in Sage's
semantics or creating accidental whole-crate dependencies. Its local contract
is [META-A1](./subsystems/external-metadata.md#meta-a1); exact oracle
independence remains [D4](#d4-oracle-test-harness).

## D19: Semantic evidence starts from source

When a semantic behavior can be produced by a Rust program, integration and
acceptance evidence starts from a checked-in Cargo project and traverses the
production layers named by the claim. Snapshots, query traces, and rendered
review packets are observations of that execution. They are never loaded as
precomputed semantic answers which replace parsing, expansion, resolution,
checking, inspection, or transport.

Constructed values remain useful for focused unit tests, and a narrow test
double may inject an operating-system or external-service failure which cannot
be reproduced safely or deterministically. Such a test establishes only that
boundary. It cannot establish a Sage semantic anchor, and a transport test
over a scripted semantic provider cannot be presented as end-to-end evidence.

This decision makes the authority of a test visible: Rust projects are inputs,
reviewed snapshots are outputs, and production code performs the semantic
work between them. Its general verification contract is
[TEN-A7](./tenets.md#ten-a7); the Semantic Inspector applies it in
[SI-A3](./validation/semantic-inspector.md#si-a3).
