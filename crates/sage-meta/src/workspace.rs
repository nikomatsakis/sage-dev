//! Shared loading for command-line, browser, and runtime contract consumers.
use std::path::{Path, PathBuf};

use crate::{Document, correspondence::CorrespondenceIndex};

/// Locate the repository model in the configured mdBook source directory.
/// Standalone fixture workspaces can retain the legacy `sage-src` layout.
/// Explicit model paths should be passed directly to [`load_documents`].
pub fn default_model_root(workspace: &Path) -> Result<PathBuf, Box<dyn std::error::Error>> {
    #[derive(Default, serde::Deserialize)]
    struct BookConfig {
        #[serde(default)]
        book: Book,
    }
    #[derive(Default, serde::Deserialize)]
    struct Book {
        src: Option<PathBuf>,
    }

    let config_file = workspace.join("book.toml");
    if config_file.exists() {
        let config: BookConfig = toml::from_str(&std::fs::read_to_string(config_file)?)?;
        let model = workspace
            .join(config.book.src.unwrap_or_else(|| PathBuf::from("src")))
            .join("design/crates");
        if model.is_dir() {
            return Ok(model);
        }
    }
    Ok(workspace.join("sage-src"))
}

/// Stable namespace shared by source correspondence and browser navigation.
/// The model validator separately diagnoses invalid headers and file placement.
pub fn document_namespace(documents: &[Document], document: &Document) -> Option<String> {
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

#[cfg(test)]
mod tests {
    use super::default_model_root;

    #[test]
    fn configured_book_model_takes_precedence_over_legacy_fixture_layout() {
        let workspace = std::env::temp_dir().join(format!(
            "sage-model-roots-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(workspace.join("sage-src")).unwrap();
        assert_eq!(
            default_model_root(&workspace).unwrap(),
            workspace.join("sage-src")
        );

        std::fs::write(workspace.join("book.toml"), "[book]\nsrc = 'manual'\n").unwrap();
        let book_model = workspace.join("manual/design/crates");
        std::fs::create_dir_all(&book_model).unwrap();
        assert_eq!(default_model_root(&workspace).unwrap(), book_model);

        std::fs::write(workspace.join("book.toml"), "[book]\n").unwrap();
        let default_book_model = workspace.join("src/design/crates");
        std::fs::create_dir_all(&default_book_model).unwrap();
        assert_eq!(default_model_root(&workspace).unwrap(), default_book_model);

        // Broken configuration must not silently select the legacy model.
        std::fs::write(workspace.join("book.toml"), "[book\n").unwrap();
        assert!(default_model_root(&workspace).is_err());
        assert!(crate::check(&workspace).model.load_error.is_some());
        std::fs::remove_dir_all(workspace).unwrap();
    }
}
