// Source: crates/sage-ir/Cargo.toml
crate sage_ir {
    """Demand-driven semantic frontend. Symbols connect source parsing, expansion, resolution, checked interfaces, inference, trait solving, and completed typed bodies.

{anchor}`semantic_query_boundaries`
Query boundaries follow stable semantic products. Each public
tracked query has a semantic key and returns the narrowest independently
reusable product at that key. Temporary algorithm state is kept inside its
owning query rather than exposed merely to obtain finer-grained memoization.

{anchor}`reexecution_and_invalidation`
Reexecution and downstream invalidation are distinct. A coarse
input change may cause a cheap producer or keyed lookup to execute again.
When its semantic result is equal, backdating must stop the change before
expensive consumers; the architecture does not claim that every unrelated
edit prevents all upstream execution.

{anchor}`incremental_evidence`
Incremental guarantees are verified as executions and semantic
reads, not timings. Evidence distinguishes a tracked function body which
executed from a memoized request and records owned external-metadata reads
separately. Independent scheduling order and elapsed time are excluded
unless a specific ordering or count is itself the invariant.


{anchor}`identity_before_detail`
Semantic identity precedes semantic detail. A definition's
semantic identity is created and retained independently from its signature,
members, fields, body, and other lazily computed facts. Symbol-bearing item
definitions use their symbol; generic parameters, fields, and locals use
their appropriate scoped identity family. Downstream semantic values refer
to that identity rather than reconstructing it from those details. This is
the pipeline-wide consequence of
[Symbols form the uniform semantic identity family](../../../rfds/architectural-foundations/README.md#symbols-form-the-uniform-semantic-identity-family).

{anchor}`narrow_demands`
Demand follows the narrowest semantic boundary. Asking for one
semantic product requests only the interfaces and metadata needed for that
product. In particular, checking a caller may read a callee signature but
never the callee body. This is the pipeline-wide consequence of
[Cross-item dependencies stop at semantic interfaces](../../../rfds/architectural-foundations/README.md#cross-item-dependencies-stop-at-semantic-interfaces).

{anchor}`terminal_incompleteness`
Phase incompleteness is a terminal semantic outcome. A phase
distinguishes a complete result from a conservative result limited by user
error, unsupported functionality, unavailable facts, or an explicit
resource bound. It never presents such a result as computation which merely
needs more scheduling. This is the pipeline-wide consequence of
[Incompleteness is an explicit terminal outcome](../../../rfds/architectural-foundations/README.md#incompleteness-is-an-explicit-terminal-outcome).

<!-- end anchor -->

    Dependencies below describe workspace crates, including development dependencies; third-party libraries remain outside this model.


{anchor}`explicit_restrictions`
A language restriction is explicit at its semantic boundary. An
intentionally unsupported Rust construct states what is excluded, why Sage
excludes it, its user-visible impact, and the response at the responsible
phase. That response is normally a diagnostic or conservative incomplete
result; any temporary semantic approximation, such as `Lifetime::Dummy`, is
identified explicitly as a soundness exception rather than silently
presented as full Rust semantics.

{anchor}`restrictions_and_gaps`
Deliberate restrictions define Sage's supported language contract and explain
its limits. Temporary implementation gaps in otherwise supported Rust appear
as TODO entries on the relevant specification anchors, with their limitations
and evidence. Such gaps do not silently narrow the supported contract; RFDs
record plans for closing them.

{anchor}`represented_completeness`
Unrepresented source never becomes negative evidence. Published
symbols from an incomplete phase may support their own positive facts, but
omitted or unsupported source cannot justify a false proof, failed lookup,
or exhaustive ground `No` in a downstream consumer.
"""
    depends $sage_macros_from_impls;
    depends $sage_reflect;
    depends $sage_stash;
    module types;
    module symbols;
    module modules;
    module checking;
    module resolution;
    module inference;
    module solver;
    module metadata;
}
