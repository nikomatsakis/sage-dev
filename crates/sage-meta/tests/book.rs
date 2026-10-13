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
                        "path": "design/crates.md", "source_path": "design/crates.md",
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
            let path = chapter["source_path"].as_str().unwrap();
            assert!(result.insert(path.into(), chapter).is_none());
            for (path, child) in chapters(chapter["sub_items"].as_array().unwrap()) {
                assert!(result.insert(path, child).is_none());
            }
        }
    }
    result
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
                "design/crates.md",
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
    assert_eq!(navigation["home"], "design/crates.html");
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
    let escaped_parent = crates.join("unsafe</script><script>");
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
        "design/crates/unsafe%3C/script%3E%3Cscript%3E/demo/crate.html"
    );
}

#[test]
#[sage::test(meta.book.browser_delivery)]
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
            "no containing crate.sage",
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
    let output = fixture.run(&input);
    assert!(output.status.success(), "{:?}", output);
    let book: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(book, input[1]);
    assert!(!fixture.asset().exists());
}
