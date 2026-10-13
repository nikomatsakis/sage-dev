//! Repository checks shared by the command line, CI, and workspace tests.
//!
//! These checks validate model consistency and source correspondence. Runtime
//! call checking lives in [`crate::contracts`] and requires observed events.
use std::fmt;
use std::path::Path;

use crate::correspondence::{CorrespondenceIndex, RustSite};
use crate::review::{self, Decision};
use crate::spec::SpecIndex;
use crate::{Diagnostic, Span};

#[derive(Debug, serde::Serialize)]
pub struct ModelCheck {
    /// Whether all model documents loaded, parsed, and passed static validation.
    pub valid: bool,
    /// Number of parsed `.sage` documents; zero if loading did not complete.
    pub documents: usize,
    /// Static model errors, including invalid declarations and dependency envelopes.
    /// Failures to locate, read, or parse the model are reported in `load_error`.
    pub diagnostics: Vec<Diagnostic>,
    /// Failure to locate, read, or parse the model, preventing static validation.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub load_error: Option<String>,
}

#[derive(Debug, serde::Serialize)]
pub struct ItemCoverage {
    pub sage_item: String,
    pub source_sites: usize,
    pub executable_declarations: usize,
}

#[derive(Debug, serde::Serialize)]
pub struct CorrespondenceCheck {
    pub valid: bool,
    pub mapped_items: usize,
    pub source_sites: usize,
    pub executable_declarations: usize,
    pub source_only_sites: usize,
    pub diagnostics: Vec<Diagnostic>,
    pub coverage: Vec<ItemCoverage>,
    pub scope: &'static str,
    pub runtime_conformance_checked: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub load_error: Option<String>,
}

#[derive(Debug, serde::Serialize)]
pub struct AnchorCoverage {
    pub anchor: String,
    pub file: String,
    pub span: Span,
    pub tests: Vec<RustSite>,
    pub implementations: Vec<crate::evidence::ImplementationSite>,
}

#[derive(Debug, serde::Serialize)]
pub struct SpecificationCheck {
    pub valid: bool,
    pub documents: usize,
    pub anchors: usize,
    pub test_references: usize,
    pub diagnostics: Vec<Diagnostic>,
    pub coverage: Vec<AnchorCoverage>,
    /// Source annotations establish traceability; the test runner supplies results.
    pub tests_executed: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub load_error: Option<String>,
}

#[derive(Debug, serde::Serialize)]
pub struct ReviewCheck {
    pub valid: bool,
    pub approved: usize,
    pub todo: usize,
    pub not_applicable: usize,
    pub diagnostics: Vec<Diagnostic>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub load_error: Option<String>,
}

/// A report is successful only when every build-time validation stage ran and passed.
#[derive(Debug, serde::Serialize)]
pub struct CheckReport {
    /// Model loading and static consistency results, always present even on load failure.
    pub model: ModelCheck,
    /// Bidirectional, crate-correct mappings between Sage items and Rust declarations.
    /// Absent when model loading or validation prevented this stage from starting.
    pub correspondence: Option<CorrespondenceCheck>,
    /// Anchor definitions and their Rust implementation and test associations.
    /// Absent if static model validation failed or the Rust source index could not load.
    pub specification: Option<SpecificationCheck>,
    /// Freshness and decisions of saved claim-and-test reviews, without executing tests.
    /// Absent when specification validation or an earlier prerequisite failed.
    pub review: Option<ReviewCheck>,
    /// Formatted diagnostics and load failures, including source excerpts when available.
    /// Prepared while validating, so formatting does not depend on later source edits.
    #[serde(skip)]
    rendered_errors: Vec<String>,
}

impl CheckReport {
    pub fn is_valid(&self) -> bool {
        self.model.valid
            && self.correspondence.as_ref().is_some_and(|r| r.valid)
            && self.specification.as_ref().is_some_and(|r| r.valid)
            && self.review.as_ref().is_some_and(|r| r.valid)
    }

    /// Turn diagnostics and load failures into a failing command or test result.
    pub fn ensure_valid(&self) -> Result<(), CheckError> {
        if self.is_valid() {
            Ok(())
        } else {
            Err(CheckError(self.to_string()))
        }
    }
}

pub struct CheckError(String);

impl fmt::Display for CheckError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}
impl std::error::Error for CheckError {}

// Rust's test and main Result handlers use Debug. Keep their error reports as
// readable as Display, rather than quoting a string with escaped newlines.
impl fmt::Debug for CheckError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(self, f)
    }
}

impl fmt::Display for CheckReport {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if !self.is_valid() {
            let count = self.rendered_errors.len();
            writeln!(
                f,
                "sage-meta check failed ({count} {})",
                if count == 1 { "error" } else { "errors" }
            )?;
            for error in &self.rendered_errors {
                write!(f, "\n{error}\n")?;
            }
            return Ok(());
        }
        fn diagnostics(
            f: &mut fmt::Formatter<'_>,
            stage: &str,
            errors: &[Diagnostic],
            load_error: &Option<String>,
        ) -> fmt::Result {
            if let Some(error) = load_error {
                writeln!(f, "{stage}: {error}")?;
            }
            for error in errors {
                writeln!(f, "{error}")?;
            }
            Ok(())
        }
        if self.model.valid {
            writeln!(f, "Validated {} .sage documents.", self.model.documents)?;
        }
        diagnostics(f, "model", &self.model.diagnostics, &self.model.load_error)?;
        if let Some(report) = &self.correspondence {
            if report.valid {
                writeln!(
                    f,
                    "Mapped {} Sage items at {} Rust sites ({} executable declarations; {} source-only).",
                    report.mapped_items,
                    report.source_sites,
                    report.executable_declarations,
                    report.source_only_sites
                )?;
            }
            diagnostics(f, "correspondence", &report.diagnostics, &report.load_error)?;
        }
        if let Some(report) = &self.specification {
            if report.valid {
                writeln!(
                    f,
                    "Checked {} specification anchors and {} test references in {} Markdown documents.",
                    report.anchors, report.test_references, report.documents
                )?;
            }
            diagnostics(f, "specification", &report.diagnostics, &report.load_error)?;
        }
        if let Some(report) = &self.review {
            if report.valid {
                writeln!(
                    f,
                    "Checked review snapshots: {} approved, {} TODO, {} N/A.",
                    report.approved, report.todo, report.not_applicable
                )?;
            }
            diagnostics(f, "review", &report.diagnostics, &report.load_error)?;
        }
        writeln!(
            f,
            "Build-time checks do not execute tests or observe runtime calls."
        )
    }
}

/// Validate the default model, Cargo-owned Rust sources/tests, and `md/` in a repository.
///
/// The explicit root makes this independent of the process's working directory.
/// The function invokes Cargo metadata, but never builds or runs tests recursively.
pub fn check(workspace_root: &Path) -> CheckReport {
    match crate::workspace::default_model_root(workspace_root) {
        Ok(model_root) => check_roots(&model_root, Some(workspace_root)),
        Err(error) => CheckReport {
            model: ModelCheck {
                valid: false,
                documents: 0,
                diagnostics: Vec::new(),
                load_error: Some(error.to_string()),
            },
            correspondence: None,
            specification: None,
            review: None,
            rendered_errors: vec![format!(
                "error: could not locate the .sage model\n  = {error}"
            )],
        },
    }
}

/// Validate a selected model directory using its containing Cargo workspace and `md/`.
/// This is the CLI's explicit-path entry point for `check path/to/model`.
pub fn check_model(model_root: &Path) -> CheckReport {
    let absolute_root = model_root.canonicalize().ok();
    let rust_root = absolute_root.as_deref().and_then(|root| {
        root.ancestors()
            .find(|path| path.join("Cargo.toml").is_file())
    });
    check_roots(model_root, rust_root)
}

fn check_roots(model_root: &Path, rust_root: Option<&Path>) -> CheckReport {
    let mut report = CheckReport {
        model: ModelCheck {
            valid: false,
            documents: 0,
            diagnostics: Vec::new(),
            load_error: None,
        },
        correspondence: None,
        specification: None,
        review: None,
        rendered_errors: Vec::new(),
    };
    let documents = match crate::workspace::load_documents(model_root) {
        Ok(documents) => documents,
        Err(error) => {
            report
                .rendered_errors
                .push(match error.downcast_ref::<Diagnostic>() {
                    Some(diagnostic) => render_file_diagnostic(diagnostic, rust_root),
                    None => format!("error: could not load .sage documents\n  = {error}"),
                });
            report.model.load_error = Some(error.to_string());
            return report;
        }
    };
    report.model.documents = documents.len();
    report.model.diagnostics = crate::validate(&documents);
    report.rendered_errors.extend(
        report
            .model
            .diagnostics
            .iter()
            .map(|diagnostic| render_file_diagnostic(diagnostic, rust_root)),
    );
    report.model.valid = report.model.diagnostics.is_empty();
    if !report.model.valid {
        return report;
    }

    let mut correspondence = CorrespondenceCheck {
        valid: false,
        mapped_items: 0,
        source_sites: 0,
        executable_declarations: 0,
        source_only_sites: 0,
        diagnostics: Vec::new(),
        coverage: Vec::new(),
        scope: "crate/module headers, named declarations, and explicitly declared methods; all source cfg alternatives",
        runtime_conformance_checked: false,
        load_error: None,
    };
    let Some(rust_root) = rust_root else {
        report
            .rendered_errors
            .push("error: checking requires a Cargo workspace containing the model".into());
        correspondence.load_error =
            Some("checking requires a Cargo workspace containing the model".into());
        report.correspondence = Some(correspondence);
        return report;
    };
    let index = match CorrespondenceIndex::load(rust_root) {
        Ok(index) => index,
        Err(error) => {
            report.rendered_errors.push(format!(
                "error: could not load Rust correspondences\n  = {error}"
            ));
            correspondence.load_error = Some(error.to_string());
            report.correspondence = Some(correspondence);
            return report;
        }
    };
    correspondence.diagnostics = index.validate(&documents);
    report
        .rendered_errors
        .extend(correspondence.diagnostics.iter().map(|diagnostic| {
            if let Some(source) = index.files.get(&diagnostic.file) {
                render_diagnostic(diagnostic, &diagnostic.file, Some(source))
            } else {
                // Missing Rust mappings point back into the .sage declarations.
                render_file_diagnostic(diagnostic, index.workspace_root())
            }
        }));
    correspondence.valid = correspondence.diagnostics.is_empty();
    correspondence.mapped_items = index.entries.len();
    correspondence.source_sites = index.entries.values().flatten().count();
    correspondence.executable_declarations = index
        .entries
        .values()
        .flatten()
        .filter(|site| site.executable)
        .count();
    correspondence.source_only_sites =
        correspondence.source_sites - correspondence.executable_declarations;
    correspondence.coverage = index
        .entries
        .iter()
        .map(|(name, sites)| ItemCoverage {
            sage_item: name.clone(),
            source_sites: sites.len(),
            executable_declarations: sites.iter().filter(|site| site.executable).count(),
        })
        .collect();
    report.correspondence = Some(correspondence);

    let mut specification = SpecificationCheck {
        valid: false,
        documents: 0,
        anchors: 0,
        test_references: index.tests.values().map(Vec::len).sum(),
        diagnostics: Vec::new(),
        coverage: Vec::new(),
        tests_executed: false,
        load_error: None,
    };
    // A selected model may be inside a member crate. Cargo, rather than the
    // nearest manifest's directory, determines the documentation workspace.
    let workspace = index.workspace_root().unwrap_or(rust_root);
    let docs_root = if workspace.join("book.toml").is_file() {
        crate::workspace::book_source_root(workspace).unwrap_or_else(|_| workspace.join("md"))
    } else {
        workspace.join("md")
    };
    match SpecIndex::load_workspace(workspace) {
        Ok(spec) => {
            specification.documents = spec.documents;
            specification.anchors = spec.anchors.len();
            let mut anchor_errors = spec.validate_anchors(&index.tests);
            anchor_errors.extend(spec.validate_implementations(&index));
            report
                .rendered_errors
                .extend(anchor_errors.iter().map(|diagnostic| {
                    let path = docs_root
                        .join(&diagnostic.file)
                        .to_string_lossy()
                        .into_owned();
                    render_diagnostic(
                        diagnostic,
                        &path,
                        spec.sources.get(&diagnostic.file).map(String::as_str),
                    )
                }));
            let mut test_errors = spec.validate_test_references(&index.tests);
            test_errors.extend(spec.validate_implementation_references(&index));
            report
                .rendered_errors
                .extend(test_errors.iter().map(|diagnostic| {
                    render_diagnostic(
                        diagnostic,
                        &diagnostic.file,
                        index.files.get(&diagnostic.file).map(String::as_str),
                    )
                }));
            specification.diagnostics = anchor_errors;
            specification.diagnostics.extend(test_errors);
            specification.valid = specification.diagnostics.is_empty();
            specification.coverage = spec
                .anchors
                .iter()
                .map(|(name, anchor)| AnchorCoverage {
                    anchor: name.clone(),
                    file: anchor.file.clone(),
                    span: anchor.span.clone(),
                    tests: index.tests.get(name).cloned().unwrap_or_default(),
                    implementations: index.implementations.get(name).cloned().unwrap_or_default(),
                })
                .collect();
            if specification.valid {
                check_reviews(
                    &mut report,
                    &spec,
                    &index,
                    index.workspace_root().unwrap_or(rust_root),
                );
            }
        }
        Err(error) => {
            report.rendered_errors.push(format!(
                "error: could not load Markdown specifications from {}\n  = {error}",
                docs_root.display()
            ));
            specification.load_error = Some(error.to_string());
        }
    }
    report.specification = Some(specification);
    report
}

// {anchor}`meta.reviews.gate`
fn check_reviews(
    report: &mut CheckReport,
    spec: &SpecIndex,
    rust: &CorrespondenceIndex,
    workspace: &Path,
) {
    let mut stage = ReviewCheck {
        valid: false,
        approved: 0,
        todo: 0,
        not_applicable: 0,
        diagnostics: Vec::new(),
        load_error: None,
    };
    let docs_root = if workspace.join("book.toml").is_file() {
        crate::workspace::book_source_root(workspace).unwrap_or_else(|_| workspace.join("md"))
    } else {
        workspace.join("md")
    };
    let inputs = review::inventory(spec, rust);
    let checklist = review::load(&workspace.join(review::CHECKLIST_FILE));
    match inputs.and_then(|inputs| checklist.map(|checklist| (inputs, checklist))) {
        Ok((inputs, checklist)) => {
            let issues = review::validate(&inputs, &checklist);
            for record in checklist.reviews.iter().filter(|record| {
                inputs.get(&record.anchor) == Some(&record.input)
                    && !issues.iter().any(|issue| {
                        issue
                            .anchor
                            .as_ref()
                            .is_none_or(|anchor| anchor == &record.anchor)
                    })
            }) {
                match record.decision {
                    Decision::Approved => stage.approved += 1,
                    Decision::Todo => stage.todo += 1,
                    Decision::NotApplicable => stage.not_applicable += 1,
                    Decision::Pending | Decision::NeedsWork => {}
                }
            }
            for issue in issues {
                let anchor = issue
                    .anchor
                    .as_ref()
                    .and_then(|name| spec.anchors.get(name));
                let diagnostic = Diagnostic {
                    file: anchor.map_or_else(
                        || review::CHECKLIST_FILE.into(),
                        |anchor| docs_root.join(&anchor.file).to_string_lossy().into_owned(),
                    ),
                    span: anchor.map_or(Span { start: 0, end: 0 }, |anchor| anchor.span.clone()),
                    message: issue.message,
                };
                let source = anchor.and_then(|anchor| spec.sources.get(&anchor.file));
                report.rendered_errors.push(render_diagnostic(
                    &diagnostic,
                    &diagnostic.file,
                    source.map(String::as_str),
                ));
                stage.diagnostics.push(diagnostic);
            }
            stage.valid = stage.diagnostics.is_empty();
        }
        Err(error) => {
            stage.load_error = Some(error.to_string());
            report.rendered_errors.push(format!(
                "error: could not load specification review snapshots from {}\n  = {error}",
                workspace.join(review::CHECKLIST_FILE).display()
            ));
        }
    }
    if !stage.valid {
        if let Some(error) = report.rendered_errors.last_mut() {
            error.push_str(&format!("\n  = help: run the {} skill to review changed claims and tests; start with `cargo run -p sage-meta -- review prepare`, then record the completed review with `cargo run -p sage-meta -- review record <file>`", review::REVIEW_SKILL));
        }
    }
    report.review = Some(stage);
}

fn render_file_diagnostic(diagnostic: &Diagnostic, workspace: Option<&Path>) -> String {
    let file = Path::new(&diagnostic.file);
    let absolute = file.canonicalize().ok();
    let root = workspace.and_then(|root| root.canonicalize().ok());
    let path = absolute
        .as_deref()
        .zip(root.as_deref())
        .and_then(|(file, root)| file.strip_prefix(root).ok())
        .unwrap_or(file)
        .to_string_lossy();
    let source = std::fs::read_to_string(file).ok();
    render_diagnostic(diagnostic, &path, source.as_deref())
}

fn render_diagnostic(diagnostic: &Diagnostic, path: &str, source: Option<&str>) -> String {
    use annotate_snippets::{AnnotationKind, Group, Level, Renderer, Snippet};
    let span = &diagnostic.span;
    if let Some(source) =
        source.filter(|source| span.start <= span.end && source.get(span.start..span.end).is_some())
    {
        let snippet = Snippet::source(source)
            .path(path)
            .fold(true)
            .annotation(AnnotationKind::Primary.span(span.start..span.end));
        return Renderer::plain()
            .render(&[
                Group::with_title(Level::ERROR.primary_title(&diagnostic.message)).element(snippet),
            ])
            .to_string();
    }
    // Reporting must still succeed if a file cannot be read or its span no
    // longer fits; retain the original location rather than inventing a line.
    format!(
        "error: {}\n  --> {path} (bytes {}..{})",
        diagnostic.message, span.start, span.end
    )
}
