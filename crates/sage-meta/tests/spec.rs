#![feature(register_tool)]
#![register_tool(sage)]

use std::collections::BTreeMap;

use sage_meta::Span;
use sage_meta::correspondence::{RustItemKind, RustSite};
use sage_meta::spec::{SpecIndex, SpecSource, render_anchors};

fn source(file: &str, text: &str) -> SpecSource {
    SpecSource {
        file: file.into(),
        source: text.into(),
    }
}

fn test_site() -> RustSite {
    RustSite {
        file: "tests/rules.rs".into(),
        line: 2,
        end_line: 3,
        column: 0,
        end_column: 22,
        rust_path: "test_crate::checks_rule".into(),
        crate_name: "test_crate".into(),
        target: "test:rules".into(),
        kind: RustItemKind::Function,
        cfg: Vec::new(),
        macro_input: false,
        executable: true,
        span: Span { start: 10, end: 60 },
        annotation_span: Span { start: 10, end: 38 },
    }
}

#[test]
fn paragraphs_preserve_prose_and_utf8_source_spans() {
    let text = "# Règles\n\n{anchor}`meta.calls.declared` Every call must be declared.\n\n{anchor}`meta.calls.forbidden`\nAncestors may **forbid** calls, even when cached.\n";
    let index = SpecIndex::from_sources(&[source("rules.md", text)]);
    assert!(index.diagnostics.is_empty(), "{:?}", index.diagnostics);
    assert_eq!(index.documents, 1);
    assert_eq!(index.anchors.len(), 2);
    let declared = &index.anchors["meta.calls.declared"];
    assert_eq!(declared.paragraph, "Every call must be declared.");
    assert_eq!(
        &text[declared.span.start..declared.span.end],
        "{anchor}`meta.calls.declared`"
    );
    assert_eq!(declared.file, "rules.md");
    assert_eq!(
        index.anchors["meta.calls.forbidden"].paragraph,
        "Ancestors may **forbid** calls, even when cached."
    );
}

#[test]
fn code_html_and_escaped_examples_are_not_rules() {
    let text = r#"# Examples

```markdown
{anchor}`example.fenced` Not a requirement.
```

    {anchor}`example.indented` Also code.

Inline `` {anchor}`example.inline` `` is code too.

<div>
{anchor}`example.html_block` HTML content.
</div>

Text <span><em>{anchor}`example.html_inline` HTML content.</em></span>

<!-- {anchor}`example.comment` A comment. -->

\{anchor}`example.escaped` Escaped text.

&#123;anchor}`example.entity` An encoded brace.

{anchor}`meta.real` This is the only requirement.
"#;
    let index = SpecIndex::from_sources(&[source("examples.md", text)]);
    assert!(index.diagnostics.is_empty(), "{:?}", index.diagnostics);
    assert_eq!(index.anchors.keys().collect::<Vec<_>>(), [&"meta.real"]);
}

#[test]
fn malformed_roles_and_empty_or_misplaced_rules_are_errors() {
    for text in [
        "{anchor}missing A rule.",
        "{anchor} `meta.spaced` A rule.",
        "{anchor}``meta.double`` A rule.",
        "{anchor}`meta..empty` A rule.",
        "{anchor}`meta.9number` A rule.",
        "{anchor}`méta.unicode` A rule.",
        "{anchor}`meta::rust` A rule.",
        "{anchor}`meta.raw#name` A rule.",
        "{anchor}`meta.rule`without separator.",
        "{anchor}`meta.empty`\n",
        "{anchor}`meta.empty`\n\nThe rule belongs to another paragraph.",
        "Before {anchor}`meta.misplaced` A rule.",
        "# {anchor}`meta.heading` A heading.",
        "{anchor}`meta.comment` <!-- no rule -->\n",
    ] {
        let index = SpecIndex::from_sources(&[source("broken.md", text)]);
        assert!(!index.diagnostics.is_empty(), "accepted {text:?}");
        assert!(index.anchors.is_empty(), "indexed {text:?}");
        assert_eq!(index.diagnostics[0].file, "broken.md");
    }
}

#[test]
#[sage::test(meta.spec.unique_names)]
fn duplicate_names_are_rejected_across_documents() {
    let index = SpecIndex::from_sources(&[
        source("first.md", "{anchor}`meta.rule` First rule."),
        source("second.md", "{anchor}`meta.rule` Another rule."),
    ]);
    assert_eq!(index.anchors.len(), 1);
    assert_eq!(index.diagnostics.len(), 1);
    assert_eq!(index.diagnostics[0].file, "second.md");
    assert!(index.diagnostics[0].message.contains("duplicate"));
    assert!(index.diagnostics[0].message.contains("first.md"));
    assert_eq!(index.anchors["meta.rule"].file, "first.md");
    assert_eq!(index.validate(&BTreeMap::new()).len(), 2);
}

#[test]
#[sage::test(meta.spec.test_coverage)]
#[sage::test(meta.spec.test_references)]
fn test_coverage_is_bidirectional_and_allows_many_to_many_links() {
    let index = SpecIndex::from_sources(&[source(
        "rules.md",
        "{anchor}`meta.first` First requirement.\n\n{anchor}`meta.second` Second requirement.",
    )]);
    let mut tests = BTreeMap::from([
        ("meta.first".into(), vec![test_site(), test_site()]),
        ("meta.second".into(), vec![test_site()]),
    ]);
    assert!(index.validate(&tests).is_empty());
    tests.get_mut("meta.second").unwrap().clear();
    tests.insert("meta.typo".into(), vec![test_site()]);
    let diagnostics = index.validate(&tests);
    assert_eq!(diagnostics.len(), 2);
    assert_eq!(diagnostics[0].file, "rules.md");
    assert!(diagnostics[0].message.contains("meta.second"));
    assert!(diagnostics[0].message.contains("no associated Rust test"));
    assert_eq!(diagnostics[1].file, "tests/rules.rs");
    assert_eq!(diagnostics[1].span, test_site().annotation_span);
    assert!(
        diagnostics[1]
            .message
            .contains("unknown specification anchor `meta.typo`")
    );
}

#[test]
fn rendering_preserves_markdown_and_exposes_stable_permalinks() {
    let text = "# Rules\n\n{anchor}`meta.rule` A **strong** rule.\n\n```markdown\n{anchor}`example.rule` An example.\n```\n";
    let rendered = render_anchors(text).unwrap();
    assert!(rendered.contains("id=\"meta.rule\" href=\"#meta.rule\""));
    assert!(rendered.contains("aria-label=\"Specification meta.rule\""));
    assert!(rendered.contains("class=\"spec-anchor-label\" aria-hidden=\"true\">spec</span>"));
    assert!(rendered.contains("class=\"spec-anchor-name\">meta.rule</span>"));
    assert!(rendered.contains("</a> A **strong** rule."));
    assert!(rendered.ends_with("```markdown\n{anchor}`example.rule` An example.\n```\n"));
    assert!(render_anchors("{anchor}`meta.empty`\n").is_err());
    assert!(render_anchors("{anchor}`meta.rule` One.\n\n{anchor}`meta.rule` Two.").is_err());
    assert_eq!(render_anchors("Ordinary text.").unwrap(), "Ordinary text.");
}

#[test]
fn loading_recurses_through_markdown_only() {
    let directory = std::env::temp_dir().join(format!("sage-spec-index-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&directory);
    std::fs::create_dir_all(directory.join("nested")).unwrap();
    std::fs::write(directory.join("a.md"), "{anchor}`meta.a` A rule.").unwrap();
    std::fs::write(directory.join("nested/b.md"), "{anchor}`meta.b` B rule.").unwrap();
    std::fs::write(
        directory.join("ignored.rs"),
        "{anchor}`meta.ignored` Ignore.",
    )
    .unwrap();
    let index = SpecIndex::load(&directory).unwrap();
    std::fs::remove_dir_all(&directory).unwrap();
    assert!(index.diagnostics.is_empty(), "{:?}", index.diagnostics);
    assert_eq!(index.documents, 2);
    assert_eq!(index.anchors["meta.b"].file, "nested/b.md");
    assert!(SpecIndex::load(&directory).is_err());
}
