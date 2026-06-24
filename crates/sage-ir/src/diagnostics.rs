/// Diagnostic types for sage.
///
/// These are used by the oracle test harness to verify that sage produces
/// the expected errors and warnings.

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum Severity {
    Error,
    Warning,
}

/// A diagnostic emitted by sage, with a byte offset into the source file.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct SageDiagnostic {
    pub severity: Severity,
    pub message: String,
    /// Absolute byte offset into the source file.
    pub offset: u32,
}
