// {anchor}`crates.sage_test_harness.compile_tests.project_isolation`
// {anchor}`crates.sage_test_harness.compile_tests.fixtures`
use sage_stash::{Slice, Stash};

use crate::cst::attrs::{AttrCst, AttrCstKind};
use crate::cst::paths::Path;
use crate::span::RelativeSpan;

use super::Parser;

impl<'a, 'db> Parser<'a, 'db> {
    /// Evaluate configuration before constructing item identities. Unknown or
    /// malformed predicates stay visible as unsupported attributes.
    pub(super) fn cfg_enabled(&self, attrs: &[tree_sitter::Node<'a>]) -> bool {
        attrs
            .iter()
            .all(|attr| self.cfg_value(*attr) != Some(false))
    }

    fn cfg_value(&self, node: tree_sitter::Node<'a>) -> Option<bool> {
        let attribute = node.named_child(0)?;
        let name = attribute.named_child(0)?;
        if &self.text[name.byte_range()] != "cfg" {
            return None;
        }
        let arguments = attribute.child_by_field_name("arguments")?;
        let text = &self.text[arguments.byte_range()];
        evaluate_cfg(
            text.strip_prefix('(')?.strip_suffix(')')?,
            self.db.configuration().cfgs(self.db).as_ref()?,
        )
    }

    pub(super) fn associated_attr_supported(&self, node: tree_sitter::Node<'a>) -> bool {
        if self.cfg_value(node).is_some() {
            return true;
        }
        node.named_child(0)
            .and_then(|attr| attr.named_child(0))
            .is_some_and(|name| {
                crate::cst::attrs::is_known_inert_item_attribute(&self.text[name.byte_range()])
            })
    }

    pub(super) fn module_path(&self, node: tree_sitter::Node<'a>) -> Option<&'a str> {
        let attribute = node.named_child(0)?;
        let name = attribute.named_child(0)?;
        if &self.text[name.byte_range()] != "path" {
            return None;
        }
        let value = attribute.child_by_field_name("value")?;
        let text = &self.text[value.byte_range()];
        // Unsupported literal spellings remain active attributes and produce
        // an explicit incomplete-expansion diagnostic.
        let value = text.strip_prefix('"')?.strip_suffix('"')?;
        (!value.contains('\\')).then_some(value)
    }

    pub(super) fn inner_attr_is_known_inert(&self, node: tree_sitter::Node<'a>) -> bool {
        debug_assert_eq!(node.kind(), "inner_attribute_item");
        let attribute = node
            .named_child(0)
            .filter(|child| child.kind() == "attribute")
            .unwrap_or(node);
        let mut cursor = attribute.walk();
        let Some(path) = attribute
            .children(&mut cursor)
            .find(|child| matches!(child.kind(), "identifier" | "scoped_identifier"))
        else {
            return false;
        };
        path.kind() == "identifier"
            && crate::cst::attrs::is_known_inert_inner_attribute(&self.text[path.byte_range()])
    }

    pub(super) fn parse_attr_nodes(
        &self,
        stash: &mut Stash,
        nodes: &[tree_sitter::Node<'a>],
        item_start: u32,
    ) -> Slice<AttrCst<'db>> {
        if nodes.is_empty() {
            return stash.alloc_slice(&[]);
        }
        let attrs: Vec<AttrCst<'db>> = nodes
            .iter()
            .filter(|n| self.cfg_value(**n) != Some(true))
            .map(|n| self.parse_one_attr(stash, *n, item_start))
            .collect();
        stash.alloc_slice(&attrs)
    }

    fn parse_one_attr(
        &self,
        stash: &mut Stash,
        node: tree_sitter::Node<'a>,
        item_start: u32,
    ) -> AttrCst<'db> {
        let span = RelativeSpan {
            start: node.start_byte() as u32 - item_start,
            end: node.end_byte() as u32 - item_start,
        };

        // `attribute_item`/`inner_attribute_item` wrap the semantic
        // `attribute` node which owns the path and `arguments` field.
        let attribute = node
            .named_child(0)
            .filter(|child| child.kind() == "attribute")
            .unwrap_or(node);
        let mut path_node = None;
        let mut cursor = attribute.walk();

        for child in attribute.children(&mut cursor) {
            match child.kind() {
                "identifier" | "scoped_identifier" => {
                    path_node = Some(child);
                }
                _ => {}
            }
        }
        let args_node = attribute.child_by_field_name("arguments");

        let path = match path_node {
            Some(n) => self.parse_path(stash, n, item_start),
            None => {
                let name = crate::name::Name::new(self.db, String::new());
                let type_args = stash.alloc_slice(&[]);
                let rest = stash.alloc_slice(&[]);
                let seg = crate::cst::paths::PathSegment {
                    name,
                    type_args,
                    span,
                };
                stash.alloc(Path::Relative(seg, rest))
            }
        };

        let args = match args_node {
            Some(n) => {
                let bytes = self.text[n.byte_range()].as_bytes();
                stash.alloc_slice(bytes)
            }
            None => stash.alloc_slice(&[]),
        };

        AttrCst {
            kind: AttrCstKind::Normal,
            path,
            args,
            is_inner: node.kind() == "inner_attribute_item",
            span,
        }
    }
}

/// Parse the recursively nested Rust cfg predicate grammar without interpreting
/// arbitrary attribute text as configuration.
fn evaluate_cfg(text: &str, enabled: &[String]) -> Option<bool> {
    let text = text.trim();
    if let Some((operator, rest)) = text.split_once('(') {
        let rest = rest.strip_suffix(')')?;
        let mut depth = 0usize;
        let mut quoted = false;
        let mut escaped = false;
        let mut start = 0;
        let mut values = Vec::new();
        for (offset, character) in rest.char_indices() {
            if quoted {
                if escaped {
                    escaped = false;
                } else if character == '\\' {
                    escaped = true;
                } else if character == '"' {
                    quoted = false;
                }
                continue;
            }
            match character {
                '"' => quoted = true,
                '(' => depth += 1,
                ')' => depth = depth.checked_sub(1)?,
                ',' if depth == 0 => {
                    values.push(evaluate_cfg(&rest[start..offset], enabled)?);
                    start = offset + 1;
                }
                _ => {}
            }
        }
        if depth != 0 || quoted {
            return None;
        }
        if !rest[start..].trim().is_empty() {
            values.push(evaluate_cfg(&rest[start..], enabled)?);
        }
        return match operator.trim() {
            "all" => Some(values.iter().all(|v| *v)),
            "any" => Some(values.iter().any(|v| *v)),
            "not" if values.len() == 1 => Some(!values[0]),
            _ => None,
        };
    }
    let identifier = |s: &str| !s.is_empty() && s.chars().all(|c| c == '_' || c.is_alphanumeric());
    let key = if let Some((key, value)) = text.split_once('=') {
        let key = key.trim();
        let value = value.trim();
        if !identifier(key) || !value.starts_with('"') || !value.ends_with('"') {
            return None;
        }
        format!("{key}={value}")
    } else {
        if !identifier(text) {
            return None;
        }
        text.to_owned()
    };
    Some(enabled.contains(&key))
}
