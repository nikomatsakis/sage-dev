//! Compile an isolated real Salsa database, keeping test-only architecture out
//! of the production corpus and dependency graph.
#![feature(register_tool)]
#![register_tool(sage)]

#[test]
#[sage::test(meta.language.calls.declared)]
#[sage::test(meta.language.calls.forbidden)]
#[sage::test(meta.contracts.validated_session)]
#[sage::test(meta.contracts.cache_invocations)]
#[sage::test(meta.contracts.observation_coverage)]
fn real_salsa_contract_fixture() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let fixture = root.join("tests/fixtures/contracts");
    let output = std::process::Command::new(env!("CARGO"))
        .args(["test", "--offline", "--locked", "--manifest-path"])
        .arg(fixture.join("Cargo.toml"))
        .args(["--", "--nocapture", "--test-threads=1"])
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
    let stdout = String::from_utf8(output.stdout).unwrap();
    let evidence: Vec<serde_json::Value> = stdout
        .lines()
        .filter_map(|line| line.split_once("SAGE_CONTRACT_EVIDENCE:"))
        .map(|(_, json)| serde_json::from_str(json).unwrap())
        .collect();
    assert_eq!(evidence.len(), 2, "expected cold and prewarmed reports");
    let root = "sage_contract_fixture::nodes::Root::run";
    let helper = "sage_contract_fixture::nodes::Helper::work";
    let leaf = "sage_contract_fixture::nodes::Leaf::read";
    for prewarmed in [false, true] {
        let sample = evidence
            .iter()
            .find(|sample| sample["prewarmed"] == prewarmed)
            .expect("each cache scenario must report evidence");
        let operations = &sample["model"]["operations"];
        assert!(operations[helper]["invokes"].as_array().unwrap().is_empty());
        assert_eq!(
            operations[root]["prohibits"][0]["targets"],
            serde_json::json!([leaf])
        );
        let report = &sample["contracts"];
        assert_eq!(report["calls_conform"], false);
        assert_eq!(report["coverage"]["checked_requests"], 3);
        assert!(report["coverage"]["gaps"].as_array().unwrap().is_empty());
        let violations = report["violations"].as_array().unwrap();
        assert_eq!(violations.len(), 2);
        assert_eq!(violations[0]["kind"], "UndeclaredCall");
        assert_eq!(violations[1]["kind"], "ForbiddenTransitive");
        assert_eq!(violations[1]["rule"]["operation"], root);
        assert_eq!(
            violations[1]["path"]
                .as_array()
                .unwrap()
                .iter()
                .map(|frame| frame["operation"].as_str().unwrap())
                .collect::<Vec<_>>(),
            [root, helper, leaf],
        );
        if prewarmed {
            let events = sample["query_events"].as_array().unwrap();
            assert!(events.iter().any(|event| {
                let event = event.as_str().unwrap();
                event.starts_with("return Leaf::read") && event.contains("[Reused]")
            }));
            assert!(
                !events
                    .iter()
                    .any(|event| event.as_str().unwrap().starts_with("execute Leaf::read"))
            );
        }
    }
}
