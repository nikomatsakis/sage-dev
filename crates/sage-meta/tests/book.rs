#![feature(register_tool)]
#![register_tool(sage)]

use std::collections::BTreeMap;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};

use serde_json::{Value, json};

static NEXT: AtomicUsize = AtomicUsize::new(0);

struct Fixture {
    root: PathBuf,
    source: PathBuf,
}

impl Fixture {
    fn new(source: &str) -> Self {
        let root = std::env::temp_dir().join(format!(
            "sage-book-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let fixture = Self {
            source: root.join(source),
            root,
        };
        fixture.write(
            "Cargo.toml",
            "[package]\nname = 'demo'\nversion = '0.0.0'\nedition = '2024'\n\
             [workspace]\nmembers = ['support']\n\
             [dependencies]\nsupport = { path = 'support' }\n",
        );
        fixture.write("book.toml", &format!("[book]\nsrc = '{source}'\n"));
        fixture.doc("SUMMARY.md", "# Summary\n\n- [Design](design/README.md)\n  - [Crates](design/crates/README.md)\n  - [Handwritten](design/handwritten.md)\n");
        fixture.doc("design/README.md", "# Design\n");
        fixture.doc("design/crates/README.md", "# Crates\n");
        fixture.doc("design/handwritten.md", "# Handwritten\nKeep this text.\n");
        fixture.write(
            "src/lib.rs",
            "#[sage::item(crate)] mod architecture {}\n\
             #[sage::item(crate::symbols)] mod symbols {\n\
             #[sage::item(crate::symbols::Item)] pub struct ActualItem;\n}\n",
        );
        fixture.write(
            "support/Cargo.toml",
            "[package]\nname = 'support'\nversion = '0.0.0'\nedition = '2024'\n",
        );
        fixture.write(
            "support/src/lib.rs",
            "#[sage::item(crate)] mod architecture {}\n\
             #[sage::item(crate::Value)] pub struct ActualValue;\n",
        );
        fixture.model(
            "demo/crate.sage",
            "crate demo { \"\"\"Compile demo programs.\"\"\" depends $support; module symbols; }",
        );
        fixture.model(
            "demo/symbols.sage",
            "module crate::symbols;\n\
             struct Item { \"\"\"A compiler symbol.\"\"\" value: $support::Value, }",
        );
        fixture.model(
            "support/crate.sage",
            "crate support { \"\"\"Shared semantic values.\"\"\" }\n\
             type Value \"\"\"A semantic value.\"\"\";",
        );
        fixture
    }

    fn write(&self, relative: &str, text: &str) {
        let path = self.root.join(relative);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
    }

    fn model(&self, relative: &str, text: &str) {
        let path = self.source.join("design/crates").join(relative);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
    }

    fn doc(&self, relative: &str, text: &str) {
        let path = self.source.join(relative);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
    }

    fn input(&self, items_key: &str) -> Value {
        json!([
            {
                "root": self.root,
                "renderer": "html",
                "mdbook_version": "0.5.0",
                "config": {"book": {"src": self.source.strip_prefix(&self.root).unwrap()}},
            },
            {(items_key): [{"Chapter": {
                "name": "Design", "content": "# Design\n", "number": [2],
                "path": "design/README.md", "source_path": "design/README.md",
                "parent_names": [],
                "sub_items": [
                    {"Chapter": {
                        "name": "Crates", "content": "# Crates\n", "number": [2, 1],
                        "path": "design/crates/README.md", "source_path": "design/crates/README.md",
                        "parent_names": ["Design"], "sub_items": [],
                    }},
                    {"Chapter": {
                        "name": "Handwritten", "content": "# Handwritten\nKeep this text.\n",
                        "number": [2, 2], "path": "design/handwritten.md",
                        "source_path": "design/handwritten.md",
                        "parent_names": ["Design"], "sub_items": [],
                    }},
                ],
            }}]},
        ])
    }

    fn run(&self, input: &Value) -> Output {
        let mut child = Command::new(env!("CARGO_BIN_EXE_sage-meta"))
            .arg("preprocess")
            .current_dir(&self.root)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        child
            .stdin
            .take()
            .unwrap()
            .write_all(input.to_string().as_bytes())
            .unwrap();
        child.wait_with_output().unwrap()
    }

    fn book(&self, items_key: &str) -> Value {
        let output = self.run(&self.input(items_key));
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        serde_json::from_slice(&output.stdout).unwrap()
    }

    fn asset(&self) -> PathBuf {
        self.source.join("sage-explorer.html")
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.root).unwrap();
    }
}

fn chapters(items: &[Value]) -> BTreeMap<String, &Value> {
    let mut result = BTreeMap::new();
    for item in items {
        if let Some(chapter) = item.get("Chapter") {
            let path = chapter["source_path"]
                .as_str()
                .or_else(|| chapter["path"].as_str())
                .unwrap();
            assert!(result.insert(path.into(), chapter).is_none());
            for (path, child) in chapters(chapter["sub_items"].as_array().unwrap()) {
                assert!(result.insert(path, child).is_none());
            }
        }
    }
    result
}

#[test]
#[sage::test(meta.book.crates_landing)]
#[sage::test(meta.anchors.book_explorer)]
fn curated_crates_landing_preserves_prose_and_links_to_generated_chapters() {
    let fixture = Fixture::new("manual");
    let landing = "# Crates\n\nStart with [symbols](demo/symbols.sage#item:demo::symbols::Item).\n\nOur handwritten tour.\n";
    fixture.doc("design/crates/README.md", landing);
    fixture.doc(
        "design/crates/demo/guide.md",
        "# Semantic guide\n\nRead [symbols](symbols.sage).\n",
    );
    let mut input = fixture.input("items");
    input[1]["items"][0]["Chapter"]["sub_items"][0]["Chapter"]["content"] = json!(landing);
    let output = fixture.run(&input);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let book: Value = serde_json::from_slice(&output.stdout).unwrap();
    let entries = chapters(book["items"].as_array().unwrap());
    let rendered = entries["design/crates/README.md"]["content"]
        .as_str()
        .unwrap();
    assert!(rendered.contains("Our handwritten tour."));
    assert!(rendered.contains("demo/symbols.html#item:demo::symbols::Item"));
    assert_eq!(rendered, landing.replace(".sage", ".html"));
    assert!(!rendered.contains("<iframe"));
    let crate_children = entries["design/crates/README.md"]["sub_items"]
        .as_array()
        .unwrap();
    assert!(
        crate_children
            .iter()
            .any(|child| { child["Chapter"]["source_path"] == "design/crates/demo/crate.sage" })
    );
    let module_children = entries["design/crates/demo/crate.sage"]["sub_items"]
        .as_array()
        .unwrap();
    assert!(
        module_children
            .iter()
            .any(|child| { child["Chapter"]["source_path"] == "design/crates/demo/symbols.sage" })
    );
    let guide = entries["design/crates/demo/guide.md"];
    assert!(guide["content"].as_str().unwrap().contains("symbols.html"));
    assert_eq!(guide["parent_names"], json!(["Design", "Crates", "demo"]));
    let data = script_data(
        &std::fs::read_to_string(fixture.asset()).unwrap(),
        "book-data",
    );
    assert_eq!(data["home"], "design/crates/index.html");
}

#[test]
#[sage::test(meta.book.shared_model)]
fn companion_guides_use_the_nearest_enclosing_crate_in_inventory_and_book() {
    let fixture = Fixture::new("manual");
    std::fs::rename(
        fixture.source.join("design/crates/support"),
        fixture.source.join("design/crates/demo/support"),
    )
    .unwrap();
    fixture.doc("design/crates/demo/guide.md", "# Demo guide\n");
    fixture.doc("design/crates/demo/support/guide.md", "# Support guide\n");

    let spec = sage_meta::spec::SpecIndex::load_workspace(&fixture.root).unwrap();
    assert!(spec.diagnostics.is_empty(), "{:?}", spec.diagnostics);
    let book = fixture.book("items");
    let entries = chapters(book["items"].as_array().unwrap());
    for (file, owner) in [
        ("design/crates/demo/guide.md", "demo"),
        ("design/crates/demo/support/guide.md", "support"),
    ] {
        assert!(spec.chapters.contains_key(file));
        assert_eq!(entries[file]["source_path"], file);
        assert_eq!(
            entries[file]["parent_names"],
            json!(["Design", "Crates", owner])
        );
    }
}

#[test]
#[sage::test(meta.book.index_namespace_pages)]
#[sage::test(meta.book.index_rows)]
#[sage::test(meta.book.index_groups)]
#[sage::test(meta.book.index_status)]
#[sage::test(meta.anchors.anchor_pages)]
#[sage::test(meta.anchors.context_navigation)]
fn generated_anchor_pages_join_namespaces_claims_implementations_tests_and_reviews() {
    let fixture = Fixture::new("manual");
    fixture.doc("SUMMARY.md", "# Summary\n\n- [Design](design/README.md)\n  - [Crates](design/crates/README.md)\n  - [Meta](design/meta.md)\n  - [Anchors](design/anchor/README.md)\n  - [Handwritten](design/handwritten.md)\n");
    let definitions = "# Meta\n\n{anchor}`foo`\nThe **parent rule** with `code`.\n\n{anchor}`foo.bar`\nThe child rule.\n\n{anchor}`other.leaf`\nA rule in a sibling branch.\n";
    fixture.doc("design/meta.md", definitions);
    fixture.doc("design/anchor/README.md", "# Anchors\n");
    let source = std::fs::read_to_string(fixture.root.join("src/lib.rs")).unwrap();
    fixture.write(
        "src/lib.rs",
        &format!(
            "{source}\n{}",
            r#"
// {anchor}`meta.foo`
fn implementation() {}
#[test]
#[sage::test(meta.foo)]
fn behavior() { assert_eq!(2 + 2, 4); }
mod todo {
    // {anchor}`meta.foo`
    // TODO: Additional implementation and edge case.
    #[test]
    #[sage::test(meta.foo)]
    fn remaining() {}
}
mod not_applicable {
    // {anchor}`meta.foo.bar`<<
    // N/A: This requirement describes an editorial convention.
    #[test]
    #[sage::test(meta.foo.bar)]
    fn editorial() {}
    // >>

    // {anchor}`meta.other.leaf`
    // N/A: This sibling rule is also editorial.
    #[test]
    #[sage::test(meta.other.leaf)]
    fn sibling_editorial() {}
}
"#
        ),
    );
    for anchor in ["meta.foo", "meta.foo.bar", "meta.other.leaf"] {
        sage_meta::review::start(&fixture.root, anchor, vec![]).unwrap();
        if anchor != "meta.other.leaf" {
            sage_meta::review::record(
                &fixture.root,
                anchor,
                &sage_meta::review::ValidationOutput {
                    reviewer: "fixture".into(),
                    report: "Explicit placeholder status reviewed.\n\nLiteral <p> & report detail."
                        .into(),
                    requested_edits: if anchor == "meta.foo.bar" {
                        vec!["Clarify the editorial explanation.".into()]
                    } else {
                        vec![]
                    },
                },
            )
            .unwrap();
        }
    }
    let mut input = fixture.input("items");
    let children = input[1]["items"][0]["Chapter"]["sub_items"]
        .as_array_mut()
        .unwrap();
    children.push(json!({"Chapter":{"name":"Meta","content":definitions,"path":"design/meta.md","source_path":"design/meta.md","number":[2,3],"parent_names":["Design"],"sub_items":[]}}));
    children.push(json!({"Chapter":{"name":"Anchors","content":"# Anchors\n","path":"design/anchor/README.md","source_path":"design/anchor/README.md","number":[2,4],"parent_names":["Design"],"sub_items":[]}}));
    let output = fixture.run(&input);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let book: Value = serde_json::from_slice(&output.stdout).unwrap();
    let entries = chapters(book["items"].as_array().unwrap());
    let overview = entries["design/anchor/README.md"]["content"]
        .as_str()
        .unwrap();
    assert!(overview.contains("data-group=\"meta\""));
    assert!(overview.contains("data-group=\"meta.foo\""));
    assert!(overview.contains("3 anchors"));
    assert!(overview.contains("1 Implemented"));
    assert!(overview.contains("1 TODO"));
    assert!(overview.contains("2 N/A"));
    assert!(overview.contains("aria-controls=\"anchor-text-meta-foo\""));
    assert!(overview.contains("<strong>parent rule</strong>"));
    let page = entries["design/anchor/meta/foo.md"]["content"]
        .as_str()
        .unwrap();
    assert!(page.contains(
        "Explicit placeholder status reviewed.<br><br>Literal &lt;p&gt; &amp; report detail."
    ));
    assert!(!page.contains("<p>Explicit placeholder status reviewed.\n"));
    let table_rows = |content: &str| {
        content
            .split("data-anchor=\"")
            .skip(1)
            .map(|tail| tail.split('"').next().unwrap().to_owned())
            .collect::<Vec<_>>()
    };
    assert_eq!(
        table_rows(overview),
        ["meta.foo", "meta.foo.bar", "meta.other.leaf"]
    );
    for prefix in ["meta", "meta/foo", "meta/other"] {
        assert!(entries.contains_key(format!("design/anchor/{prefix}.md").as_str()));
    }
    assert_eq!(
        table_rows(
            entries["design/anchor/meta.md"]["content"]
                .as_str()
                .unwrap()
        ),
        ["meta.foo", "meta.foo.bar", "meta.other.leaf"]
    );
    assert_eq!(
        table_rows(
            entries["design/anchor/meta/other.md"]["content"]
                .as_str()
                .unwrap()
        ),
        ["meta.other.leaf"]
    );
    assert!(entries.contains_key("design/anchor/meta/other/leaf.md"));
    let leaf = entries["design/anchor/meta/foo.md"];
    assert_eq!(
        leaf["sub_items"].as_array().unwrap().len(),
        1,
        "a rule can also be a namespace"
    );
    assert!(
        leaf["source_path"].is_null(),
        "generated views have no canonical source"
    );
    let content = leaf["content"].as_str().unwrap();
    assert_eq!(table_rows(content), ["meta.foo", "meta.foo.bar"]);
    assert!(content.contains("../../meta.html#meta.foo"));
    assert!(content.contains("fn implementation()"));
    assert!(content.contains("fn behavior() { assert_eq!(2 + 2, 4); }"));
    assert!(content.contains("See more source"));
    assert!(content.contains("Review: <strong>Accepted</strong>"));
    assert!(content.contains("Additional implementation and edge case."));
    let child = entries["design/anchor/meta/foo/bar.md"]["content"]
        .as_str()
        .unwrap();
    assert!(child.contains("N/A: This requirement describes an editorial convention."));
    assert!(child.contains("Review: <strong>Changes requested</strong>"));
    assert!(child.contains("Clarify the editorial explanation."));
    let unfinished = entries["design/anchor/meta/other/leaf.md"]["content"]
        .as_str()
        .unwrap();
    assert!(unfinished.contains("Review: <strong>Unfinished</strong>"));
    assert!(content.contains("data-status=\"TODO\""));
    assert!(child.contains("data-status=\"N/A\""));
    let definition = entries["design/meta.md"]["content"].as_str().unwrap();
    assert!(definition.contains("href=\"anchor/meta/foo.html\""));
    let inventory = sage_meta::spec::SpecIndex::load_workspace(&fixture.root).unwrap();
    assert_eq!(
        inventory.anchors.len(),
        3,
        "rendered copies never become definitions"
    );
}

#[test]
#[sage::test(meta.reviews.claim_text)]
fn anchor_pages_and_disclosures_render_the_complete_reviewed_content() {
    let fixture = Fixture::new("manual");
    fixture.doc("SUMMARY.md", "# Summary\n\n- [Design](design/README.md)\n  - [Crates](design/crates/README.md)\n  - [Handwritten](design/handwritten.md)\n  - [Anchors](design/anchor/README.md)\n");
    fixture.doc("design/anchor/README.md", "# Anchors\n");
    let claim = "The **cached result** is reused.\n\nA second paragraph preserves `identity` with [crate context](crates/README.md).\n\n- Same input\n- Same result\n\n| Input | Result |\n| --- | --- |\n| unchanged | reused |\n\n```rust\nassert_eq!(first, second);\n\n// The blank line belongs to this example.\nassert!(cached);\n```";
    let definitions = format!(
        "# Handwritten\n\n{{anchor}}`multiblock`\n{claim}\n\n<!-- end anchor -->\n\nUnrelated commentary is outside the claim.\n"
    );
    fixture.doc("design/handwritten.md", &definitions);
    let spec = sage_meta::spec::SpecIndex::load_workspace(&fixture.root).unwrap();
    assert!(spec.diagnostics.is_empty(), "{:?}", spec.diagnostics);
    let rust = sage_meta::correspondence::CorrespondenceIndex::load(&fixture.root).unwrap();
    let inputs = sage_meta::review::inventory(&spec, &rust).unwrap();
    let claim_fragment = inputs["handwritten.multiblock"]
        .fragments
        .iter()
        .find(|fragment| {
            matches!(
                &fragment.selector,
                sage_meta::review::Selector::Claim { anchor_id }
                    if anchor_id == "handwritten.multiblock"
            )
        })
        .unwrap();
    assert_eq!(claim_fragment.contents.join("\n"), claim);

    let mut input = fixture.input("items");
    let children = input[1]["items"][0]["Chapter"]["sub_items"]
        .as_array_mut()
        .unwrap();
    children[1]["Chapter"]["content"] = json!(definitions);
    children.push(json!({"Chapter":{"name":"Anchors","content":"# Anchors\n","path":"design/anchor/README.md","source_path":"design/anchor/README.md","number":[2,3],"parent_names":["Design"],"sub_items":[]}}));
    let output = fixture.run(&input);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let book: Value = serde_json::from_slice(&output.stdout).unwrap();
    let entries = chapters(book["items"].as_array().unwrap());
    let page = entries["design/anchor/handwritten/multiblock.md"]["content"]
        .as_str()
        .unwrap();
    let definition = page
        .split_once("<div class=\"anchor-definition\">")
        .unwrap()
        .1
        .split_once("</div>")
        .unwrap()
        .0;
    let overview = entries["design/anchor/README.md"]["content"]
        .as_str()
        .unwrap();
    let disclosure = overview
        .split_once("data-anchor-text=\"handwritten.multiblock\"")
        .unwrap()
        .1
        .split_once("<td colspan=\"3\">")
        .unwrap()
        .1
        .split_once("<p><a href=")
        .unwrap()
        .0;
    for (rendered, crate_href) in [
        (definition, "../../crates/index.html"),
        (disclosure, "../crates/index.html"),
    ] {
        assert!(
            !rendered.contains("\n\n"),
            "raw HTML must survive mdBook's Markdown pass"
        );
        let rendered = rendered.replace("&#10;", "\n");
        assert!(rendered.contains("<p>The <strong>cached result</strong> is reused.</p>"));
        assert!(rendered.contains("<p>A second paragraph preserves <code>identity</code>"));
        assert!(rendered.contains(&format!("href=\"{crate_href}\">crate context</a>")));
        assert!(rendered.contains("<ul>\n<li>Same input</li>\n<li>Same result</li>\n</ul>"));
        assert!(rendered.contains("<table>"));
        assert!(rendered.contains("<th>Input</th><th>Result</th>"));
        assert!(rendered.contains("<td>unchanged</td><td>reused</td>"));
        assert!(rendered.contains(
            "<pre><code class=\"language-rust\">assert_eq!(first, second);\n\n// The blank line belongs to this example.\nassert!(cached);\n</code></pre>"
        ));
        assert!(!rendered.contains("end anchor"));
        assert!(!rendered.contains("Unrelated commentary"));
    }
    assert_eq!(
        definition.replace("../../crates/index.html", "../crates/index.html"),
        disclosure,
        "the page and disclosure present the same claim with page-relative links"
    );
    for (chapter, protected_claim) in [(page, definition), (overview, disclosure)] {
        let events = pulldown_cmark::Parser::new_ext(chapter, pulldown_cmark::Options::all())
            .collect::<Vec<_>>();
        assert!(events.iter().all(|event| !matches!(
            event,
            pulldown_cmark::Event::Start(pulldown_cmark::Tag::CodeBlock(_))
        )));
        assert!(
            events.iter().all(|event| !matches!(
                event,
                pulldown_cmark::Event::Text(text) if text.contains("assert!(cached)")
            )),
            "an example must not escape its generated HTML into Markdown prose"
        );
        let mut final_html = String::new();
        pulldown_cmark::html::push_html(&mut final_html, events.into_iter());
        assert!(
            final_html.contains(protected_claim),
            "mdBook's Markdown pass must retain the complete claim HTML"
        );
    }
    let context = entries["design/handwritten.md"]["content"]
        .as_str()
        .unwrap();
    assert!(context.contains("Unrelated commentary is outside the claim."));
}

#[test]
#[sage::test(meta.anchors.context_navigation)]
fn sage_anchor_links_retain_the_containing_entity_and_exact_prose() {
    let fixture = Fixture::new("manual");
    fixture.model("demo/symbols.sage", "module crate::symbols;\nstruct Item {\n    \"\"\"\n    {anchor}`stable`\n    Every **symbol** keeps its identity.\n    \"\"\"\n    value: $support::Value,\n}\n");
    fixture.book("items");
    let data = script_data(
        &std::fs::read_to_string(fixture.asset()).unwrap(),
        "source-data",
    );
    let anchor = &data["anchors"]["crates.demo.symbols.stable"];
    assert_eq!(anchor["entity"], "demo::symbols::Item");
    assert_eq!(
        anchor["context_url"],
        "design/crates/demo/symbols.html#anchor:crates.demo.symbols.stable"
    );
    let prose = data["prose"]
        .as_object()
        .unwrap()
        .values()
        .flat_map(|file| file.as_object().unwrap().values())
        .filter_map(Value::as_str)
        .collect::<Vec<_>>()
        .join("\n");
    assert!(prose.contains("<strong>symbol</strong>"));
    assert!(prose.contains("id=\"crates.demo.symbols.stable\""));
    assert!(prose.contains("href=\"design/anchor/crates/demo/symbols/stable.html\""));
    assert!(prose.contains("target=\"_parent\""));
}

fn script_data(html: &str, id: &str) -> Value {
    let marker = format!("<script id=\"{id}\" type=\"application/json\">");
    let data = html
        .split_once(&marker)
        .unwrap_or_else(|| panic!("missing {id} script"))
        .1
        .split_once("</script>")
        .unwrap()
        .0;
    serde_json::from_str(data).unwrap()
}

#[test]
#[sage::test(meta.book.sage_chapters)]
#[sage::test(meta.anchors.book_explorer)]
fn generates_crate_and_module_chapters_for_both_mdbook_protocols() {
    for items_key in ["items", "sections"] {
        let fixture = Fixture::new("manual/src");
        let input = fixture.input(items_key);
        let book = fixture.book(items_key);
        let entries = chapters(book[items_key].as_array().unwrap());
        assert_eq!(
            entries.keys().map(String::as_str).collect::<Vec<_>>(),
            [
                "design/README.md",
                "design/crates/README.md",
                "design/crates/demo/crate.sage",
                "design/crates/demo/symbols.sage",
                "design/crates/support/crate.sage",
                "design/handwritten.md",
            ]
        );
        assert_eq!(
            entries["design/handwritten.md"],
            &input[1][items_key][0]["Chapter"]["sub_items"][1]["Chapter"]
        );
        let overview = entries["design/crates/demo/crate.sage"];
        assert_eq!(overview["name"], "demo");
        assert_eq!(overview["number"], json!([2, 1, 1]));
        assert_eq!(overview["parent_names"], json!(["Design", "Crates"]));
        assert_eq!(overview["sub_items"].as_array().unwrap().len(), 1);
        let module = entries["design/crates/demo/symbols.sage"];
        assert_eq!(module["name"], "symbols");
        assert_eq!(module["number"], json!([2, 1, 1, 1]));
        assert_eq!(module["parent_names"], json!(["Design", "Crates", "demo"]));
        for (path, chapter) in entries.iter().filter(|(path, _)| path.ends_with(".sage")) {
            assert_eq!(
                Path::new(chapter["path"].as_str().unwrap()).with_extension("html"),
                Path::new(path).with_extension("html")
            );
            let content = chapter["content"].as_str().unwrap();
            assert!(content.contains("<iframe"), "{path}: {content}");
            assert!(content.contains("data-sage-full-page"));
            assert!(content.contains("../../../sage-explorer.html"));
            assert!(!fixture.source.join(path).with_extension("md").exists());
        }
        let overview_text = overview["content"].as_str().unwrap();
        assert!(overview_text.contains("Compile demo programs."));
        assert!(overview_text.contains("support"));
        let module_text = module["content"].as_str().unwrap();
        assert!(module_text.contains("Item"));
        assert!(module_text.contains("A compiler symbol."));
        assert!(fixture.asset().is_file());
    }
}

#[test]
#[sage::test(meta.book.crate_ownership)]
fn relocating_crate_sources_preserves_model_identity_and_updates_book_routes() {
    let fixture = Fixture::new("manual");
    fixture.book("items");
    let before = std::fs::read_to_string(fixture.asset()).unwrap();
    let before_model = script_data(&before, "model-data");
    let crates = fixture.source.join("design/crates");
    std::fs::create_dir(crates.join("compiler")).unwrap();
    std::fs::rename(crates.join("demo"), crates.join("compiler/frontend")).unwrap();

    let book = fixture.book("items");
    let entries = chapters(book["items"].as_array().unwrap());
    assert!(entries.contains_key("design/crates/compiler/frontend/crate.sage"));
    assert!(entries.contains_key("design/crates/compiler/frontend/symbols.sage"));
    assert!(!entries.contains_key("design/crates/demo/crate.sage"));
    let after = std::fs::read_to_string(fixture.asset()).unwrap();
    let after_model = script_data(&after, "model-data");
    let namespaces = |model: &Value| {
        model
            .as_array()
            .unwrap()
            .iter()
            .map(|document| document["namespace"].as_str().unwrap().to_owned())
            .collect::<std::collections::BTreeSet<_>>()
    };
    assert_eq!(namespaces(&before_model), namespaces(&after_model));
    assert_eq!(
        namespaces(&after_model),
        ["demo", "demo::symbols", "support"]
            .map(String::from)
            .into_iter()
            .collect()
    );
    let entities = |model: &Value| {
        model
            .as_array()
            .unwrap()
            .iter()
            .flat_map(|document| {
                let namespace = document["namespace"].as_str().unwrap();
                document["document"]["declarations"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(move |declaration| {
                        let name = declaration["Type"]["name"].as_str().unwrap();
                        format!("{namespace}::{name}")
                    })
            })
            .collect::<std::collections::BTreeSet<_>>()
    };
    assert_eq!(entities(&before_model), entities(&after_model));
    assert_eq!(
        entities(&after_model),
        ["demo::symbols::Item", "support::Value"]
            .map(String::from)
            .into_iter()
            .collect()
    );
    let before_source = script_data(&before, "source-data");
    let after_source = script_data(&after, "source-data");
    assert_eq!(
        before_source["targets"]["demo::symbols::Item"],
        after_source["targets"]["demo::symbols::Item"]
    );
    assert_eq!(
        after_source["targets"]["demo::symbols::Item"][0]["path"],
        "demo::symbols::ActualItem"
    );
    let navigation = script_data(&after, "book-data");
    assert_eq!(navigation["home"], "design/crates/index.html");
    assert_eq!(
        navigation["chapters"]["demo::symbols"],
        "design/crates/compiler/frontend/symbols.html"
    );
    let module_chapter = entries["design/crates/compiler/frontend/symbols.sage"];
    assert_eq!(
        Path::new(module_chapter["path"].as_str().unwrap()).with_extension("html"),
        Path::new("design/crates/compiler/frontend/symbols.html")
    );
    let module = module_chapter["content"].as_str().unwrap();
    assert!(module.contains("../../../../sage-explorer.html"));
}

#[test]
fn chapter_paths_with_url_delimiters_fail_before_publishing_broken_links() {
    for delimiter in ['#', '?', '%'] {
        let fixture = Fixture::new("manual");
        let crates = fixture.source.join("design/crates");
        let directory = format!("frontend{delimiter}special");
        std::fs::rename(crates.join("demo"), crates.join(&directory)).unwrap();
        let output = fixture.run(&fixture.input("items"));
        assert!(!output.status.success());
        let error = String::from_utf8_lossy(&output.stderr);
        assert!(error.contains(&directory), "{error}");
        assert!(error.contains("chapter path"), "{error}");
        assert!(output.stdout.is_empty());
        assert!(!fixture.asset().exists());
    }
}

#[test]
#[sage::test(meta.book.browser_delivery)]
#[sage::test(meta.anchors.book_explorer)]
fn book_asset_is_self_contained_and_embeds_exact_script_safe_model_and_source() {
    let fixture = Fixture::new("manual");
    let prose = "</script><script>globalThis.compromised = true</script> & prose";
    fixture.model(
        "demo/crate.sage",
        &format!("crate demo {{ \"\"\"{prose}\"\"\" depends $support; module symbols; }}"),
    );
    let rust = format!(
        "// {prose}\n{}",
        std::fs::read_to_string(fixture.root.join("src/lib.rs")).unwrap()
    );
    fixture.write("src/lib.rs", &rust);
    let crates = fixture.source.join("design/crates");
    let escaped_parent = crates.join("nested");
    std::fs::create_dir_all(&escaped_parent).unwrap();
    std::fs::rename(crates.join("demo"), escaped_parent.join("demo")).unwrap();
    fixture.book("items");
    let html = std::fs::read_to_string(fixture.asset()).unwrap();
    assert!(!html.contains("</script><script>globalThis.compromised"));
    assert!(!html.contains("<script src="));
    assert!(!html.contains("<link rel=\"stylesheet\""));
    assert!(!html.contains("<!-- BROWSER_DATA -->"));
    assert!(!html.contains("/* BROWSER_JS */"));
    let model = script_data(&html, "model-data");
    let demo = model
        .as_array()
        .unwrap()
        .iter()
        .find(|document| document["namespace"] == "demo")
        .unwrap();
    assert_eq!(demo["document"]["header"]["Crate"]["purpose"], prose);
    let sources = script_data(&html, "source-data");
    assert_eq!(sources["files"]["src/lib.rs"], rust);
    assert_eq!(
        sources["targets"]["demo::symbols::Item"][0]["path"],
        "demo::symbols::ActualItem"
    );
    let navigation = script_data(&html, "book-data");
    assert_eq!(navigation["chapters"].as_object().unwrap().len(), 3);
    assert_eq!(
        navigation["chapters"]["demo"],
        "design/crates/nested/demo/crate.html"
    );
}

#[test]
#[sage::test(meta.book.browser_delivery)]
#[sage::test(meta.anchors.book_explorer)]
fn unchanged_preprocessing_preserves_asset_timestamp_and_source_edits_refresh_it() {
    let fixture = Fixture::new("manual");
    fixture.book("items");
    let before = std::fs::read(fixture.asset()).unwrap();
    let sentinel = std::time::UNIX_EPOCH + std::time::Duration::from_secs(1_700_000_000);
    std::fs::File::options()
        .write(true)
        .open(fixture.asset())
        .unwrap()
        .set_modified(sentinel)
        .unwrap();
    fixture.book("items");
    assert_eq!(std::fs::read(fixture.asset()).unwrap(), before);
    assert_eq!(
        std::fs::metadata(fixture.asset())
            .unwrap()
            .modified()
            .unwrap(),
        sentinel,
        "rewriting an unchanged source asset would repeatedly trigger mdbook serve"
    );
    fixture.model(
        "demo/symbols.sage",
        "module crate::symbols; struct Item { \"\"\"Updated symbol description.\"\"\" value: $support::Value, }",
    );
    let book = fixture.book("items");
    let after = std::fs::read_to_string(fixture.asset()).unwrap();
    assert_ne!(after.as_bytes(), before);
    assert!(after.contains("Updated symbol description."));
    let entries = chapters(book["items"].as_array().unwrap());
    assert!(
        entries["design/crates/demo/symbols.sage"]["content"]
            .as_str()
            .unwrap()
            .contains("Updated symbol description.")
    );
}

#[test]
#[sage::test(meta.book.browser_delivery)]
#[sage::test(meta.book.crate_ownership)]
fn invalid_models_and_correspondence_fail_before_publishing_an_explorer() {
    for (path, replacement, expected) in [
        (
            "manual/design/crates/demo/symbols.sage",
            "module crate::symbols; struct Item { value: Missing }",
            "unknown type Missing",
        ),
        (
            "src/lib.rs",
            "#[sage::item(crate)] mod architecture {}",
            "has no Rust",
        ),
        (
            "src/lib.rs",
            "#[sage::item(support::Value)] pub struct WrongCrate;",
            "different crate",
        ),
    ] {
        let fixture = Fixture::new("manual");
        fixture.write(path, replacement);
        let output = fixture.run(&fixture.input("items"));
        assert!(!output.status.success());
        let error = String::from_utf8_lossy(&output.stderr);
        assert!(error.contains(expected), "{error}");
        assert!(output.stdout.is_empty());
        assert!(!fixture.asset().exists());
    }
}

#[test]
#[sage::test(meta.book.crate_ownership)]
fn orphan_sage_module_and_nonexistent_rust_crate_are_rejected() {
    for (path, source, expected) in [
        (
            "orphan.sage",
            "module crate::orphan;",
            "no modeled crate owner",
        ),
        (
            "missing/crate.sage",
            "crate missing { \"\"\"No Rust workspace crate owns this model.\"\"\" }",
            "missing",
        ),
    ] {
        let fixture = Fixture::new("manual");
        fixture.model(path, source);
        let output = fixture.run(&fixture.input("items"));
        assert!(!output.status.success());
        let error = String::from_utf8_lossy(&output.stderr);
        assert!(error.contains(expected), "{error}");
        assert!(output.stdout.is_empty());
        assert!(!fixture.asset().exists());
    }
}

#[test]
fn books_without_a_crates_chapter_do_not_load_or_publish_a_model() {
    let fixture = Fixture::new("manual");
    let mut input = fixture.input("items");
    input[1]["items"][0]["Chapter"]["sub_items"]
        .as_array_mut()
        .unwrap()
        .remove(0);
    std::fs::remove_dir_all(fixture.source.join("design/crates")).unwrap();
    fixture.doc(
        "SUMMARY.md",
        "# Summary\n\n- [Design](design/README.md)\n  - [Handwritten](design/handwritten.md)\n",
    );
    let output = fixture.run(&input);
    assert!(output.status.success(), "{:?}", output);
    let book: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(book, input[1]);
    assert!(!fixture.asset().exists());
}
