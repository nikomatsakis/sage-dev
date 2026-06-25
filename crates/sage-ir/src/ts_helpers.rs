//! Tree-sitter helpers shared between `lower` and `memmap::expand`.
//!
//! These small utilities avoid duplicating the same tree-sitter traversal
//! logic in multiple places. They're the only place outside of `lower.rs`
//! and `memmap/expand.rs` that interacts with tree-sitter nodes.

/// Extract the input-token text from a `macro_invocation` tree-sitter node.
///
/// Finds the invocation's `token_tree` child (the `(...)`, `[...]` or
/// `{...}` argument), strips the outer delimiter pair, and trims
/// whitespace. Returns the empty string if the structure doesn't match
/// (e.g., for a malformed invocation).
pub(crate) fn extract_macro_invocation_tokens(node: tree_sitter::Node<'_>, text: &str) -> String {
    let mut cursor = node.walk();
    node.children(&mut cursor)
        .find(|c| c.kind() == "token_tree")
        .map(|tt| {
            let raw = &text[tt.byte_range()];
            // Strip one outer delimiter pair of any kind.
            let inner = raw
                .strip_prefix('(')
                .and_then(|s| s.strip_suffix(')'))
                .or_else(|| raw.strip_prefix('[').and_then(|s| s.strip_suffix(']')))
                .or_else(|| raw.strip_prefix('{').and_then(|s| s.strip_suffix('}')))
                .unwrap_or(raw);
            inner.trim().to_owned()
        })
        .unwrap_or_default()
}
/// Extract the body tokens from a `macro_definition` tree-sitter node.
///
/// Only handles the trivial `() => { ... }` form: the first rule must have
/// an empty LHS pattern (no named children in the `token_tree_pattern`).
/// Returns the RHS with outer braces stripped and trimmed. Returns the empty
/// string if the structure doesn't match or the LHS is non-empty.
pub(crate) fn extract_macro_body_tokens(node: tree_sitter::Node<'_>, text: &str) -> String {
    let mut cursor = node.walk();
    let rule = match node
        .children(&mut cursor)
        .find(|c| c.kind() == "macro_rule")
    {
        Some(r) => r,
        None => return String::new(),
    };

    let lhs = match rule.child_by_field_name("left") {
        Some(l) => l,
        None => return String::new(),
    };

    if lhs.named_child_count() != 0 {
        return String::new();
    }

    rule.child_by_field_name("right")
        .map(|tt| {
            let raw = &text[tt.byte_range()];
            raw.strip_prefix('{')
                .and_then(|s| s.strip_suffix('}'))
                .unwrap_or(raw)
                .trim()
                .to_owned()
        })
        .unwrap_or_default()
}
