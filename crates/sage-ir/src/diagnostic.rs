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

impl ErrorReported {
    pub(crate) fn new() -> Self {
        ErrorReported(())
    }
}

// ---------------------------------------------------------------------------
// Rendering (simple format for tests)
// ---------------------------------------------------------------------------

impl<'db> Diagnostic<'db> {
    /// Simple one-line rendering: `"error at {start}..{end}: {message}"`
    pub fn render_short(&self, db: &'db dyn crate::Db) -> String {
        let abs = self.span.resolve(db);
        format!("error at {}..{}: {}", abs.start, abs.end, self.message)
    }
}
