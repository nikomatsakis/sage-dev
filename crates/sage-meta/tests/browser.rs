use sage_meta::{parse, render_browser};

#[test]
fn standalone_export_escapes_script_terminators_in_model_data() {
    let document = parse(
        "demo/crate.sage",
        r#"crate demo {
        """</script><script>globalThis.compromised = true</script> & <img src=x>"""
    }
    type Item """A value.""";
    "#,
    )
    .unwrap();
    let html = render_browser(&[document]).unwrap();
    assert!(!html.contains("</script><script>globalThis.compromised"));
    assert!(html.contains("\\u003c/script>"));
    assert!(html.contains("id=\"model-data\""));
    assert!(!html.contains("<!-- BROWSER_DATA -->"));
    assert!(!html.contains("/* BROWSER_JS */"));
    assert!(!html.contains("<script src="));
    let data = html
        .split("<script id=\"model-data\" type=\"application/json\">")
        .nth(1)
        .unwrap()
        .split("</script>")
        .next()
        .unwrap();
    let value: serde_json::Value = serde_json::from_str(data).unwrap();
    assert_eq!(value[0]["namespace"], "demo");
    assert!(
        value[0]["document"]["header"]["Crate"]["purpose"]
            .as_str()
            .unwrap()
            .contains("</script>")
    );
}

#[test]
fn export_rejects_invalid_and_empty_models() {
    assert!(render_browser(&[]).is_err());
    let document = parse(
        "demo/crate.sage",
        r#"crate demo { """Demo.""" }
        struct Item { missing: Unknown }
    "#,
    )
    .unwrap();
    assert!(
        render_browser(&[document])
            .unwrap_err()
            .to_string()
            .contains("unknown type Unknown")
    );
}

#[test]
fn browse_command_writes_an_explorer_for_the_repository() {
    let output =
        std::env::temp_dir().join(format!("sage-browser-test-{}.html", std::process::id()));
    let workspace = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let root = sage_meta::workspace::default_model_root(&workspace).unwrap();
    let result = std::process::Command::new(env!("CARGO_BIN_EXE_sage-meta"))
        .arg("browse")
        .arg(root)
        .arg(&output)
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let html = std::fs::read_to_string(&output).unwrap();
    std::fs::remove_file(output).unwrap();
    assert!(html.contains("Architecture explorer"));
    assert!(html.contains("LocalFnSym"));
    assert!(html.contains("rust"));
}

#[test]
fn embedded_rust_source_is_script_safe_and_preserves_exact_text() {
    let root = std::env::temp_dir().join(format!("sage-source-export-{}", std::process::id()));
    std::fs::create_dir_all(root.join("src")).unwrap();
    std::fs::write(root.join("Cargo.toml"),
        "[package]\nname = \"source_fixture\"\nversion = \"0.1.0\"\nedition = \"2024\"\n[workspace]\n").unwrap();
    let source = "// </script><script>alert(1)</script> & <!-- SOURCE_DATA -->\n#[sage::item(crate::Item)]\npub struct Item;\n";
    std::fs::write(root.join("src/lib.rs"), source).unwrap();
    let document = parse(
        "source_fixture/crate.sage",
        r#"crate source_fixture { """Fixture.""" }
        type Item """Value.""";"#,
    )
    .unwrap();
    let html = sage_meta::render_browser_with_workspace(&[document], &root).unwrap();
    assert!(!html.contains("</script><script>alert(1)"));
    let data = html
        .split("<script id=\"source-data\" type=\"application/json\">")
        .nth(1)
        .unwrap()
        .split("</script>")
        .next()
        .unwrap();
    let value: serde_json::Value = serde_json::from_str(data).unwrap();
    assert_eq!(value["files"]["src/lib.rs"], source);
    assert_eq!(
        value["targets"]["source_fixture::Item"][0]["locations"][0]["line"],
        2
    );
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn serve_rejects_missing_invalid_or_extra_arguments() {
    for args in [
        vec!["serve"],
        vec!["serve", "abc"],
        vec!["serve", "0"],
        vec!["serve", "65536"],
        vec!["serve", "3001", "sage-src", "extra"],
    ] {
        let output = std::process::Command::new(env!("CARGO_BIN_EXE_sage-meta"))
            .args(&args)
            .output()
            .unwrap();
        assert!(!output.status.success(), "{args:?}");
        let error = String::from_utf8_lossy(&output.stderr);
        assert!(error.contains("port") || error.contains("usage"), "{error}");
    }
}
