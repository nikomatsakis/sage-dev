#![feature(register_tool)]
#![register_tool(sage)]

use sage_meta::{parse, render_browser};

#[test]
#[sage::test(meta.anchors.content.explicit_end)]
#[sage::test(meta.anchors.content.literal_syntax)]
fn prose_rendering_hides_control_comments_but_preserves_literal_examples() {
    use sage_meta::spec::{SpecChapter, SpecIndex, SpecSource};
    let source = "{anchor}`demo.rule` A rule.\n\nMore of the rule.\n\n<!-- end anchor -->\n\nOutside commentary.\n\n```html\n<!-- end anchor -->\n```\n\nInline `<!-- end anchor -->`.\n\n<div>\n\n<!-- end anchor -->\n\n</div>\n";
    let mut spec = SpecIndex::from_sources(&[SpecSource {
        file: "design/demo.md".into(),
        source: source.into(),
    }]);
    assert!(spec.diagnostics.is_empty(), "{:?}", spec.diagnostics);
    spec.chapters.insert(
        "design/demo.md".into(),
        SpecChapter {
            file: "design/demo.md".into(),
            namespace: String::new(),
            url: "design/demo.html".into(),
            normative: true,
        },
    );
    let html =
        sage_meta::render_spec_markdown(&spec, "design/demo.md", source, "sage-explorer.html")
            .unwrap();
    assert!(html.contains("More of the rule."));
    assert!(html.contains("Outside commentary."));
    assert!(!html.contains("<!-- end anchor -->"));
    assert_eq!(html.matches("&lt;!-- end anchor --&gt;").count(), 3);
    assert!(html.contains("<code>&lt;!-- end anchor --&gt;</code>"));
    assert!(html.contains("<pre><code class=\"language-html\">&lt;!-- end anchor --&gt;"));
    assert!(html.contains("&lt;div&gt;"));
}

#[test]
fn specification_markdown_links_only_parsed_roles_and_keeps_source_html_inert() {
    use sage_meta::spec::{SpecChapter, SpecIndex, SpecSource};
    let mut spec = SpecIndex::from_sources(&[SpecSource {
        file: "design/demo.md".into(),
        source: "{anchor}`demo.rule` A rule.\n".into(),
    }]);
    spec.chapters.insert(
        "design/demo.md".into(),
        SpecChapter {
            file: "design/demo.md".into(),
            namespace: "demo".into(),
            url: "design/demo.html".into(),
            normative: true,
        },
    );
    let html = sage_meta::render_spec_markdown(
        &spec,
        "design/demo.md",
        r#"
{anchor}`rule` A **rule**.

Read {spec}`demo.rule` and [the guide](guide.md).

Escaped \{spec}`demo.rule` and `literal {spec}`.

```text
{anchor}`rule`
{spec}`demo.rule`
```

<script>alert('inert')</script>
"#,
        "sage-explorer.html",
    )
    .unwrap();
    assert_eq!(html.matches("class=\"spec-anchor\"").count(), 1);
    assert_eq!(html.matches("class=\"spec-reference\"").count(), 1);
    assert!(html.contains("<strong>rule</strong>"));
    assert!(html.contains("<a target=\"_parent\" href=\"design/guide.html\">the guide</a>"));
    assert!(html.contains("{spec}<code>demo.rule</code>"));
    assert!(html.contains("{anchor}&#96;rule&#96;"));
    assert!(!html.contains("{anchor}`"));
    assert!(!html.contains("<script>"));
    assert!(html.contains("href=\"design/anchor/demo/rule.html\""));
}

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
        vec!["serve", "3001", "md/design/crates", "extra"],
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
