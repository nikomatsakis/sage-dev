// Source: crates/sage-oracle-harness/Cargo.toml
crate sage_oracle_harness {
    """Run conformance fixtures through the independent emitters and compare their serialized results exactly.

{anchor}`exact_comparison`
Conformance is exact identity of independent emissions. Sage
and rustc each produce and validate a complete value in the shared semantic
schema, serialize it deterministically, and pass only when the serialized
bytes are identical. A structural or textual diff may diagnose inequality
but cannot decide equality.

{anchor}`fixed_projection`
Each adapter is a fixed projection into the shared model. An
emitter may translate its native identities, adjustments, and deliberately
deferred lifetime representation, but the projection cannot inspect the
other output or erase a distinction merely because Sage does not yet match
it. Missing shared vocabulary is repaired by extending the schema and both
independent projections.

{anchor}`deterministic_emission`
Determinism is an emitter responsibility. Each side chooses
stable definition identities and semantic ordering before emission,
including forward references. The comparator never sorts, renumbers, or
otherwise repairs nondeterministic or order-dependent output.

{anchor}`independent_coverage`
Equality is accepted only after independent coverage. Each
emitter accounts for every in-scope item and body and rejects forbidden
placeholders before comparison. Paired omission, paired unsupported nodes,
or paired debug fallback values therefore cannot satisfy conformance even if
the remaining serialized bytes agree.

<!-- end anchor -->

    Dependencies below describe workspace crates, including development dependencies; third-party libraries remain outside this model.
    """
    depends $rust_ref;
    depends $sage_emit;
    depends $sage_ir;
    depends $sage_oracle;
    depends $sage_rustc_bridge;
    depends $sage_stash;
    depends $sage_test_harness;
}
