//! Shared loading for command-line, browser, and runtime contract consumers.
use std::path::Path;

use crate::{Document, correspondence::CorrespondenceIndex};

/// Stable namespace shared by source correspondence and browser navigation.
/// The model validator separately diagnoses invalid headers and file placement.
pub(crate) fn document_namespace(documents: &[Document], document: &Document) -> Option<String> {
    match &document.header {
        crate::Header::Crate { name, .. } => Some(name.clone()),
        crate::Header::Module(path) => {
            let (name, _) = documents
                .iter()
                .filter_map(|document| match &document.header {
                    crate::Header::Crate { name, .. } => Some((
                        name,
                        Path::new(&document.file).parent().unwrap_or(Path::new("")),
                    )),
                    crate::Header::Module(_) => None,
                })
                .filter(|(_, root)| Path::new(&document.file).starts_with(root))
                .max_by_key(|(_, root)| root.components().count())?;
            Some(
                std::iter::once(name.as_str())
                    .chain(path.segments.iter().map(String::as_str))
                    .collect::<Vec<_>>()
                    .join("::"),
            )
        }
    }
}

/// Read the model once. A caller owns this snapshot for the duration of a session.
pub fn load_documents(root: &Path) -> Result<Vec<Document>, Box<dyn std::error::Error>> {
    fn visit(path: &Path, documents: &mut Vec<Document>) -> Result<(), Box<dyn std::error::Error>> {
        let mut entries = std::fs::read_dir(path)?
            .map(|entry| entry.map(|e| e.path()))
            .collect::<Result<Vec<_>, _>>()?;
        entries.sort();
        for path in entries {
            if path.is_symlink() {
                continue;
            }
            if path.is_dir() {
                visit(&path, documents)?;
            } else if path.extension().is_some_and(|ext| ext == "sage") {
                documents.push(crate::parse(
                    &path.to_string_lossy(),
                    &std::fs::read_to_string(&path)?,
                )?);
            }
        }
        Ok(())
    }
    let mut documents = Vec::new();
    visit(root, &mut documents)?;
    if documents.is_empty() {
        return Err("no .sage documents found".into());
    }
    Ok(documents)
}

/// Immutable source/model snapshot. Compilation provenance is verified separately
/// through `CorrespondenceIndex::verify_source` when observing compiled queries.
pub struct Workspace {
    pub documents: Vec<Document>,
    pub correspondences: CorrespondenceIndex,
}

impl Workspace {
    pub fn load(model_root: &Path, rust_root: &Path) -> Result<Self, Box<dyn std::error::Error>> {
        let documents = load_documents(model_root)?;
        let errors = crate::validate(&documents);
        if let Some(error) = errors.first() {
            return Err(error.clone().into());
        }
        let correspondences = CorrespondenceIndex::load(rust_root)?;
        Ok(Self {
            documents,
            correspondences,
        })
    }
}
