use regex::Regex;
use std::sync::LazyLock;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ExpectedSeverity {
    Error,
    Warn,
}

#[derive(Clone, Debug)]
pub struct Annotation {
    pub line: usize,
    pub severity: ExpectedSeverity,
    pub pattern: String,
}

impl Annotation {
    pub fn matches_message(&self, message: &str) -> bool {
        if self.pattern.contains('\u{2026}') {
            let parts: Vec<&str> = self.pattern.split('\u{2026}').collect();
            let mut remaining = message;
            for (i, part) in parts.iter().enumerate() {
                if part.is_empty() {
                    continue;
                }
                match remaining.find(part) {
                    Some(pos) => {
                        if i == 0 && pos != 0 {
                            return false;
                        }
                        remaining = &remaining[pos + part.len()..];
                    }
                    None => return false,
                }
            }
            true
        } else {
            message.contains(&self.pattern)
        }
    }
}

/// Directives that override default oracle checking behavior.
#[derive(Clone, Debug, Default)]
pub struct OracleDirectives {
    /// If true, rustc is expected to succeed even though sage errors.
    pub rustc_ok: bool,
    /// If true, rustc is expected to error even though sage succeeds.
    pub rustc_error: bool,
}

pub struct ParsedAnnotations {
    pub annotations: Vec<Annotation>,
    pub directives: OracleDirectives,
}

static ANNOTATION_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"//#\s+(ERROR|WARN|RUSTC\s+OK|RUSTC\s+ERROR)\s*(.*)").unwrap());

pub fn parse_annotations(source: &str) -> ParsedAnnotations {
    let mut annotations = Vec::new();
    let mut directives = OracleDirectives::default();
    let lines: Vec<&str> = source.lines().collect();

    for (i, line) in lines.iter().enumerate() {
        if let Some(caps) = ANNOTATION_RE.captures(line) {
            let kind = caps.get(1).unwrap().as_str();
            let rest = caps.get(2).map_or("", |m| m.as_str()).trim().to_string();

            match kind {
                "ERROR" => {
                    let target_line = annotation_target_line(&lines, i);
                    annotations.push(Annotation {
                        line: target_line,
                        severity: ExpectedSeverity::Error,
                        pattern: rest,
                    });
                }
                "WARN" => {
                    let target_line = annotation_target_line(&lines, i);
                    annotations.push(Annotation {
                        line: target_line,
                        severity: ExpectedSeverity::Warn,
                        pattern: rest,
                    });
                }
                _ if kind.starts_with("RUSTC") && kind.contains("OK") => {
                    directives.rustc_ok = true;
                }
                _ if kind.starts_with("RUSTC") && kind.contains("ERROR") => {
                    directives.rustc_error = true;
                }
                _ => {}
            }
        }
    }

    ParsedAnnotations {
        annotations,
        directives,
    }
}

/// Determine the target line for an annotation.
///
/// - If the annotation appears after non-whitespace on the same line,
///   the target is that line.
/// - If the annotation is at the start of a line (possibly with leading whitespace),
///   the target is the nearest preceding non-annotation line.
fn annotation_target_line(lines: &[&str], annotation_idx: usize) -> usize {
    let line = lines[annotation_idx];
    let trimmed = line.trim_start();

    // Check if annotation appears after other content on the same line
    if !trimmed.starts_with("//#") {
        return annotation_idx + 1; // 1-based
    }

    // Annotation is at start of line — find the preceding non-annotation line
    for j in (0..annotation_idx).rev() {
        let prev = lines[j].trim_start();
        if !prev.starts_with("//#") {
            return j + 1; // 1-based
        }
    }

    // Fallback: first line (shouldn't normally happen)
    1
}

/// Convert a byte offset in source text to a 1-based line number.
pub fn offset_to_line(source: &str, offset: u32) -> usize {
    source[..offset as usize]
        .chars()
        .filter(|&c| c == '\n')
        .count()
        + 1
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_inline_annotation() {
        let source = "fn foo() -> i32 { \"hello\" } //# ERROR type mismatch\n";
        let parsed = parse_annotations(source);
        assert_eq!(parsed.annotations.len(), 1);
        assert_eq!(parsed.annotations[0].line, 1);
        assert_eq!(parsed.annotations[0].severity, ExpectedSeverity::Error);
        assert_eq!(parsed.annotations[0].pattern, "type mismatch");
    }

    #[test]
    fn parse_next_line_annotation() {
        let source = "fn foo() -> i32 { \"hello\" }\n//# ERROR type mismatch\n";
        let parsed = parse_annotations(source);
        assert_eq!(parsed.annotations.len(), 1);
        assert_eq!(parsed.annotations[0].line, 1);
    }

    #[test]
    fn parse_stacked_annotations() {
        let source = "fn foo() {}\n//# ERROR first\n//# ERROR second\n";
        let parsed = parse_annotations(source);
        assert_eq!(parsed.annotations.len(), 2);
        assert_eq!(parsed.annotations[0].line, 1);
        assert_eq!(parsed.annotations[1].line, 1);
    }

    #[test]
    fn ellipsis_matching() {
        let ann = Annotation {
            line: 1,
            severity: ExpectedSeverity::Error,
            pattern: "expected…got…".to_string(),
        };
        assert!(ann.matches_message("expected u32 got String"));
        assert!(!ann.matches_message("got u32 expected String"));
    }

    #[test]
    fn parse_directives() {
        let source = "//# RUSTC OK\nfn foo() {}\n//# ERROR something\n";
        let parsed = parse_annotations(source);
        assert!(parsed.directives.rustc_ok);
        assert!(!parsed.directives.rustc_error);
        assert_eq!(parsed.annotations.len(), 1);
    }
}
