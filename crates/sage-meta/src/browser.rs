use crate::Document;
use std::collections::BTreeMap;
use std::path::Path;

#[derive(serde::Serialize)]
struct BrowserDocument<'a> {
    namespace: String,
    document: &'a Document,
}

/// Build a self-contained HTML explorer from a valid model. No network requests
/// or source-code execution are needed when the resulting file is opened.
pub fn render_browser(documents: &[Document]) -> Result<String, Box<dyn std::error::Error>> {
    render(documents, None, None)
}

/// Include source snapshots from the supplied Cargo workspace.
pub fn render_browser_with_workspace(
    documents: &[Document],
    workspace: &Path,
) -> Result<String, Box<dyn std::error::Error>> {
    render(documents, Some(workspace), None)
}

/// Render the shared book asset with links back to the owning source chapters.
/// Paths in `chapters` and `home` are relative to the asset at the book root.
pub fn render_book_browser(
    documents: &[Document],
    workspace: &Path,
    home: &str,
    chapters: &BTreeMap<String, String>,
) -> Result<String, Box<dyn std::error::Error>> {
    let book = serde_json::json!({"home": home, "chapters": chapters});
    render(documents, Some(workspace), Some(&book))
}

fn render(
    documents: &[Document],
    workspace: Option<&Path>,
    book: Option<&serde_json::Value>,
) -> Result<String, Box<dyn std::error::Error>> {
    let errors = crate::validate(documents);
    if !errors.is_empty() {
        return Err(errors[0].clone().into());
    }
    if documents.is_empty() {
        return Err("cannot browse an empty model".into());
    }
    let data = documents
        .iter()
        .map(|document| {
            let namespace = crate::workspace::document_namespace(documents, document)
                .expect("validated document has a namespace");
            BrowserDocument {
                namespace,
                document,
            }
        })
        .collect::<Vec<_>>();
    let sources = source_data(&data, workspace)?;
    let mut sources = sources;
    if book.is_some()
        && let Some(workspace) = workspace
    {
        let spec = crate::spec::SpecIndex::load_workspace(workspace)?;
        let book_root = crate::workspace::book_source_root(workspace)?;
        for document in documents {
            let file = Path::new(&document.file)
                .strip_prefix(&book_root)?
                .to_string_lossy()
                .replace('\\', "/");
            for block in &document.prose {
                sources
                    .prose
                    .entry(document.file.clone())
                    .or_default()
                    .insert(
                        block.text.clone(),
                        render_spec_markdown(
                            &spec,
                            &file,
                            &crate::spec::prose_markdown(&block.text),
                            "sage-explorer.html",
                        )
                        .map_err(|errors| {
                            errors
                                .iter()
                                .map(ToString::to_string)
                                .collect::<Vec<_>>()
                                .join("\n")
                        })?,
                    );
            }
        }
        for anchor in spec
            .anchors
            .values()
            .filter(|anchor| anchor.entity.is_some())
        {
            sources.anchors.insert(anchor.name.clone(), anchor.clone());
        }
    }
    let source_json = serde_json::to_string(&sources)?
        .replace('<', "\\u003c")
        .replace('&', "\\u0026");
    // Even non-executable JSON script elements terminate on a literal </script>.
    let json = serde_json::to_string(&data)?
        .replace('<', "\\u003c")
        .replace('&', "\\u0026");
    let template = include_str!("../web/index.html");
    let book_json = serde_json::to_string(&book)?
        .replace('<', "\\u003c")
        .replace('&', "\\u0026");
    let html = template
        .replace("/* BROWSER_CSS */", include_str!("../web/style.css"))
        .replace("/* BROWSER_JS */", include_str!("../web/browser.js"));
    // Substitute data only after assets, so source text is never treated as a template.
    let (prefix, rest) = html
        .split_once("<!-- BROWSER_DATA -->")
        .expect("model placeholder");
    let (middle, suffix) = rest
        .split_once("<!-- SOURCE_DATA -->")
        .expect("source placeholder");
    // The navigation configuration precedes the browser script, which reads it
    // during initialization. Never substitute template tokens inside user data.
    Ok(format!(
        "{prefix}{json}{middle}{source_json}</script><script id=\"book-data\" type=\"application/json\">{book_json}{suffix}"
    ))
}

#[derive(Default, serde::Serialize)]
struct SourceData {
    files: BTreeMap<String, String>,
    targets: BTreeMap<String, Vec<SourceReference>>,
    warnings: Vec<String>,
    prose: BTreeMap<String, BTreeMap<String, String>>,
    anchors: BTreeMap<String, crate::spec::SpecAnchor>,
}

/// Render specification prose with safe Markdown, rule links, and book-relative URLs.
/// Generated badges are inserted after Markdown rendering, so raw source HTML
/// cannot acquire executable meaning in an exported architecture snapshot.
pub fn render_spec_markdown(
    spec: &crate::spec::SpecIndex,
    file: &str,
    text: &str,
    page_url: &str,
) -> Result<String, Vec<crate::Diagnostic>> {
    use pulldown_cmark::{Event, Options, Parser, Tag};
    let source_url = crate::spec::chapter_url(file);
    // Parsed roles become opaque tokens during Markdown rendering. This keeps
    // escaped markers and code examples literal while preserving only the HTML
    // produced by the shared specification renderer.
    let mut prefix = String::from("SAGERENDERROLE");
    while text.contains(&prefix) {
        prefix.push('X');
    }
    let mut protected = String::new();
    let mut replacements = Vec::new();
    let mut cursor = 0;
    for (span, mut role) in spec.role_replacements(file, text)? {
        protected.push_str(&text[cursor..span.start]);
        cursor = span.end;
        if role.is_empty() {
            continue;
        }
        let token = format!("{prefix}{}END", replacements.len());
        protected.push_str(&token);
        if let Some(start) = role.find("href=\"") {
            let start = start + 6;
            if let Some(end) = role[start..].find('"') {
                let href = prose_href(&source_url, page_url, &role[start..start + end]);
                role.replace_range(start..start + end, &href);
            }
        }
        role = role.replacen("<a ", "<a target=\"_parent\" ", 1);
        replacements.push((token, role));
    }
    protected.push_str(&text[cursor..]);
    let events = Parser::new_ext(&protected, Options::all()).map(|event| match event {
        Event::Html(value) | Event::InlineHtml(value) => Event::Text(value),
        Event::Start(Tag::Link {
            link_type,
            dest_url,
            title,
            id,
        }) => {
            let href = prose_href(&source_url, page_url, &dest_url);
            Event::Start(Tag::Link {
                link_type,
                dest_url: href.into(),
                title,
                id,
            })
        }
        Event::Start(Tag::Image {
            link_type,
            dest_url,
            title,
            id,
        }) => {
            let href = prose_href(&source_url, page_url, &dest_url);
            Event::Start(Tag::Image {
                link_type,
                dest_url: href.into(),
                title,
                id,
            })
        }
        other => other,
    });
    let mut html = String::new();
    pulldown_cmark::html::push_html(&mut html, events);
    // Prose may link to a guide, RFD, or another source chapter. Keep the book
    // shell as the navigation owner when this HTML appears inside its iframe.
    html = html.replace("<a href=", "<a target=\"_parent\" href=");
    for (token, role) in replacements {
        html = html.replace(&token, &role);
    }
    // Legacy excerpt preprocessing scans raw HTML for role-shaped source text.
    // Entity spelling leaves that source visible without reinterpreting it.
    Ok(html.replace('`', "&#96;"))
}

fn prose_href(source: &str, page: &str, href: &str) -> String {
    if let Some((scheme, _)) = href.split_once(':')
        && !scheme.contains(['/', '#', '?'])
    {
        return if ["http", "https", "mailto"].contains(&scheme.to_ascii_lowercase().as_str()) {
            href.into()
        } else {
            String::new()
        };
    }
    if href.starts_with('/') {
        return href.into();
    }
    let target = if href.starts_with('#') {
        format!("{source}{href}")
    } else {
        let directory = source
            .rsplit_once('/')
            .map_or("", |(directory, _)| directory);
        let (path, fragment) = href
            .split_once('#')
            .map_or((href, None), |(path, fragment)| (path, Some(fragment)));
        let path = if path.ends_with(".md") || path.ends_with(".sage") {
            crate::spec::chapter_url(path)
        } else {
            path.into()
        };
        format!(
            "{directory}/{path}{}",
            fragment.map_or(String::new(), |fragment| format!("#{fragment}"))
        )
    };
    let (path, fragment) = target
        .split_once('#')
        .map_or((target.as_str(), None), |(path, fragment)| {
            (path, Some(fragment))
        });
    let mut parts = Vec::new();
    for part in path.split('/') {
        match part {
            "." | "" => {}
            ".." => {
                parts.pop();
            }
            part => parts.push(part),
        }
    }
    let target = format!(
        "{}{}",
        parts.join("/"),
        fragment.map_or(String::new(), |fragment| format!("#{fragment}"))
    );
    crate::spec::relative_url(page, &target)
}
#[derive(serde::Serialize)]
struct SourceReference {
    path: String,
    locations: Vec<crate::correspondence::RustSite>,
}

// {anchor}`meta.anchors.evidence_status`
fn source_data(
    _documents: &[BrowserDocument<'_>],
    workspace: Option<&Path>,
) -> Result<SourceData, Box<dyn std::error::Error>> {
    let Some(workspace) = workspace else {
        return Ok(SourceData::default());
    };
    let index = crate::correspondence::CorrespondenceIndex::load(workspace)?;
    let mut data = SourceData {
        warnings: index.diagnostics.iter().map(ToString::to_string).collect(),
        ..SourceData::default()
    };
    for file in index.tests.values().flatten().map(|site| &site.file).chain(
        index
            .implementations
            .values()
            .flatten()
            .map(|site| &site.file),
    ) {
        if let Some(source) = index.files.get(file) {
            data.files.insert(file.clone(), source.clone());
        }
    }
    for (key, sites) in index.entries {
        let mut references = Vec::new();
        for site in sites {
            if let Some(source) = index.files.get(&site.file) {
                data.files.insert(site.file.clone(), source.clone());
            }
            references.push(SourceReference {
                path: site.rust_path.clone(),
                locations: vec![site],
            });
        }
        data.targets.insert(key, references);
    }
    Ok(data)
}
