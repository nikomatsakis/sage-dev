//! Shared loading for command-line, browser, and runtime contract consumers.
use std::path::{Path, PathBuf};

use crate::{Document, correspondence::CorrespondenceIndex};

/// Locate the repository model in the configured mdBook source directory.
/// Explicit model paths should be passed directly to [`load_documents`].
pub fn default_model_root(workspace: &Path) -> Result<PathBuf, Box<dyn std::error::Error>> {
    Ok(book_source_root(workspace)?.join("design/crates"))
}

/// Configured mdBook source root shared by specification and model loading.
pub fn book_source_root(workspace: &Path) -> Result<PathBuf, Box<dyn std::error::Error>> {
    #[derive(Default, serde::Deserialize)]
    struct BookConfig {
        #[serde(default)]
        book: Book,
    }
    #[derive(Default, serde::Deserialize)]
    struct Book {
        src: Option<PathBuf>,
    }
    let config: BookConfig =
        toml::from_str(&std::fs::read_to_string(workspace.join("book.toml"))?)?;
    Ok(workspace.join(config.book.src.unwrap_or_else(|| PathBuf::from("src"))))
}

/// Find the nearest enclosing modeled crate for a source file. The source and
/// document paths must use the same root, as they do in the shared book loader.
pub fn crate_source_owner<'a>(documents: &'a [Document], source: &Path) -> Option<&'a str> {
    documents
        .iter()
        .filter_map(|document| match &document.header {
            crate::Header::Crate { name, .. } => Some((
                name.as_str(),
                Path::new(&document.file).parent().unwrap_or(Path::new("")),
            )),
            crate::Header::Module(_) => None,
        })
        .filter(|(_, root)| source.starts_with(root))
        .max_by_key(|(_, root)| root.components().count())
        .map(|(name, _)| name)
}

/// Stable namespace shared by source correspondence and browser navigation.
/// The model validator separately diagnoses invalid headers and file placement.
pub fn document_namespace(documents: &[Document], document: &Document) -> Option<String> {
    match &document.header {
        crate::Header::Crate { name, .. } => Some(name.clone()),
        crate::Header::Module(path) => {
            let name = crate_source_owner(documents, Path::new(&document.file))?;
            Some(
                std::iter::once(name)
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
    use super::{default_model_root, load_documents};

    #[test]
    fn default_model_requires_the_configured_book_source() {
        let workspace = std::env::temp_dir().join(format!(
            "sage-model-roots-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        // An old-layout model must not substitute for missing book configuration
        // or for an absent configured model.
        std::fs::create_dir_all(workspace.join("sage-src")).unwrap();
        std::fs::write(
            workspace.join("sage-src/crate.sage"),
            "crate old { \"\"\"Old layout.\"\"\" }",
        )
        .unwrap();
        assert!(default_model_root(&workspace).is_err());
        assert!(crate::check(&workspace).model.load_error.is_some());

        std::fs::write(workspace.join("book.toml"), "[book]\nsrc = 'manual'\n").unwrap();
        let book_model = workspace.join("manual/design/crates");
        assert_eq!(default_model_root(&workspace).unwrap(), book_model);
        assert!(load_documents(&book_model).is_err());
        assert!(crate::check(&workspace).model.load_error.is_some());
        std::fs::create_dir_all(&book_model).unwrap();
        assert_eq!(
            load_documents(&book_model).unwrap_err().to_string(),
            "no .sage documents found"
        );
        std::fs::write(
            book_model.join("crate.sage"),
            "crate configured { \"\"\"Configured book model.\"\"\" }",
        )
        .unwrap();
        assert_eq!(load_documents(&book_model).unwrap().len(), 1);

        std::fs::write(workspace.join("book.toml"), "[book]\n").unwrap();
        let default_book_model = workspace.join("src/design/crates");
        assert_eq!(default_model_root(&workspace).unwrap(), default_book_model);
        assert!(load_documents(&default_book_model).is_err());
        assert!(crate::check(&workspace).model.load_error.is_some());
        std::fs::create_dir_all(&default_book_model).unwrap();
        std::fs::write(
            default_book_model.join("crate.sage"),
            "crate default { \"\"\"Default book model.\"\"\" }",
        )
        .unwrap();
        assert_eq!(load_documents(&default_book_model).unwrap().len(), 1);

        // Broken configuration must not silently select a different model.
        std::fs::write(workspace.join("book.toml"), "[book\n").unwrap();
        assert!(default_model_root(&workspace).is_err());
        assert!(crate::check(&workspace).model.load_error.is_some());
        std::fs::remove_dir_all(workspace).unwrap();
    }
}
