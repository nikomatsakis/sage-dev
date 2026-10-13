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
}
#[derive(serde::Serialize)]
struct SourceReference {
    path: String,
    locations: Vec<crate::correspondence::RustSite>,
}

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
