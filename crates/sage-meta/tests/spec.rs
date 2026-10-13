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
fn claims_preserve_prose_and_utf8_source_spans() {
    let text = "# Règles\n\n{anchor}`meta.language.calls.declared` Every call must be declared.\n\n{anchor}`meta.language.calls.forbidden`\nAncestors may **forbid** calls, even when cached.\n";
    let index = SpecIndex::from_sources(&[source("rules.md", text)]);
    assert!(index.diagnostics.is_empty(), "{:?}", index.diagnostics);
    assert_eq!(index.documents, 1);
    assert_eq!(index.anchors.len(), 2);
    let declared = &index.anchors["meta.language.calls.declared"];
    assert_eq!(declared.content, "Every call must be declared.");
    assert_eq!(
        &text[declared.span.start..declared.span.end],
        "{anchor}`meta.language.calls.declared`"
    );
    assert_eq!(declared.file, "rules.md");
    assert_eq!(
        index.anchors["meta.language.calls.forbidden"].content,
        "Ancestors may **forbid** calls, even when cached."
    );
}

#[test]
#[sage::test(meta.anchors.content.blocks)]
#[sage::test(meta.anchors.content.next_anchor)]
#[sage::test(meta.anchors.content.source_end)]
#[sage::test(meta.anchors.content.explicit_end)]
#[sage::test(meta.anchors.content.unmatched_end)]
fn claims_retain_all_blocks_until_the_next_definition_or_document_end() {
    let body = "Cachés retain **results**.\n\nA later paragraph qualifies the rule.\n\n- Keep unchanged inputs.\n- Recompute edited inputs.\n\n| Input | Result |\n| --- | --- |\n| same | reused |\n\n```rust\nlet résultat = cached();\n```\n\n    An indented example.\n\n> A quoted qualification.";
    let text = format!(
        "# Règles\n\n{{anchor}}`meta.first`\n{body}\n\n{{anchor}}`meta.second` Réévaluate.\n\nFinal qualification.  \n\n"
    );
    let index = SpecIndex::from_sources(&[source("rules.md", &text)]);
    assert!(index.diagnostics.is_empty(), "{:?}", index.diagnostics);
    let first = &index.anchors["meta.first"];
    let second = &index.anchors["meta.second"];
    assert_eq!(first.content, body);
    assert_eq!(second.content, "Réévaluate.\n\nFinal qualification.");
    for anchor in [first, second] {
        assert_eq!(
            &text[anchor.content_span.start..anchor.content_span.end],
            anchor.content
        );
    }
    assert!(first.content_span.end < second.span.start);
}

#[test]
#[sage::test(meta.anchors.content.heading)]
#[sage::test(meta.anchors.content.explicit_end)]
fn every_atx_and_setext_heading_closes_content_until_a_new_definition() {
    // The explicit_end rule also says its marker is optional at another boundary.
    // Every input omits that marker: the heading alone must close the claim
    // without a missing-end error or capturing the following prose.
    for heading in [
        "# Heading",
        "## Heading",
        "### Heading",
        "#### Heading",
        "##### Heading",
        "###### Heading",
        "Heading\n=======",
        "Heading\n-------",
    ] {
        let text = format!(
            "{{anchor}}`meta.before` Opening.\n\nA qualification.\n\n{heading}\n\nOutside prose.\n\n{{anchor}}`meta.after` Fresh claim."
        );
        let index = SpecIndex::from_sources(&[source("headings.md", &text)]);
        assert!(
            index.diagnostics.is_empty(),
            "{heading}: {:?}",
            index.diagnostics
        );
        assert_eq!(
            index.anchors["meta.before"].content,
            "Opening.\n\nA qualification."
        );
        assert_eq!(index.anchors["meta.after"].content, "Fresh claim.");
        assert!(index.anchors["meta.before"].content_span.end < text.find(heading).unwrap());
    }
}

#[test]
#[sage::test(meta.anchors.content.explicit_end)]
fn exact_standalone_comments_end_content_and_allow_fresh_definitions() {
    for end in [
        "<!-- end anchor -->",
        "  <!-- end anchor --> \t",
        "<!-- end anchor -->\r",
    ] {
        let text = format!(
            "{{anchor}}`meta.first` Opening.\n\nFurther detail.\n{end}\nOutside prose.\n\n{{anchor}}`meta.second` Fresh claim.\n<!-- end anchor -->"
        );
        let index = SpecIndex::from_sources(&[source("ends.md", &text)]);
        assert!(
            index.diagnostics.is_empty(),
            "{end:?}: {:?}",
            index.diagnostics
        );
        assert_eq!(
            index.anchors["meta.first"].content,
            "Opening.\n\nFurther detail."
        );
        assert_eq!(index.anchors["meta.second"].content, "Fresh claim.");
        for anchor in index.anchors.values() {
            assert_eq!(
                &text[anchor.content_span.start..anchor.content_span.end],
                anchor.content
            );
        }
    }
}

#[test]
#[sage::test(meta.anchors.content.unmatched_end)]
fn unmatched_ends_are_errors_after_explicit_implicit_and_source_boundaries() {
    for text in [
        "<!-- end anchor -->",
        "{anchor}`meta.first` Claim.\n\n<!-- end anchor -->\n<!-- end anchor -->\n",
        "{anchor}`meta.first` Claim.\n\n# Heading\n\n<!-- end anchor -->\n",
        "{anchor}`meta.first` Claim.\n\nHeading\n-------\n\n<!-- end anchor -->\n",
    ] {
        let index = SpecIndex::from_sources(&[source("ends.md", text)]);
        assert_eq!(
            index.diagnostics.len(),
            1,
            "{text}: {:?}",
            index.diagnostics
        );
        let error = &index.diagnostics[0];
        assert!(error.message.contains("no active anchor"));
        assert_eq!(
            &text[error.span.start..error.span.end],
            "<!-- end anchor -->"
        );
    }
    let index = SpecIndex::from_sources(&[
        source("first.md", "{anchor}`meta.first` Claim."),
        source("second.md", "<!-- end anchor -->"),
    ]);
    assert_eq!(index.diagnostics.len(), 1);
    assert_eq!(index.diagnostics[0].file, "second.md");
    assert_eq!(index.anchors["meta.first"].content, "Claim.");
}

#[test]
#[sage::test(meta.anchors.content.explicit_end)]
#[sage::test(meta.anchors.content.literal_syntax)]
fn malformed_looking_comments_are_content_and_do_not_end_claims() {
    let body = "Opening.\n\n<!--end anchor-->\n\n<!-- end anchor --> trailing text\n\n<!-- end anchor --> <!-- another comment -->\n\nBefore <!-- end anchor -->\n\n<!-- end anchor\n-->\n\n<!-- outer comment\n<!-- end anchor -->\n\nAfter all examples.";
    let text = format!("{{anchor}}`meta.rule` {body}\n\n<!-- end anchor -->\nOutside.");
    let index = SpecIndex::from_sources(&[source("comments.md", &text)]);
    assert!(index.diagnostics.is_empty(), "{:?}", index.diagnostics);
    assert_eq!(index.anchors["meta.rule"].content, body);
}

#[test]
#[sage::test(meta.anchors.content.literal_syntax)]
fn boundary_examples_in_code_escapes_and_enclosing_html_are_literal() {
    let body = r#"Opening.

```markdown
{anchor}`example.fenced` Example.
# Heading
<!-- end anchor -->
```

    {anchor}`example.indented` Example.
    Heading
    -------
    <!-- end anchor -->

`` {anchor}`example.inline` <!-- end anchor --> # Heading ``

\{anchor}`example.escaped` Example.

\# Heading

\<!-- end anchor -->

<div><span title="a > b and </span>">

# HTML heading

{anchor}`example.html` Example.

<!-- end anchor -->

</span></div>

<div
    class="example">

HTML heading
============

{anchor}`example.multiline_html` Example.

<!-- end anchor -->

</div>

Inline <span>

# Inline HTML heading

{anchor}`example.inline_html` Example.

<!-- end anchor -->

</span>

<!-- <div> does not open an enclosing element. -->

The actual final qualification."#;
    let text = format!(
        "{{anchor}}`meta.rule` {body}\n\n<!-- end anchor -->\n\nOutside.\n\n{{anchor}}`meta.fresh` Fresh."
    );
    let index = SpecIndex::from_sources(&[source("literals.md", &text)]);
    assert!(index.diagnostics.is_empty(), "{:?}", index.diagnostics);
    assert_eq!(index.anchors.len(), 2);
    assert_eq!(index.anchors["meta.rule"].content, body);
    assert_eq!(index.anchors["meta.fresh"].content, "Fresh.");
    let examples = SpecIndex::from_sources(&[source("examples.md", body)]);
    assert!(
        examples.diagnostics.is_empty(),
        "{:?}",
        examples.diagnostics
    );
    assert!(examples.anchors.is_empty());
}

#[test]
#[sage::test(meta.anchors.content.literal_syntax)]
fn raw_html_text_does_not_hide_boundaries_after_its_closing_element() {
    for html in [
        "<script>const example = \"<\";</script>",
        "<script>const example = \"<!--\";</script>",
        "<style>.example::before { content: \"<\"; }</style>",
        "<textarea>some < text</textarea>",
        "<pre>some < text</pre>",
        "<div>some < text</div>",
    ] {
        let body = format!("Claim.\n\n{html}");
        let text = format!(
            "{{anchor}}`meta.before` {body}\n\n<!-- end anchor -->\n\n{{anchor}}`meta.after` Following."
        );
        let index = SpecIndex::from_sources(&[source("html.md", &text)]);
        assert!(
            index.diagnostics.is_empty(),
            "{html}: {:?}",
            index.diagnostics
        );
        assert_eq!(index.anchors["meta.before"].content, body);
        assert_eq!(index.anchors["meta.after"].content, "Following.");
    }
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
#[sage::test(meta.anchors.unique_names)]
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
#[sage::test(meta.anchors.test_coverage)]
#[sage::test(meta.anchors.test_references)]
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
#[sage::test(meta.anchors.test_coverage)]
fn implementation_placeholders_do_not_replace_explicit_placeholder_tests() {
    use sage_meta::correspondence::{CorrespondenceIndex, RustSource};
    use sage_meta::review::{TestStatus, inventory};

    let spec = SpecIndex::from_sources(&[source(
        "rules.md",
        "{anchor}`meta.todo` An unfinished behavior.\n\n{anchor}`meta.na` An editorial rule.",
    )]);
    let implementations = r#"
        mod todo {
            // {anchor}`meta.todo`
            // TODO: Implement the unfinished behavior.
            struct Unfinished;
        }
        mod not_applicable {
            // {anchor}`meta.na`
            // N/A: Editorial rules have no runtime implementation.
            struct Editorial;
        }
    "#;
    let mut input = RustSource {
        crate_name: "test_crate".into(),
        target: "test:rules".into(),
        file: "tests/rules.rs".into(),
        source: implementations.into(),
    };
    let implementation_only = CorrespondenceIndex::from_sources(&[input.clone()]).unwrap();
    assert!(implementation_only.diagnostics.is_empty());
    assert!(
        spec.validate_implementations(&implementation_only)
            .is_empty()
    );
    assert!(implementation_only.tests.is_empty());
    let errors = spec.validate(&implementation_only.tests);
    assert_eq!(errors.len(), 2);
    assert!(
        errors
            .iter()
            .all(|error| error.message.contains("no associated Rust test"))
    );

    input.source.push_str(
        r#"
        mod tests {
            mod todo {
                #[test]
                #[sage::test(meta.todo)]
                fn unfinished() {
                    // TODO: Verify the unfinished behavior after implementation.
                }
            }
            mod not_applicable {
                #[test]
                #[sage::test(meta.na)]
                fn editorial() {
                    // N/A: This rule is an editorial requirement reviewed as prose.
                }
            }
        }
    "#,
    );
    let with_tests = CorrespondenceIndex::from_sources(&[input]).unwrap();
    assert!(with_tests.diagnostics.is_empty());
    assert!(spec.validate(&with_tests.tests).is_empty());
    let evidence = inventory(&spec, &with_tests).unwrap();
    assert_eq!(evidence["meta.todo"].tests.len(), 1);
    assert_eq!(evidence["meta.todo"].tests[0].status, TestStatus::Todo);
    assert_eq!(evidence["meta.na"].tests.len(), 1);
    assert_eq!(
        evidence["meta.na"].tests[0].status,
        TestStatus::NotApplicable
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
#[sage::test(meta.anchors.content.explicit_end)]
#[sage::test(meta.anchors.content.literal_syntax)]
fn rendering_removes_only_parsed_end_markers() {
    let text = "{anchor}`claim` A claim.\n\n```html\n<!-- end anchor -->\n```\n\n<div>\n\n<!-- end anchor -->\n\n</div>\n\n<!--end anchor-->\n\n  <!-- end anchor --> \t\n\nOutside prose.\n";
    let fixture = BookFixture::new("- [Rules](design/rules.md)\n");
    fixture.write("design/rules.md", text);
    let index = fixture.load();
    assert!(index.diagnostics.is_empty(), "{:?}", index.diagnostics);
    for rendered in [
        render_anchors(text).unwrap(),
        index.render_chapter("design/rules.md", text).unwrap(),
    ] {
        assert!(!rendered.contains("  <!-- end anchor --> \t"));
        assert_eq!(rendered.matches("<!-- end anchor -->").count(), 2);
        assert!(rendered.contains("```html\n<!-- end anchor -->\n```"));
        assert!(rendered.contains("<div>\n\n<!-- end anchor -->\n\n</div>"));
        assert!(rendered.contains("<!--end anchor-->"));
        assert!(rendered.ends_with("Outside prose.\n"));
    }
    let replacements = index.role_replacements("design/rules.md", text).unwrap();
    let removals = replacements
        .iter()
        .filter(|(_, value)| value.is_empty())
        .collect::<Vec<_>>();
    assert_eq!(removals.len(), 1);
    let (span, _) = removals[0];
    assert_eq!(&text[span.start..span.end], "  <!-- end anchor --> \t");
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

struct BookFixture(std::path::PathBuf);
impl BookFixture {
    fn new(summary: &str) -> Self {
        static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let root = std::env::temp_dir().join(format!(
            "sage-spec-book-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        ));
        std::fs::create_dir_all(root.join("manual/design")).unwrap();
        std::fs::write(root.join("book.toml"), "[book]\nsrc = 'manual'\n").unwrap();
        let result = Self(root);
        result.write("SUMMARY.md", summary);
        result
    }
    fn write(&self, file: &str, text: &str) {
        let path = self.0.join("manual").join(file);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
    }
    fn load(&self) -> SpecIndex {
        SpecIndex::load_workspace(&self.0).unwrap()
    }
}
impl Drop for BookFixture {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.0).unwrap();
    }
}

#[test]
fn markdown_source_links_use_published_readme_urls_in_every_book_section() {
    let fixture = BookFixture::new(
        "# Summary\n\n- [Guide](guide/README.md)\n- [Design](design/README.md)\n  - [Meta](design/meta/README.md)\n  - [Crates](design/crates/README.md)\n  - [Anchors](design/anchor/README.md)\n",
    );
    let design = "# Design\n\n[Crates](crates/README.md#tour), [Meta](meta/README.md), [Anchors](anchor/README.md), and [Guide](../guide/README.md).\n";
    let guide = "# Guide\n\n[Design](../design/README.md#overview), [chapters][design], [design] again, and [source](https://example.com/README.md).\n\n[../design/README.md](../design/README.md \"../design/README.md\")\n\n[design]: ../design/README.md#overview \"../design/README.md#overview\"\n";
    fixture.write("design/README.md", design);
    fixture.write("guide/README.md", guide);
    fixture.write("design/meta/README.md", "# Meta\n");
    fixture.write("design/crates/README.md", "# Crates\n");
    fixture.write(
        "design/crates/demo/crate.sage",
        "crate demo { \"\"\"Demo.\"\"\" }",
    );
    fixture.write("design/anchor/README.md", "# Anchors\n");
    let spec = fixture.load();
    assert!(spec.diagnostics.is_empty(), "{:?}", spec.diagnostics);
    let rendered = spec.render_chapter("design/README.md", design).unwrap();
    assert!(rendered.contains("[Crates](crates/index.html#tour)"));
    assert!(rendered.contains("[Meta](meta/index.html)"));
    assert!(rendered.contains("[Anchors](anchor/index.html)"));
    assert!(rendered.contains("[Guide](../guide/index.html)"));
    let rendered = spec.render_chapter("guide/README.md", guide).unwrap();
    assert!(rendered.contains("[Design](../design/index.html#overview)"));
    assert!(rendered.contains("[design]: ../design/index.html#overview"));
    assert!(rendered.contains("[chapters][design], [design] again"));
    assert!(
        rendered.contains("[../design/README.md](../design/index.html \"../design/README.md\")")
    );
    assert!(
        rendered
            .contains("[design]: ../design/index.html#overview \"../design/README.md#overview\"")
    );
    assert!(rendered.contains("[source](https://example.com/README.md)"));
}

#[test]
#[sage::test(meta.book.chapter_names)]
#[sage::test(meta.book.authority)]
#[sage::test(meta.book.shared_model)]
fn canonical_inventory_uses_configured_chapters_and_local_names() {
    let fixture = BookFixture::new(
        "- [Design](design/README.md)\n  - [Meta](design/meta/README.md)\n  - [Rules](design/meta/call-rules.md)\n  - [Tenets](design/tenets.md)\n- [RFD](rfds/proposal.md)\n",
    );
    fixture.write("design/README.md", "# Design\n");
    fixture.write(
        "design/meta/README.md",
        "{anchor}`authority` Meta is normative.\n",
    );
    fixture.write(
        "design/meta/call-rules.md",
        "{anchor}`declared` Calls are declared.\n",
    );
    fixture.write("design/tenets.md", "{anchor}`not_a_rule` A motivation.\n");
    fixture.write(
        "rfds/proposal.md",
        "{anchor}`not_a_rule` Historical rationale.\n",
    );
    let index = fixture.load();
    assert!(index.diagnostics.is_empty(), "{:?}", index.diagnostics);
    assert_eq!(
        index.anchors.keys().map(String::as_str).collect::<Vec<_>>(),
        ["meta.authority", "meta.call_rules.declared"]
    );
    assert_eq!(index.chapters["design/meta/README.md"].namespace, "meta");
    assert_eq!(
        index.anchors["meta.authority"].context_url,
        "design/meta/index.html#meta.authority"
    );
    assert_eq!(index.documents, 3);
}

#[test]
#[sage::test(meta.book.chapter_names)]
#[sage::test(meta.anchors.unique_names)]
fn chapter_namespace_collisions_and_duplicate_local_rules_are_errors() {
    let fixture =
        BookFixture::new("- [One](design/call-rules.md)\n- [Two](design/call_rules.md)\n");
    fixture.write("design/call-rules.md", "{anchor}`same` First rule.\n");
    fixture.write("design/call_rules.md", "{anchor}`same` Second rule.\n");
    let index = fixture.load();
    assert!(
        index
            .diagnostics
            .iter()
            .any(|error| error.message.contains("namespace `call_rules` collides"))
    );
    assert!(index.diagnostics.iter().any(|error| {
        error
            .message
            .contains("duplicate specification anchor `call_rules.same`")
    }));
    assert!(sage_meta::spec::chapter_namespace("design/bad.name.md").is_err());
    assert!(sage_meta::spec::chapter_namespace("outside/design.md").is_err());
}

#[test]
#[sage::test(meta.book.shared_model)]
fn unpublished_rule_sources_do_not_silently_disappear() {
    let fixture = BookFixture::new("- [Design](design/README.md)\n");
    fixture.write("design/README.md", "# Design\n");
    fixture.write("design/orphan.md", "{anchor}`claim` An unpublished rule.\n");
    let index = fixture.load();
    assert!(index.diagnostics.iter().any(|error| {
        error.file == "design/orphan.md"
            && error
                .message
                .contains("omitted from the Design chapter inventory")
    }));
}

#[test]
#[sage::test(meta.anchors.canonical_sources)]
#[sage::test(meta.anchors.context_navigation)]
#[sage::test(meta.anchors.content.source_end)]
#[sage::test(meta.reviews.claim_text)]
fn sage_prose_rules_have_precise_spans_model_owners_and_cross_source_references() {
    let fixture =
        BookFixture::new("- [Meta](design/meta.md)\n- [Crates](design/crates/README.md)\n");
    fixture.write(
        "design/meta.md",
        "{anchor}`rule` A rule refers to {spec}`crates.demo.nodes.member`.\n",
    );
    fixture.write("design/crates/README.md", "# Crates\n");
    fixture.write(
        "design/crates/demo/crate.sage",
        "crate demo { \"\"\"Demo.\"\"\" module nodes; }",
    );
    let sage = r#"module crate::nodes;
struct Value {
    """
    {anchor}`value`
    A value with **meaning**.

    Précise values survive edits.

    - Keep the cached value.

    ```text
        Preserve relative indentation.
    ```
    """
    """
    {anchor}`member`
    Fields obey {spec}`meta.rule`.
    """
    number: Value,
}
class Node {
    """A node."""
    query value() -> Value {
        """
        {anchor}`query`
        Query results have stable identity.
        """
        envelope {}
    }
}
"#;
    fixture.write("design/crates/demo/nodes.sage", sage);
    let index = fixture.load();
    assert!(index.diagnostics.is_empty(), "{:?}", index.diagnostics);
    let value = &index.anchors["crates.demo.nodes.value"];
    assert_eq!(value.entity.as_deref(), Some("demo::nodes::Value"));
    assert_eq!(
        value.content,
        "A value with **meaning**.\n\nPrécise values survive edits.\n\n- Keep the cached value.\n\n```text\n    Preserve relative indentation.\n```"
    );
    assert_eq!(&sage[value.span.start..value.span.end], "{anchor}`value`");
    assert_eq!(
        &sage[value.content_span.start..value.content_span.end],
        "A value with **meaning**.\n\n    Précise values survive edits.\n\n    - Keep the cached value.\n\n    ```text\n        Preserve relative indentation.\n    ```"
    );
    let rust = sage_meta::correspondence::CorrespondenceIndex::from_sources(&[]).unwrap();
    let inputs = sage_meta::review::inventory(&index, &rust).unwrap();
    assert_eq!(
        inputs["crates.demo.nodes.value"].claim.join("\n"),
        "A value with **meaning**.\n\nPrécise values survive edits.\n\n- Keep the cached value.\n\n```text\n    Preserve relative indentation.\n```"
    );
    assert_eq!(
        index.anchors["crates.demo.nodes.member"].entity.as_deref(),
        Some("demo::nodes::Value::number")
    );
    assert_eq!(
        index.anchors["crates.demo.nodes.member"].content,
        "Fields obey {spec}`meta.rule`."
    );
    assert_eq!(
        index.anchors["crates.demo.nodes.query"].entity.as_deref(),
        Some("demo::nodes::Node::value")
    );
    let query = &index.anchors["crates.demo.nodes.query"];
    assert_eq!(query.content, "Query results have stable identity.");
    assert_eq!(
        &sage[query.content_span.start..query.content_span.end],
        query.content
    );
    assert_eq!(
        value.context_url,
        "design/crates/demo/nodes.html#anchor:crates.demo.nodes.value"
    );
    assert_eq!(index.references.len(), 2);
    for reference in &index.references {
        assert_eq!(
            &index.sources[&reference.file][reference.span.start..reference.span.end],
            format!("{{spec}}`{}`", reference.name)
        );
    }
}

#[test]
#[sage::test(meta.anchors.content.source_end)]
#[sage::test(meta.anchors.content.unmatched_end)]
fn an_end_marker_in_another_sage_prose_block_is_unmatched() {
    let fixture = BookFixture::new("- [Crates](design/crates/README.md)\n");
    fixture.write("design/crates/README.md", "# Crates\n");
    fixture.write(
        "design/crates/demo/crate.sage",
        "crate demo { \"\"\"Demo.\"\"\" module nodes; }",
    );
    let sage = r#"module crate::nodes;
struct Value {
    """
    {anchor}`value` Une règle précise.

    Last qualification.
    """
    """
    <!-- end anchor -->
    """
    number: Value,
}
"#;
    fixture.write("design/crates/demo/nodes.sage", sage);
    let index = fixture.load();
    assert_eq!(
        index.anchors["crates.demo.nodes.value"].content,
        "Une règle précise.\n\nLast qualification."
    );
    assert_eq!(index.diagnostics.len(), 1, "{:?}", index.diagnostics);
    let error = &index.diagnostics[0];
    assert_eq!(error.file, "design/crates/demo/nodes.sage");
    assert!(error.message.contains("no active anchor"));
    assert_eq!(
        &sage[error.span.start..error.span.end],
        "<!-- end anchor -->"
    );
}

#[test]
#[sage::test(meta.anchors.context_navigation)]
fn badges_open_rule_pages_and_keep_definition_permalinks_after_expansion() {
    let fixture = BookFixture::new("- [Rules](design/meta/rules.md)\n");
    fixture.write(
        "design/meta/rules.md",
        "{anchor}`claim` A precise claim.\n\nSee {spec}`meta.rules.claim`.\n",
    );
    let index = fixture.load();
    let rendered = index.render_chapter("design/meta/rules.md", "Expanded preprocessor content.\n\n{anchor}`claim` A precise claim.\n\nSee {spec}`meta.rules.claim`.\n").unwrap();
    assert!(rendered.contains("id=\"meta.rules.claim\" href=\"../anchor/meta/rules/claim.html\""));
    assert!(rendered.contains("class=\"spec-reference\" href=\"../anchor/meta/rules/claim.html\""));
    assert!(rendered.starts_with("Expanded preprocessor content."));
    assert_eq!(
        index.anchors["meta.rules.claim"].context_url,
        "design/meta/rules.html#meta.rules.claim"
    );
}

#[test]
#[sage::test(meta.anchors.canonical_sources)]
fn reference_roles_validate_targets_but_ignore_code_examples() {
    let fixture = BookFixture::new("- [Rules](design/rules.md)\n");
    fixture.write("design/rules.md", "{anchor}`claim` A precise claim.\n\nSee {spec}`missing.rule`.\n\n`` {spec}`example` ``\n\n```text\n{spec}`code.example`\n```\n\n{spec} malformed\n");
    let index = fixture.load();
    assert_eq!(index.references.len(), 1);
    assert!(index.diagnostics.iter().any(|error| {
        error
            .message
            .contains("unknown specification reference `missing.rule`")
    }));
    assert!(
        index
            .diagnostics
            .iter()
            .any(|error| error.message.contains("expected {spec}"))
    );
    assert_eq!(index.diagnostics.len(), 2);
}

#[test]
#[sage::test(meta.anchors.implementation_coverage)]
fn tests_and_model_correspondence_do_not_replace_explicit_implementation_entries() {
    let spec = SpecIndex::from_sources(&[source(
        "rules.md",
        "{anchor}`demo.rule` An independently covered rule.",
    )]);
    let rust = sage_meta::correspondence::CorrespondenceIndex::from_sources(&[sage_meta::correspondence::RustSource {
        crate_name: "demo".into(), target: "lib:demo".into(), file: "src/lib.rs".into(),
        source: "#[sage::item(crate)] mod model {}\n#[test] #[sage::test(demo.rule)] fn evidence() {}".into(),
    }]).unwrap();
    assert!(spec.validate(&rust.tests).is_empty());
    assert_eq!(spec.validate_implementations(&rust).len(), 1);
    assert!(
        spec.validate_implementations(&rust)[0]
            .message
            .contains("no associated Rust implementation")
    );
}

#[test]
#[sage::test(meta.book.shared_model)]
fn generated_crate_guides_are_canonical_and_link_to_rendered_sage_chapters() {
    let fixture = BookFixture::new("- [Crates](design/crates/README.md)\n");
    fixture.write(
        "design/crates/README.md",
        "Explore [Demo](demo/crate.sage#crate:demo).\n",
    );
    fixture.write(
        "design/crates/demo/crate.sage",
        "crate demo { \"\"\"Demo.\"\"\" }",
    );
    fixture.write(
        "design/crates/demo/guide.md",
        "{anchor}`claim` The guide supplements the model.\n",
    );
    let index = fixture.load();
    assert!(index.diagnostics.is_empty(), "{:?}", index.diagnostics);
    assert!(index.anchors.contains_key("crates.demo.guide.claim"));
    let rendered = index
        .render_chapter(
            "design/crates/README.md",
            "Explore [Demo](demo/crate.sage#crate:demo).\n",
        )
        .unwrap();
    assert_eq!(rendered, "Explore [Demo](demo/crate.html#crate:demo).\n");
    assert!(
        index
            .render_chapter("design/crates/README.md", "[Missing](demo/absent.sage)")
            .is_err()
    );
    fixture.write("SUMMARY.md", "- [Design](design/README.md)\n");
    fixture.write("design/README.md", "# Design\n");
    let unpublished = fixture.load();
    assert!(
        unpublished
            .diagnostics
            .iter()
            .any(|error| error.file.ends_with("crate.sage") && error.message.contains("omitted"))
    );
}

#[test]
#[sage::test(meta.book.shared_model)]
#[sage::test(meta.anchors.canonical_sources)]
fn nonnormative_book_chapters_reference_rules_without_defining_them() {
    let fixture = BookFixture::new(
        "- [Design](design/README.md)\n- [Tenets](design/tenets.md)\n- [History](rfds/history.md)\n",
    );
    fixture.write("design/README.md", "{anchor}`rule` A canonical rule.\n");
    fixture.write("design/tenets.md", "See {spec}`rule`.\n");
    fixture.write(
        "rfds/history.md",
        "See {spec}`rule`.\n\n{anchor}`historical` This is not a definition.\n",
    );
    let index = fixture.load();
    assert!(index.diagnostics.is_empty(), "{:?}", index.diagnostics);
    assert_eq!(index.anchors.len(), 1);
    assert_eq!(index.references.len(), 2);
    assert!(!index.chapters["rfds/history.md"].normative);
    assert!(
        index
            .render_chapter("design/tenets.md", "See {spec}`rule`.")
            .unwrap()
            .contains("href=\"anchor/rule.html\"")
    );
    fixture.write("rfds/history.md", "See {spec}`missing.rule`.\n");
    assert!(fixture.load().diagnostics.iter().any(|diagnostic| {
        diagnostic.file == "rfds/history.md"
            && diagnostic
                .message
                .contains("unknown specification reference")
    }));
}

#[test]
#[sage::test(meta.book.chapter_names)]
fn distinct_namespaces_cannot_publish_the_same_output_url() {
    let fixture = BookFixture::new("- [Landing](design/README.md)\n- [Other](design/index.md)\n");
    fixture.write("design/README.md", "# Landing\n");
    fixture.write("design/index.md", "# Other\n");
    let index = fixture.load();
    assert!(index.diagnostics.iter().any(|diagnostic| {
        diagnostic
            .message
            .contains("chapter output URL `design/index.html` collides")
    }));
}
