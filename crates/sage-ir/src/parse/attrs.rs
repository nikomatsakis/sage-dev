use sage_stash::{Slice, Stash};

use crate::cst::attrs::{AttrCst, AttrCstKind};
use crate::cst::paths::Path;
use crate::span::RelativeSpan;

use super::Parser;

impl<'a, 'db> Parser<'a, 'db> {
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
