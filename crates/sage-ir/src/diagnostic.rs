//! Structured diagnostics for sage.
//!
//! `Diagnostic<'db>` is the self-contained output type stored in `CheckedBody`.
//! It carries pre-rendered `String` messages and anchored `Span<'db>` locations.
//! Type display happens at `to_diagnostic()` time (while the stash is live).

use crate::local_syms::LocalModItemSym;
use crate::span::{AbsoluteSpan, RelativeSpan};

// ---------------------------------------------------------------------------
// Core types
// ---------------------------------------------------------------------------

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum Severity {
    Error,
    Warning,
}

/// A source location, resolvable to an absolute file position at rendering time.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum Span<'db> {
    /// A byte range within a symbol's body.
    Relative(LocalModItemSym<'db>, RelativeSpan),

    /// The entire span of a symbol.
    Symbol(LocalModItemSym<'db>),

    /// An already-absolute span. Use sparingly.
    Absolute(AbsoluteSpan<'db>),
}

impl<'db> Span<'db> {
    pub fn resolve(&self, db: &'db dyn crate::Db) -> AbsoluteSpan<'db> {
        match self {
            Span::Relative(sym, rel) => sym.absolute_span(db).resolve(*rel),
            Span::Symbol(sym) => sym.absolute_span(db),
            Span::Absolute(abs) => *abs,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum LabelStyle {
    Primary,
    Secondary,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Label<'db> {
    pub span: Span<'db>,
    pub message: String,
    pub style: LabelStyle,
}

/// A single diagnostic emitted by sage.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
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
/// Only constructible by the diagnostic-reporting machinery.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub struct ErrorReported(());

impl sage_stash::StashDirect for ErrorReported {}

impl ErrorReported {
    /// Mint a new witness. Only call this from `BodyCheck::report()`.
    pub(crate) fn mint() -> Self {
        ErrorReported(())
    }
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
            ParseSource::BangMacro(..) => return self.render_short(db),
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
        _ => false,
    }
}
