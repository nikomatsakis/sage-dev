//! Structured diagnostics for sage.
//!
//! `Diagnostic<'db>` is the self-contained output type stored in `CheckedBody`.
//! It carries pre-rendered `String` messages and anchored `Span<'db>` locations.
//! Type display happens at `to_diagnostic()` time (while the stash is live).

// {anchor}`crates.sage_test_harness.compile_tests.diagnostic_locations`
// {anchor}`crates.sage_test_harness.compile_tests.rendering`
use crate::local_syms::LocalModItemSym;
use crate::span::{AbsoluteSpan, RelativeSpan};

// ---------------------------------------------------------------------------
// Core types
// ---------------------------------------------------------------------------

#[derive(Clone, Debug, PartialEq, Eq, Hash, sage_reflect::Reflect)]
pub enum Severity {
    Error,
    Warning,
}

/// A source location, resolvable to an absolute file position at rendering time.
#[derive(Clone, Debug, PartialEq, Eq, Hash, sage_reflect::Reflect)]
pub enum Span<'db> {
    /// A byte range within a symbol's body.
    Relative(LocalModItemSym<'db>, RelativeSpan),

    /// The entire span of a symbol.
    Symbol(LocalModItemSym<'db>),

    /// An already-absolute span. Use sparingly.
    Absolute(AbsoluteSpan<'db>),
}

impl<'db> Span<'db> {
    // {anchor}`crates.sage_ir.types.spans.relative_composition`
    pub fn resolve(&self, db: &'db dyn crate::Db) -> AbsoluteSpan<'db> {
        match self {
            Span::Relative(sym, rel) => sym.relative_span_base(db).resolve(*rel),
            Span::Symbol(sym) => sym.absolute_span(db),
            Span::Absolute(abs) => *abs,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash, sage_reflect::Reflect)]
pub enum LabelStyle {
    Primary,
    Secondary,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash, sage_reflect::Reflect)]
pub struct Label<'db> {
    pub span: Span<'db>,
    pub message: String,
    pub style: LabelStyle,
}

/// A single diagnostic emitted by sage.
#[derive(Clone, Debug, PartialEq, Eq, Hash, sage_reflect::Reflect)]
#[sage::item(crate::types::Diagnostic)]
pub struct Diagnostic<'db> {
    pub severity: Severity,
    pub span: Span<'db>,
    pub message: String,
    pub labels: Vec<Label<'db>>,
    pub notes: Vec<Diagnostic<'db>>,
}

impl<'db> Diagnostic<'db> {
    pub fn error(span: Span<'db>, message: impl Into<String>) -> Self {
        Diagnostic {
            severity: Severity::Error,
            span,
            message: message.into(),
            labels: Vec::new(),
            notes: Vec::new(),
        }
    }

    pub fn warning(span: Span<'db>, message: impl Into<String>) -> Self {
        Diagnostic {
            severity: Severity::Warning,
            span,
            message: message.into(),
            labels: Vec::new(),
            notes: Vec::new(),
        }
    }

    pub fn label(mut self, span: Span<'db>, message: impl Into<String>) -> Self {
        self.labels.push(Label {
            span,
            message: message.into(),
            style: LabelStyle::Primary,
        });
        self
    }

    pub fn secondary(mut self, span: Span<'db>, message: impl Into<String>) -> Self {
        self.labels.push(Label {
            span,
            message: message.into(),
            style: LabelStyle::Secondary,
        });
        self
    }

    pub fn note(mut self, sub: Diagnostic<'db>) -> Self {
        self.notes.push(sub);
        self
    }
}

// ---------------------------------------------------------------------------
// ErrorReported
// ---------------------------------------------------------------------------

/// Witness that at least one diagnostic has been emitted.
///
/// This type is intentionally impossible to construct outside this module.
/// The ONLY way to obtain one is by calling [`report`], which pushes a
/// diagnostic into a collection. This guarantees at the type level that
/// every `Ty::Error(e)` and `Res::Error(e)` was preceded by a user-visible
/// diagnostic message.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, sage_reflect::Reflect)]
#[sage::item(crate::types::ErrorReported)]
pub struct ErrorReported(());

impl sage_stash::StashDirect for ErrorReported {}

/// The sole entry point for minting `ErrorReported`.
///
/// Pushes `diag` into `diagnostics` and returns the witness token.
/// Every construction of `Ty::Error` or `Res::Error` must flow through here.
// {anchor}`crates.sage_ir.types.body.diagnosed_recovery`
pub fn report<'db>(diagnostics: &mut Vec<Diagnostic<'db>>, diag: Diagnostic<'db>) -> ErrorReported {
    diagnostics.push(diag);
    ErrorReported(())
}

// ---------------------------------------------------------------------------
// Rendering
// ---------------------------------------------------------------------------

use crate::span::ParseSource;

impl<'db> Diagnostic<'db> {
    /// Rich rendering using `annotate_snippets` — produces rustc-style output
    /// with source snippets, underlines, and labels.
    pub fn render(&self, db: &'db dyn crate::Db) -> String {
        use annotate_snippets::{AnnotationKind, Group, Level, Renderer, Snippet};

        let level = match self.severity {
            Severity::Error => Level::ERROR,
            Severity::Warning => Level::WARNING,
        };

        let abs = self.span.resolve(db);
        let source_file = match abs.source {
            ParseSource::SourceFile(sf) => sf,
            ParseSource::BangMacro(..) | ParseSource::Derive(..) => {
                return self.render_short(db);
            }
        };

        let source_text = source_file.text(db);
        let path = source_file.path(db);

        let mut snippet = Snippet::source(source_text).path(path.as_str()).fold(true);

        for label in &self.labels {
            let label_abs = label.span.resolve(db);
            if !same_file(label_abs.source, abs.source) {
                continue;
            }
            let range = label_abs.start as usize..label_abs.end as usize;
            let kind = match label.style {
                LabelStyle::Primary => AnnotationKind::Primary,
                LabelStyle::Secondary => AnnotationKind::Context,
            };
            snippet = snippet.annotation(kind.span(range).label(&label.message));
        }

        let report = &[Group::with_title(level.primary_title(&self.message)).element(snippet)];

        let renderer = Renderer::plain();
        let rendered = renderer.render(report).to_string();
        rendered
    }

    /// Simple multi-line rendering with labels (no source snippets):
    /// ```text
    /// error at {start}..{end}: {message}
    ///   at {start}..{end}: {label_message}
    /// ```
    pub fn render_short(&self, db: &'db dyn crate::Db) -> String {
        let abs = self.span.resolve(db);
        let prefix = match self.severity {
            Severity::Error => "error",
            Severity::Warning => "warning",
        };
        let mut out = format!("{prefix} at {}..{}: {}", abs.start, abs.end, self.message);
        for label in &self.labels {
            let label_abs = label.span.resolve(db);
            out.push_str(&format!(
                "\n  at {}..{}: {}",
                label_abs.start, label_abs.end, label.message
            ));
        }
        out
    }
}

fn same_file(a: ParseSource<'_>, b: ParseSource<'_>) -> bool {
    match (a, b) {
        (ParseSource::SourceFile(a), ParseSource::SourceFile(b)) => a == b,
        (ParseSource::Derive(a), ParseSource::Derive(b)) => a == b,
        (ParseSource::BangMacro(a_def, a_call), ParseSource::BangMacro(b_def, b_call)) => {
            a_def == b_def && a_call == b_call
        }
        (ParseSource::SourceFile(_), ParseSource::Derive(_))
        | (ParseSource::SourceFile(_), ParseSource::BangMacro(_, _))
        | (ParseSource::Derive(_), ParseSource::SourceFile(_))
        | (ParseSource::Derive(_), ParseSource::BangMacro(_, _))
        | (ParseSource::BangMacro(_, _), ParseSource::SourceFile(_))
        | (ParseSource::BangMacro(_, _), ParseSource::Derive(_)) => false,
    }
}

/// Owned diagnostic evidence emitted by a signature query. Salsa retains each
/// query's emissions once even when multiple clients demand the same signature.
#[salsa::accumulator]
// {anchor}`crates.sage_test_harness.compile_tests.diagnostic_counts`
#[derive(Clone, Debug)]
pub struct SignatureDiagnostic(pub DiagnosticSnapshot);

/// Resolved source data: rendering this value performs no semantic queries.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SourceLocation {
    pub path: String,
    pub source: String,
    pub start: usize,
    pub end: usize,
}
impl SourceLocation {
    pub fn lines(&self) -> std::ops::RangeInclusive<usize> {
        let first = self.source.as_bytes()[..self.start]
            .iter()
            .filter(|&&b| b == b'\n')
            .count()
            + 1;
        let last_offset = if self.end > self.start {
            self.end - 1
        } else {
            self.start
        };
        let last = self.source.as_bytes()[..last_offset]
            .iter()
            .filter(|&&b| b == b'\n')
            .count()
            + 1;
        first..=last
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DiagnosticSnapshot {
    pub severity: Severity,
    pub primary: Vec<SourceLocation>,
    pub message: String,
    pub labels: Vec<(Vec<SourceLocation>, String, LabelStyle)>,
    pub notes: Vec<DiagnosticSnapshot>,
}

impl DiagnosticSnapshot {
    /// Prefix logical source paths without touching diagnostic messages or text.
    pub fn prefix_paths(&mut self, prefix: &str) {
        for location in self.primary.iter_mut().chain(
            self.labels
                .iter_mut()
                .flat_map(|(locations, _, _)| locations.iter_mut()),
        ) {
            location.path = crate::source::normalize_path(&format!("{prefix}{}", location.path));
        }
        for note in &mut self.notes {
            note.prefix_paths(prefix);
        }
    }
    pub fn render(&self) -> String {
        use annotate_snippets::{AnnotationKind, Group, Level, Renderer, Snippet};
        let level = match self.severity {
            Severity::Error => Level::ERROR,
            Severity::Warning => Level::WARNING,
        };
        let mut group = Group::with_title(level.primary_title(&self.message));
        let mut sources: Vec<_> = self.primary.first().into_iter().collect();
        for (locations, _, _) in &self.labels {
            if let Some(location) = locations.first() {
                if !sources.iter().any(|source| source.path == location.path) {
                    sources.push(location);
                }
            }
        }
        for source in sources {
            let mut snippet = Snippet::source(&source.source)
                .path(&source.path)
                .fold(true);
            if let Some(primary) = self
                .primary
                .first()
                .filter(|primary| primary.path == source.path)
            {
                snippet =
                    snippet.annotation(AnnotationKind::Primary.span(primary.start..primary.end));
            }
            for (locations, message, style) in &self.labels {
                if let Some(location) = locations
                    .iter()
                    .find(|location| location.path == source.path)
                {
                    let kind = match style {
                        LabelStyle::Primary => AnnotationKind::Primary,
                        LabelStyle::Secondary => AnnotationKind::Context,
                    };
                    snippet =
                        snippet.annotation(kind.span(location.start..location.end).label(message));
                }
            }
            group = group.element(snippet);
        }
        let mut output = Renderer::plain().render(&[group]).to_string();
        for note in &self.notes {
            output.push('\n');
            output.push_str(&note.render());
        }
        output
    }
}

impl Diagnostic<'_> {
    pub fn snapshot(&self, db: &dyn crate::Db) -> DiagnosticSnapshot {
        fn locations<'db>(
            db: &'db dyn crate::Db,
            mut span: AbsoluteSpan<'db>,
        ) -> Vec<SourceLocation> {
            let mut locations = Vec::new();
            loop {
                span = match span.source {
                    ParseSource::SourceFile(file) => {
                        let source = file.text(db).clone();
                        assert!(
                            (span.start as usize) <= source.len()
                                && (span.end as usize) <= source.len(),
                            "diagnostic span outside source"
                        );
                        locations.push(SourceLocation {
                            path: file.path(db).clone(),
                            source,
                            start: span.start as usize,
                            end: span.end as usize,
                        });
                        break;
                    }
                    ParseSource::BangMacro(_, invocation) => invocation.span(db),
                    ParseSource::Derive(expansion) => expansion.source_item(db).absolute_span(db),
                };
            }
            locations
        }
        DiagnosticSnapshot {
            severity: self.severity.clone(),
            primary: locations(db, self.span.resolve(db)),
            message: self.message.clone(),
            labels: self
                .labels
                .iter()
                .map(|label| {
                    (
                        locations(db, label.span.resolve(db)),
                        label.message.clone(),
                        label.style.clone(),
                    )
                })
                .collect(),
            notes: self.notes.iter().map(|note| note.snapshot(db)).collect(),
        }
    }
}
