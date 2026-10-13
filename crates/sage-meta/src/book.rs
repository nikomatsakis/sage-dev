//! mdBook chapters backed by the same model and renderer as the local explorer.
mod anchors;
mod redirects;
use std::collections::BTreeMap;
use std::ffi::OsString;
use std::io::{self, Read};
use std::path::Path;

use sage_meta::{Declaration, Document, Header};
use serde_json::{Value, json};

const CRATES_CHAPTER: &str = "design/crates/README.md";
const CRATES_URL: &str = "design/crates/index.html";
const EXPLORER_ASSET: &str = "sage-explorer.html";

pub fn preprocess(args: &[OsString]) -> Result<(), Box<dyn std::error::Error>> {
    if args.first().is_some_and(|arg| arg == "supports") && args.len() == 2 {
        return if args[1] == "html" || args[1] == "linkcheck2" {
            Ok(())
        } else {
            Err("specification anchors support HTML and linkcheck2 renderers".into())
        };
    }
    if !args.is_empty() {
        return Err("usage: sage-meta preprocess [supports html]".into());
    }
    let mut input = String::new();
    io::stdin().read_to_string(&mut input)?;
    let (context, mut book): (Value, Value) = serde_json::from_str(&input)?;
    // mdBook 0.5 renamed `sections` to `items`; keep the 0.4 Pages build usable.
    let items_key = if book.get("items").is_some() {
        "items"
    } else {
        "sections"
    };
    let sections = book
        .get_mut(items_key)
        .and_then(serde_json::Value::as_array_mut)
        .ok_or("mdBook input must contain book items or sections")?;
    let root = Path::new(
        context
            .get("root")
            .and_then(Value::as_str)
            .ok_or("mdBook context must specify its root")?,
    );
    let spec = sage_meta::spec::SpecIndex::load_workspace(root)?;
    if !spec.diagnostics.is_empty() {
        return Err(spec
            .diagnostics
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join("\n")
            .into());
    }
    render_sections(sections, &spec)?;
    if count_crates_chapters(sections) > 0 {
        if count_crates_chapters(sections) != 1 {
            return Err("the book must contain exactly one design/crates/README.md chapter".into());
        }
        generate_crates(&context, sections, &spec)?;
    }
    if let Some(landing) = find_chapter(sections, anchors::CHAPTER) {
        anchors::generate(root, &spec, landing)?;
    }
    redirects::apply(&context, sections)?;
    println!("{}", serde_json::to_string(&book)?);
    Ok(())
}

fn count_crates_chapters(sections: &[Value]) -> usize {
    sections
        .iter()
        .filter_map(|item| item.get("Chapter"))
        .map(|chapter| {
            usize::from(chapter.get("source_path").and_then(Value::as_str) == Some(CRATES_CHAPTER))
                + chapter
                    .get("sub_items")
                    .and_then(Value::as_array)
                    .map_or(0, |children| count_crates_chapters(children))
        })
        .sum()
}

fn crates_chapter(sections: &mut [Value]) -> Option<&mut Value> {
    find_chapter(sections, CRATES_CHAPTER)
}

fn find_chapter<'a>(sections: &'a mut [Value], source: &str) -> Option<&'a mut Value> {
    for section in sections {
        let Some(chapter) = section.get_mut("Chapter") else {
            continue;
        };
        if chapter.get("source_path").and_then(Value::as_str) == Some(source) {
            return Some(chapter);
        }
        if let Some(children) = chapter.get_mut("sub_items").and_then(Value::as_array_mut)
            && let Some(found) = find_chapter(children, source)
        {
            return Some(found);
        }
    }
    None
}

// {anchor}`meta.book.crates_landing`
// {anchor}`meta.book.crate_ownership`
fn generate_crates(
    context: &Value,
    sections: &mut [Value],
    spec: &sage_meta::spec::SpecIndex,
) -> Result<(), Box<dyn std::error::Error>> {
    let root = Path::new(
        context
            .get("root")
            .and_then(Value::as_str)
            .ok_or("mdBook context must specify its root to generate Crates")?,
    );
    let source = root.join(
        context
            .pointer("/config/book/src")
            .and_then(Value::as_str)
            .unwrap_or("src"),
    );
    let documents = sage_meta::workspace::load_documents(&source.join("design/crates"))?;
    let errors = sage_meta::validate(&documents);
    if !errors.is_empty() {
        return Err(errors
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join("\n")
            .into());
    }
    let index = sage_meta::correspondence::CorrespondenceIndex::load(root)?;
    let errors = index.validate(&documents);
    if !errors.is_empty() {
        return Err(errors
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join("\n")
            .into());
    }

    let mut chapters = BTreeMap::new();
    let mut inventory = BTreeMap::new();
    for document in &documents {
        let namespace = sage_meta::workspace::document_namespace(&documents, document)
            .ok_or("validated document must have a namespace")?;
        let path = Path::new(&document.file).strip_prefix(&source)?;
        // mdBook uses `path` in both output filenames and sidebar hrefs without
        // URL-encoding it. These delimiters would therefore break its own links.
        if url_path(path).contains(['#', '?', '%']) {
            return Err(format!("unsupported URL delimiter in Sage chapter path {}: #, ? and % cannot appear in mdBook chapter paths", path.display()).into());
        }
        chapters.insert(
            namespace.clone(),
            url_path(&path.with_extension("html"))
                .split('/')
                .map(encode_url_component)
                .collect::<Vec<_>>()
                .join("/"),
        );
        inventory.insert(namespace, (document, path));
    }

    let landing = crates_chapter(sections).expect("one Crates chapter was found");
    let mut parents = landing
        .get("parent_names")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    parents.push(landing.get("name").cloned().unwrap_or(json!("Crates")));
    let number = landing
        .get("number")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let mut children = Vec::new();
    for (namespace, (document, path)) in &inventory {
        let name = match &document.header {
            Header::Crate { name, .. } => name,
            Header::Module(_) => continue,
        };
        let mut crate_number = number.clone();
        crate_number.push(json!(children.len() + 1));
        let mut module_parents = parents.clone();
        module_parents.push(json!(name));
        let mut modules = Vec::new();
        for (module_namespace, (module, module_path)) in &inventory {
            match &module.header {
                Header::Crate { .. } => continue,
                Header::Module(_) => {}
            }
            if module_namespace.split("::").next() != Some(namespace) {
                continue;
            }
            let mut module_number = crate_number.clone();
            module_number.push(json!(modules.len() + 1));
            let title = module_namespace
                .strip_prefix(&format!("{namespace}::"))
                .unwrap_or(module_namespace);
            modules.push(chapter(
                title,
                module_path,
                &format!("module:{module_namespace}"),
                module_number,
                &module_parents,
                Vec::new(),
                document_outline(module),
            ));
        }
        // Companion prose belongs to the nearest enclosing modeled crate, just
        // like .sage source. Authors do not maintain a second SUMMARY inventory.
        for (file, prose_chapter) in &spec.chapters {
            if !file.ends_with(".md") || file == CRATES_CHAPTER {
                continue;
            }
            let file_path = Path::new(file);
            let owner = sage_meta::workspace::crate_source_owner(&documents, &source.join(file));
            if owner != Some(namespace.as_str()) {
                continue;
            }
            let prose = &spec.sources[file];
            let title = prose
                .lines()
                .find_map(|line| line.strip_prefix("# "))
                .unwrap_or_else(|| {
                    file_path
                        .file_stem()
                        .and_then(|stem| stem.to_str())
                        .unwrap_or("Guide")
                });
            let mut module_number = crate_number.clone();
            module_number.push(json!(modules.len() + 1));
            let rendered = spec.render_chapter(file, prose).map_err(|errors| {
                errors
                    .iter()
                    .map(ToString::to_string)
                    .collect::<Vec<_>>()
                    .join("\n")
            })?;
            modules.push(json!({"Chapter": {
                "name": title, "content": rendered,
                "path": Path::new(&prose_chapter.url).with_extension("md"),
                "source_path": file, "parent_names": module_parents,
                "number": module_number, "sub_items": [],
            }}));
        }
        children.push(chapter(
            name,
            path,
            &format!("crate:{name}"),
            crate_number,
            &parents,
            modules,
            document_outline(document),
        ));
    }
    landing["sub_items"] = children.into();

    // mdBook copies non-Markdown files from its source tree after preprocessing.
    // Reuse one asset across all chapters. Stable writes avoid serve rebuild loops.
    let html = sage_meta::render_book_browser(&documents, root, CRATES_URL, &chapters)?;
    let asset = source.join(EXPLORER_ASSET);
    if std::fs::read(&asset).ok().as_deref() != Some(html.as_bytes()) {
        std::fs::write(asset, html)?;
    }
    Ok(())
}

fn chapter(
    name: &str,
    source: &Path,
    route: &str,
    number: Vec<Value>,
    parents: &[Value],
    children: Vec<Value>,
    outline: String,
) -> Value {
    json!({"Chapter": {
        "name": name,
        "content": format!("# {}\n\n{}\n<details><summary>Chapter contents</summary>{outline}</details>\n", escape(name), explorer_frame(source, route)),
        "number": number,
        "sub_items": children,
        "path": url_path(&source.with_extension("md")),
        "source_path": url_path(source),
        "parent_names": parents,
    }})
}

// {anchor}`meta.anchors.book_explorer`
// {anchor}`meta.anchors.explorer_navigation`
fn explorer_frame(chapter: &Path, route: &str) -> String {
    let depth = chapter
        .parent()
        .map_or(0, |parent| parent.components().count());
    let asset = format!("{}{EXPLORER_ASSET}", "../".repeat(depth));
    // Model names and route punctuation are all valid query/fragment characters;
    // encoding is also needed for any future identifiers containing URL delimiters.
    let route = encode_url_component(route);
    format!(
        r#"<div class="sage-chapter">
<style>
main:has(.sage-chapter) {{ max-width: none; }}
.content:has(.sage-chapter) {{ padding: 0 15px; }}
body:has(.sage-chapter) .nav-wide-wrapper {{ display:none; }}
.sage-chapter iframe {{ width:100%; height:calc(100vh - 180px); min-height:620px; border:1px solid var(--table-border-color); border-radius:8px; background:var(--bg); }}
.sage-chapter .sage-toolbar {{ display:flex; justify-content:space-between; gap:1em; font-size:.85em; margin:.5em 0; }}
@media print {{ .sage-chapter iframe {{ display:none; }} }}
</style>
<div class="sage-toolbar"><span>Architecture snapshot · search, inspect, and follow dependencies</span><a data-sage-full-page href="{asset}#{route}" target="_blank" rel="noopener">Open full-page explorer ↗</a></div>
<iframe title="Sage architecture explorer" src="{asset}?embed=1&amp;route={route}" loading="eager"></iframe>
</div>"#
    )
}

fn document_outline(document: &Document) -> String {
    let mut html = String::new();
    match &document.header {
        Header::Crate {
            purpose,
            dependencies,
            modules,
            ..
        } => {
            html.push_str(&format!(
                "<p>{}</p><p>Dependencies: {}</p><p>Modules: {}</p>",
                escape(purpose).replace('\n', " "),
                escape(&dependencies.join(", ")),
                escape(&modules.join(", "))
            ));
        }
        Header::Module(_) => {}
    }
    for declaration in &document.declarations {
        let (name, description) = match declaration {
            Declaration::Class(class) => (&class.name, Some(class.description.as_str())),
            Declaration::Type(ty) => (&ty.name, ty.description.as_deref()),
        };
        html.push_str(&format!(
            "<h3>{}</h3><p>{}</p>",
            escape(name),
            escape(description.unwrap_or_default()).replace('\n', " ")
        ));
        match declaration {
            Declaration::Class(class) => {
                for method in &class.methods {
                    html.push_str(&format!(
                        "<p><strong>{}::{}</strong> {}</p>",
                        escape(name),
                        escape(&method.name),
                        escape(&method.description).replace('\n', " ")
                    ));
                }
            }
            Declaration::Type(_) => {}
        }
    }
    html
}

fn url_path(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

fn encode_url_component(text: &str) -> String {
    text.bytes()
        .map(|byte| {
            if byte.is_ascii_alphanumeric() || b"-._~".contains(&byte) {
                (byte as char).to_string()
            } else {
                format!("%{byte:02X}")
            }
        })
        .collect()
}

fn escape(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
        .replace('`', "&#96;")
}

fn render_sections(
    sections: &mut [serde_json::Value],
    spec: &sage_meta::spec::SpecIndex,
) -> Result<(), Box<dyn std::error::Error>> {
    for section in sections {
        let Some(chapter) = section.get_mut("Chapter") else {
            continue;
        };
        let content = chapter
            .get("content")
            .and_then(serde_json::Value::as_str)
            .ok_or("mdBook chapter has no content")?;
        let file = chapter
            .get("source_path")
            .and_then(Value::as_str)
            .unwrap_or("<chapter>");
        let rendered = spec.render_chapter(file, content).map_err(|mut errors| {
            let path = chapter
                .get("source_path")
                .and_then(serde_json::Value::as_str)
                .unwrap_or("<chapter>");
            for error in &mut errors {
                error.file = path.into();
            }
            errors
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>()
                .join("\n")
        })?;
        chapter["content"] = rendered.into();
        if let Some(children) = chapter
            .get_mut("sub_items")
            .and_then(serde_json::Value::as_array_mut)
        {
            render_sections(children, spec)?;
        }
    }
    Ok(())
}
