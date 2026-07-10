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

Concurrent alternatives are different: sibling candidate versions are never
merged into their common parent. Each candidate extracts a canonical response,
after which its version is discarded. This keeps alternative-specific choices
isolated while reusing the same versioning mechanism for rollback.

Because descendants read sparse state through their ancestry, per-version
writes target leaf versions only. Creating a child freezes its parent until
every child is discarded or the sole child is atomically collapsed; read-only
lookup from the frozen parent is allowed, but path compression, variable
allocation, equality/bound changes, rebuild publication, semantic revisions,
and wake publication are not. Append-only global stash allocation is the sole
non-version fact exempt from this rule. This gives each branch a stable
ancestor snapshot without copying the parent maps. The producer arena root used
by trait solving is intentionally frozen for its whole query.

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
