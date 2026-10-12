//! Compile an isolated real Salsa database, keeping test-only architecture out
//! of the production corpus and dependency graph.
#[test]
fn real_salsa_contract_fixture() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let fixture = root.join("tests/fixtures/contracts");
    let output = std::process::Command::new(env!("CARGO"))
        .args(["test", "--offline", "--locked", "--manifest-path"])
        .arg(fixture.join("Cargo.toml"))
        .env(
            "CARGO_TARGET_DIR",
            root.join("../../target/sage-contract-fixture"),
        )
        .output()
        .expect("run the isolated contracts fixture");
    assert!(
        output.status.success(),
        "contract fixture failed:\n{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}
