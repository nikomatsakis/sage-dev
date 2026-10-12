use crate::{Declaration, Document, Header, RustTarget, TypeExpr, TypeExprKind};
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
    render(documents, None)
}

/// Include source snapshots from the supplied Cargo workspace.
pub fn render_browser_with_workspace(
    documents: &[Document],
    workspace: &Path,
) -> Result<String, Box<dyn std::error::Error>> {
    render(documents, Some(workspace))
}

fn render(
    documents: &[Document],
    workspace: Option<&Path>,
) -> Result<String, Box<dyn std::error::Error>> {
    let errors = crate::validate(documents);
    if !errors.is_empty() {
        return Err(errors[0].clone().into());
    }
    if documents.is_empty() {
        return Err("cannot browse an empty model".into());
    }
    let roots: Vec<_> = documents
        .iter()
        .filter_map(|doc| match &doc.header {
            Header::Crate { name, .. } => Some((
                name,
                std::path::Path::new(&doc.file)
                    .parent()
                    .unwrap_or(std::path::Path::new("")),
            )),
            Header::Module(_) => None,
        })
        .collect();
    let data = documents
        .iter()
        .map(|document| {
            let namespace = match &document.header {
                Header::Crate { name, .. } => name.clone(),
                Header::Module(path) => {
                    let (name, _) = roots
                        .iter()
                        .filter(|(_, root)| std::path::Path::new(&document.file).starts_with(root))
                        .max_by_key(|(_, root)| root.components().count())
                        .expect("validated module has a crate");
                    format!("{name}::{}", path.segments.join("::"))
                }
            };
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
    Ok(format!("{prefix}{json}{middle}{source_json}{suffix}"))
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
    locations: Vec<crate::rust_source::RustLocation>,
}

fn named_references(ty: &TypeExpr, paths: &mut Vec<String>) {
    match &ty.kind {
        TypeExprKind::Named { path, arguments } => {
            let name = std::iter::once(path.root.as_str())
                .chain(path.segments.iter().map(String::as_str))
                .collect::<Vec<_>>()
                .join("::");
            if !paths.contains(&name) {
                paths.push(name);
            }
            for argument in arguments {
                named_references(argument, paths);
            }
        }
        TypeExprKind::Tuple(items) => {
            for item in items {
                named_references(item, paths);
            }
        }
    }
}

fn source_data(
    documents: &[BrowserDocument<'_>],
    workspace: Option<&Path>,
) -> Result<SourceData, Box<dyn std::error::Error>> {
    let Some(workspace) = workspace else {
        return Ok(SourceData::default());
    };
    let index = crate::rust_source::SourceIndex::load(workspace)?;
    let mut data = SourceData {
        warnings: index.warnings.clone(),
        ..SourceData::default()
    };
    for entry in documents {
        for declaration in &entry.document.declarations {
            let (name, rust) = match declaration {
                Declaration::Type(decl) => (&decl.name, &decl.rust),
                Declaration::Class(class) => (&class.name, &class.rust),
            };
            let Some(annotation) = rust else {
                continue;
            };
            let ty = match &annotation.target {
                RustTarget::Type(ty) => ty,
                RustTarget::None { .. } => continue,
            };
            let mut paths = Vec::new();
            named_references(ty, &mut paths);
            let mut references = Vec::new();
            for path in paths {
                let key = path.strip_prefix("crate::").map_or_else(
                    || path.clone(),
                    |rest| format!("{}::{rest}", entry.namespace.split("::").next().unwrap()),
                );
                let locations = index.resolve(&key);
                for location in &locations {
                    data.files
                        .insert(location.file.clone(), index.files[&location.file].clone());
                }
                references.push(SourceReference { path, locations });
            }
            data.targets
                .insert(format!("{}::{name}", entry.namespace), references);
        }
    }
    Ok(data)
}
