// {anchor}`crates.sage_test_harness.compile_tests.resolved_symbols`
// {anchor}`crates.sage_test_harness.compile_tests.grouped_observations`
// {anchor}`crates.sage_test_harness.compile_tests.diagnostic_counts`
use std::collections::BTreeSet;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Kind {
    Type,
    Symbol,
    Error,
    Warning,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Compiler {
    Sage,
    Rustc,
}

#[derive(Clone, Debug)]
pub(super) struct Marker {
    pub file: String,
    pub line: usize,
    pub kind: Kind,
    pub compiler: Option<Compiler>,
}

impl Marker {
    pub fn applies_to(&self, compiler: Compiler) -> bool {
        self.compiler.is_none_or(|selected| selected == compiler)
    }
}

pub(super) fn tree(source: &str) -> tree_sitter::Tree {
    let mut parser = tree_sitter::Parser::new();
    parser
        .set_language(&tree_sitter_rust::LANGUAGE.into())
        .unwrap();
    parser.parse(source, None).unwrap()
}

// {anchor}`crates.sage_test_harness.compile_tests.coordinates`
// {anchor}`crates.sage_test_harness.compile_tests.annotation_validation`<<
pub(super) fn parse(file: &str, source: &str) -> Result<Vec<Marker>, String> {
    if source.contains('\t') {
        return Err(format!("{file}: tabs are not allowed"));
    }
    let tree = tree(source);
    let mut comments = Vec::new();
    fn walk(node: tree_sitter::Node<'_>, source: &str, comments: &mut Vec<(usize, usize, String)>) {
        if node.kind() == "line_comment" {
            let text = &source[node.byte_range()];
            if let Some(directive) = text.strip_prefix("//#") {
                comments.push((
                    node.start_position().row,
                    node.start_position().column,
                    directive.trim().into(),
                ));
            }
            return;
        }
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            walk(child, source, comments);
        }
    }
    walk(tree.root_node(), source, &mut comments);
    let lines: Vec<_> = source.split('\n').collect();
    let annotation_only: BTreeSet<_> = comments
        .iter()
        .filter_map(|(row, column, _)| lines[*row][..*column].trim().is_empty().then_some(*row))
        .collect();
    let mut result = Vec::new();
    for (row, column, directive) in comments {
        let words: Vec<_> = directive.split_whitespace().collect();
        // {anchor}`crates.sage_test_harness.compile_tests.compiler_conditions`
        let (kind, compiler) = match words.as_slice() {
            ["ty"] => (Kind::Type, Some(Compiler::Sage)),
            ["sym"] => (Kind::Symbol, Some(Compiler::Sage)),
            ["ERROR"] => (Kind::Error, None),
            ["ERROR", "if", "SAGE"] => (Kind::Error, Some(Compiler::Sage)),
            ["WARN"] => (Kind::Warning, None),
            ["WARN", "if", "SAGE"] => (Kind::Warning, Some(Compiler::Sage)),
            ["ERROR", "if", "rustc"] => (Kind::Error, Some(Compiler::Rustc)),
            ["WARN", "if", "rustc"] => (Kind::Warning, Some(Compiler::Rustc)),
            _ => {
                return Err(format!(
                    "{file}:{}: malformed directive: {directive}",
                    row + 1
                ));
            }
        };
        // {anchor}`crates.sage_test_harness.compile_tests.target_line`
        let target = if !lines[row][..column].trim().is_empty() {
            Some(row)
        } else {
            (0..row)
                .rev()
                .find(|line| !lines[*line].trim().is_empty() && !annotation_only.contains(line))
        };
        let target =
            target.ok_or_else(|| format!("{file}:{}: annotation has no target", row + 1))?;
        result.push(Marker {
            file: file.into(),
            line: target + 1,
            kind,
            compiler,
        });
    }
    Ok(result)
}
// >>

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    #[sage::test(crates.sage_test_harness.compile_tests.target_line)]
    fn all_directives_share_targets_and_reject_caret_and_range_syntax() {
        for directive in ["ty", "sym", "ERROR", "WARN"] {
            let source = format!(
                "fn f() {{\n    value; //# {directive}\n\n    //# {directive}\n    //# {directive}\n}}"
            );
            let markers = parse("src/lib.rs", &source).unwrap();
            assert_eq!(
                markers.iter().map(|marker| marker.line).collect::<Vec<_>>(),
                vec![2, 2, 2],
                "{directive}"
            );
            assert!(
                parse("src/lib.rs", &format!("\n  //# {directive}"))
                    .unwrap_err()
                    .contains("annotation has no target"),
                "{directive}"
            );
            for suffix in ["^", "1..2"] {
                assert!(
                    parse(
                        "src/lib.rs",
                        &format!("fn f() {{}} //# {directive} {suffix}")
                    )
                    .unwrap_err()
                    .contains("malformed directive"),
                    "{directive} {suffix}"
                );
            }
        }
    }
    #[test]
    #[sage::test(crates.sage_test_harness.compile_tests.target_line)]
    #[sage::test(crates.sage_test_harness.compile_tests.coordinates)]
    fn inline_stacked_and_blank_lines_use_one_rule() {
        let markers = parse("src/lib.rs", "fn f() {\n    22; //# ty\n\n    //# ERROR\n    //# WARN\n    // ordinary comment\n    //# ERROR if rustc\n}").unwrap();
        assert_eq!(
            markers.iter().map(|m| m.line).collect::<Vec<_>>(),
            vec![2, 2, 2, 6]
        );
        assert!(!markers[3].applies_to(Compiler::Sage));
        assert!(markers[3].applies_to(Compiler::Rustc));
        assert!(
            parse("src/lib.rs", "fn f() {\t}")
                .unwrap_err()
                .contains("tabs")
        );
    }
    #[test]
    #[sage::test(crates.sage_test_harness.compile_tests.annotation_validation)]
    #[sage::test(crates.sage_test_harness.compile_tests.compiler_conditions)]
    #[sage::test(crates.sage_test_harness.compile_tests.target_line)]
    fn only_real_line_comment_directives_are_interpreted() {
        let markers = parse(
            "src/lib.rs",
            r##"fn f() { let s = r#"//# invalid"#; /* //# invalid */ }"##,
        )
        .unwrap();
        assert!(markers.is_empty());
        for bad in [
            "//# ty",
            "//# sym",
            "fn f() {} //# sym[slug]",
            "fn f() {} //# sym if SAGE",
            "fn f() {} //# ERROR[SAGE]",
            "fn f() {} //# ERROR if sage",
            "fn f() {} //# ty[slug]",
            "fn f() {} //# WARN if other",
        ] {
            assert!(parse("src/lib.rs", bad).is_err(), "{bad}");
        }
        assert_eq!(
            parse("src/lib.rs", "fn café() {} //# ty").unwrap()[0].line,
            1
        );
    }
}
