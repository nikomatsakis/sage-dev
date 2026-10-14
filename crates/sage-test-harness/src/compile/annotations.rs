use std::collections::BTreeSet;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Kind {
    Type,
    Error,
    Warning,
}
#[derive(Clone, Debug)]
pub(super) struct Marker {
    pub file: String,
    pub line: usize,
    pub kind: Kind,
    pub active: bool,
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
        let (kind, active) = match words.as_slice() {
            ["ty"] => (Kind::Type, true),
            ["ERROR"] | ["ERROR", "if", "SAGE"] => (Kind::Error, true),
            ["WARN"] | ["WARN", "if", "SAGE"] => (Kind::Warning, true),
            ["ERROR", "if", "rustc"] => (Kind::Error, false),
            ["WARN", "if", "rustc"] => (Kind::Warning, false),
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
            active,
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
    #[sage::test(crates.sage_test_harness.compile_tests.coordinates)]
    fn inline_stacked_and_blank_lines_use_one_rule() {
        let markers = parse("src/lib.rs", "fn f() {\n    22; //# ty\n\n    //# ERROR\n    //# WARN\n    // ordinary comment\n    //# ERROR if rustc\n}").unwrap();
        assert_eq!(
            markers.iter().map(|m| m.line).collect::<Vec<_>>(),
            vec![2, 2, 2, 6]
        );
        assert!(!markers[3].active);
        assert!(
            parse("src/lib.rs", "fn f() {\t}")
                .unwrap_err()
                .contains("tabs")
        );
    }
    #[test]
    #[sage::test(crates.sage_test_harness.compile_tests.annotation_validation)]
    #[sage::test(crates.sage_test_harness.compile_tests.compiler_conditions)]
    fn only_real_line_comment_directives_are_interpreted() {
        let markers = parse(
            "src/lib.rs",
            r##"fn f() { let s = r#"//# invalid"#; /* //# invalid */ }"##,
        )
        .unwrap();
        assert!(markers.is_empty());
        for bad in [
            "//# ty",
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
