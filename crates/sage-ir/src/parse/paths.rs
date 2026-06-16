use sage_stash::{Ptr, Stash};

use crate::cst::paths::{PathCst, PathSegmentCst};
use crate::name::Name;
use crate::span::RelativeSpan;

use super::Parser;

impl<'a, 'db> Parser<'a, 'db> {
    pub(super) fn parse_path(
        &self,
        stash: &mut Stash,
        node: tree_sitter::Node<'a>,
        item_start: u32,
    ) -> Ptr<PathCst<'db>> {
        let span = RelativeSpan {
            start: node.start_byte() as u32 - item_start,
            end: node.end_byte() as u32 - item_start,
        };
        let mut segments = Vec::new();
        self.collect_path_segments(stash, node, item_start, &mut segments);
        let seg_slice = stash.alloc_slice(&segments);
        stash.alloc(PathCst {
            segments: seg_slice,
            span,
        })
    }

    pub(super) fn parse_path_from_type_node(
        &self,
        stash: &mut Stash,
        node: tree_sitter::Node<'a>,
        item_start: u32,
    ) -> Ptr<PathCst<'db>> {
        let span = RelativeSpan {
            start: node.start_byte() as u32 - item_start,
            end: node.end_byte() as u32 - item_start,
        };

        let mut segments = Vec::new();
        match node.kind() {
            "type_identifier" | "primitive_type" | "identifier" => {
                let name = Name::new(self.db, self.text[node.byte_range()].to_owned());
                let seg_span = span;
                segments.push(PathSegmentCst {
                    name,
                    type_args: stash.alloc_slice(&[]),
                    span: seg_span,
                });
            }
            "scoped_type_identifier" => {
                self.collect_scoped_type_segments(stash, node, item_start, &mut segments);
            }
            "generic_type" => {
                self.collect_generic_type_segments(stash, node, item_start, &mut segments);
            }
            _ => {
                let name = Name::new(self.db, self.text[node.byte_range()].to_owned());
                segments.push(PathSegmentCst {
                    name,
                    type_args: stash.alloc_slice(&[]),
                    span,
                });
            }
        }

        let seg_slice = stash.alloc_slice(&segments);
        stash.alloc(PathCst {
            segments: seg_slice,
            span,
        })
    }

    fn collect_path_segments(
        &self,
        stash: &mut Stash,
        node: tree_sitter::Node<'a>,
        item_start: u32,
        out: &mut Vec<PathSegmentCst<'db>>,
    ) {
        match node.kind() {
            "identifier" | "type_identifier" | "primitive_type" | "self" | "crate" | "super" => {
                let name = Name::new(self.db, self.text[node.byte_range()].to_owned());
                let span = RelativeSpan {
                    start: node.start_byte() as u32 - item_start,
                    end: node.end_byte() as u32 - item_start,
                };
                out.push(PathSegmentCst {
                    name,
                    type_args: stash.alloc_slice(&[]),
                    span,
                });
            }
            "scoped_identifier" => {
                if let Some(path) = node.child_by_field_name("path") {
                    self.collect_path_segments(stash, path, item_start, out);
                }
                if let Some(name_node) = node.child_by_field_name("name") {
                    let name = Name::new(self.db, self.text[name_node.byte_range()].to_owned());
                    let span = RelativeSpan {
                        start: name_node.start_byte() as u32 - item_start,
                        end: name_node.end_byte() as u32 - item_start,
                    };
                    out.push(PathSegmentCst {
                        name,
                        type_args: stash.alloc_slice(&[]),
                        span,
                    });
                }
            }
            "scoped_type_identifier" => {
                self.collect_scoped_type_segments(stash, node, item_start, out);
            }
            "generic_type" => {
                self.collect_generic_type_segments(stash, node, item_start, out);
            }
            _ => {
                let name = Name::new(self.db, self.text[node.byte_range()].to_owned());
                let span = RelativeSpan {
                    start: node.start_byte() as u32 - item_start,
                    end: node.end_byte() as u32 - item_start,
                };
                out.push(PathSegmentCst {
                    name,
                    type_args: stash.alloc_slice(&[]),
                    span,
                });
            }
        }
    }

    fn collect_scoped_type_segments(
        &self,
        stash: &mut Stash,
        node: tree_sitter::Node<'a>,
        item_start: u32,
        out: &mut Vec<PathSegmentCst<'db>>,
    ) {
        if let Some(path) = node.child_by_field_name("path") {
            self.collect_path_segments(stash, path, item_start, out);
        }
        if let Some(name_node) = node.child_by_field_name("name") {
            let name = Name::new(self.db, self.text[name_node.byte_range()].to_owned());
            let span = RelativeSpan {
                start: name_node.start_byte() as u32 - item_start,
                end: name_node.end_byte() as u32 - item_start,
            };
            out.push(PathSegmentCst {
                name,
                type_args: stash.alloc_slice(&[]),
                span,
            });
        }
    }

    fn collect_generic_type_segments(
        &self,
        stash: &mut Stash,
        node: tree_sitter::Node<'a>,
        item_start: u32,
        out: &mut Vec<PathSegmentCst<'db>>,
    ) {
        if let Some(type_node) = node.child_by_field_name("type") {
            match type_node.kind() {
                "type_identifier" | "identifier" => {
                    let name = Name::new(self.db, self.text[type_node.byte_range()].to_owned());
                    let type_args = self.parse_type_arguments(stash, node, item_start);
                    let span = RelativeSpan {
                        start: type_node.start_byte() as u32 - item_start,
                        end: node.end_byte() as u32 - item_start,
                    };
                    out.push(PathSegmentCst {
                        name,
                        type_args,
                        span,
                    });
                }
                "scoped_type_identifier" => {
                    self.collect_scoped_type_segments(stash, type_node, item_start, out);
                    // Apply type args to last segment
                    if let Some(last) = out.last_mut() {
                        last.type_args = self.parse_type_arguments(stash, node, item_start);
                    }
                }
                _ => {
                    let name = Name::new(self.db, self.text[type_node.byte_range()].to_owned());
                    let span = RelativeSpan {
                        start: type_node.start_byte() as u32 - item_start,
                        end: node.end_byte() as u32 - item_start,
                    };
                    out.push(PathSegmentCst {
                        name,
                        type_args: stash.alloc_slice(&[]),
                        span,
                    });
                }
            }
        }
    }

    fn parse_type_arguments(
        &self,
        stash: &mut Stash,
        node: tree_sitter::Node<'a>,
        item_start: u32,
    ) -> sage_stash::Slice<crate::cst::ty::TypeCst<'db>> {
        let mut cursor = node.walk();
        let mut args = Vec::new();
        for child in node.children(&mut cursor) {
            if child.kind() == "type_arguments" {
                let mut inner_cursor = child.walk();
                for arg in child.children(&mut inner_cursor) {
                    if arg.is_named() && arg.kind() != "," && arg.kind() != "lifetime" {
                        let ty_ptr = self.parse_type(stash, arg, item_start);
                        args.push(stash[ty_ptr]);
                    }
                }
            }
        }
        stash.alloc_slice(&args)
    }
}
