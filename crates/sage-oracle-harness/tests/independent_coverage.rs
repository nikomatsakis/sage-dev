#![feature(register_tool, rustc_private)]
#![register_tool(sage)]

#[allow(dead_code)]
#[path = "oracle_compare.rs"]
mod fixture_runner;

#[test]
#[ignore = "TODO: requires generic independent coverage validator; run explicitly to reproduce known gap"]
// Regression for crates.sage_oracle_harness.crate.independent_coverage.
// Ignored tests cannot carry coverage associations; see the eligible TODO below.
fn paired_unsupported_bodies_cannot_satisfy_conformance() {
    let nonce = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let directory = std::env::temp_dir().join(format!(
        "sage-paired-unsupported-{}-{nonce}",
        std::process::id()
    ));
    std::fs::create_dir_all(&directory).unwrap();
    let source = directory.join("paired_unsupported.rs");
    std::fs::write(
        &source,
        "pub fn choose(x: bool) -> u32 { if x { 1 } else { 2 } }\n",
    )
    .unwrap();
    let fixture = sage_oracle_harness::Fixture::SingleFile(source);
    let result = fixture_runner::run_fixture(&fixture, &directory);
    std::fs::remove_dir_all(&directory).unwrap();
    assert!(
        result.is_err(),
        "equal unsupported body placeholders must be rejected before conformance succeeds"
    );
}

mod todo {
    // {anchor}`crates.sage_oracle_harness.crate.independent_coverage`
    // TODO: run_fixture has independent coverage checks for three named fixtures,
    // but lacks generic per-emitter source item/body inventories and rejection of
    // forbidden unsupported/debug placeholders. Implement shared coverage validation
    // and independent source inventories before requiring every fixture to reject
    // paired omissions and placeholders. Keep the existing fixture checks ordinary.

    #[test]
    #[sage::test(crates.sage_oracle_harness.crate.independent_coverage)]
    fn generic_independent_coverage_requires_source_inventories_and_validator() {
        // TODO: Once independent source inventories and the shared coverage validator
        // exist, enable paired_unsupported_bodies_cannot_satisfy_conformance above.
        // Add cases for paired omitted items/bodies and paired debug fallbacks, and
        // require each emitter to fail independently before exact comparison.
        // The ignored expected-rejection regression records the known failure;
        // this explicit TODO is the eligible deferred test, not behavioral coverage.
    }
}
