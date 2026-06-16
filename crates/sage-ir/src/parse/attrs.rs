use sage_stash::{Slice, Stash};

use crate::cst::attrs::{AttrCst, AttrCstKind};
use crate::name::Name;
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

        let mut path_segments: Vec<Name<'db>> = Vec::new();
        let mut args = None;
        let mut cursor = node.walk();

        for child in node.children(&mut cursor) {
            match child.kind() {
                "identifier" | "scoped_identifier" => {
                    self.collect_attr_path(child, &mut path_segments);
                }
                "token_tree" => {
                    let raw = &self.text[child.byte_range()];
                    let inner = raw
                        .strip_prefix('(')
                        .and_then(|s| s.strip_suffix(')'))
                        .or_else(|| raw.strip_prefix('[').and_then(|s| s.strip_suffix(']')))
                        .or_else(|| raw.strip_prefix('{').and_then(|s| s.strip_suffix('}')))
                        .unwrap_or(raw);
                    args = Some(Name::new(self.db, inner.trim().to_owned()));
                }
                _ => {}
            }
        }

        let path = stash.alloc_slice(&path_segments);

        AttrCst {
            kind: AttrCstKind::Normal,
            path,
            args,
            is_inner: false,
            span,
        }
    }

    fn collect_attr_path(&self, node: tree_sitter::Node<'a>, out: &mut Vec<Name<'db>>) {
        match node.kind() {
            "identifier" => {
                out.push(Name::new(self.db, self.text[node.byte_range()].to_owned()));
            }
            "scoped_identifier" => {
                if let Some(path) = node.child_by_field_name("path") {
                    self.collect_attr_path(path, out);
                }
                if let Some(name) = node.child_by_field_name("name") {
                    out.push(Name::new(self.db, self.text[name.byte_range()].to_owned()));
                }
            }
            _ => {
                out.push(Name::new(self.db, self.text[node.byte_range()].to_owned()));
            }
        }
    }
}
