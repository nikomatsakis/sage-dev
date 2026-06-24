use sage_stash::{Slice, Stash};

use crate::cst::attrs::{AttrCst, AttrCstKind};
use crate::cst::paths::Path;
use crate::span::RelativeSpan;

use super::Parser;

impl<'a, 'db> Parser<'a, 'db> {
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

        let mut path_node = None;
        let mut args_node = None;
        let mut cursor = node.walk();

        for child in node.children(&mut cursor) {
            match child.kind() {
                "identifier" | "scoped_identifier" => {
                    path_node = Some(child);
                }
                "token_tree" => {
                    args_node = Some(child);
                }
                _ => {}
            }
        }

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
            is_inner: false,
            span,
        }
    }
}
