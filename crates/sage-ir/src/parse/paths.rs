use sage_stash::{Ptr, Slice, Stash};

use crate::cst::paths::{Path, PathAnchor, PathAnchorKind, PathSegment};
use crate::cst::ty::TypeCst;
use crate::name::Name;
use crate::span::RelativeSpan;

use super::Parser;

/// Allocate a `Path::Segment` with no type args.
fn alloc_segment<'db>(
    stash: &mut Stash,
    name: Name<'db>,
    prefix: Ptr<Path<'db>>,
    span: RelativeSpan,
) -> Ptr<Path<'db>> {
    let type_args: Slice<TypeCst<'db>> = stash.alloc_slice(&[]);
    stash.alloc(Path::Segment(PathSegment {
        name,
        prefix,
        type_args,
        span,
    }))
}

impl<'a, 'db> Parser<'a, 'db> {
    pub(super) fn parse_path(
        &self,
        stash: &mut Stash,
        node: tree_sitter::Node<'a>,
        item_start: u32,
    ) -> Ptr<Path<'db>> {
        self.build_path(stash, node, item_start)
    }

    pub(super) fn parse_path_from_type_node(
        &self,
        stash: &mut Stash,
        node: tree_sitter::Node<'a>,
        item_start: u32,
    ) -> Ptr<Path<'db>> {
        match node.kind() {
            "type_identifier" | "primitive_type" | "identifier" => {
                let name = Name::new(self.db, self.text[node.byte_range()].to_owned());
                let span = RelativeSpan {
                    start: node.start_byte() as u32 - item_start,
                    end: node.end_byte() as u32 - item_start,
                };
                let prefix = stash.alloc(Path::Relative);
                alloc_segment(stash, name, prefix, span)
            }
            "scoped_type_identifier" => self.build_scoped_type_path(stash, node, item_start),
            "generic_type" => self.build_generic_type_path(stash, node, item_start),
            _ => {
                let name = Name::new(self.db, self.text[node.byte_range()].to_owned());
                let span = RelativeSpan {
                    start: node.start_byte() as u32 - item_start,
                    end: node.end_byte() as u32 - item_start,
                };
                let prefix = stash.alloc(Path::Relative);
                alloc_segment(stash, name, prefix, span)
            }
        }
    }

    /// Build a recursive `Path` from a tree-sitter node.
    fn build_path(
        &self,
        stash: &mut Stash,
        node: tree_sitter::Node<'a>,
        item_start: u32,
    ) -> Ptr<Path<'db>> {
        match node.kind() {
            "self" => {
                let span = RelativeSpan {
                    start: node.start_byte() as u32 - item_start,
                    end: node.end_byte() as u32 - item_start,
                };
                stash.alloc(Path::Anchor(PathAnchor {
                    kind: PathAnchorKind::Self_,
                    span,
                }))
            }
            "crate" => {
                let span = RelativeSpan {
                    start: node.start_byte() as u32 - item_start,
                    end: node.end_byte() as u32 - item_start,
                };
                stash.alloc(Path::Anchor(PathAnchor {
                    kind: PathAnchorKind::CurrentCrate,
                    span,
                }))
            }
            "super" => {
                let span = RelativeSpan {
                    start: node.start_byte() as u32 - item_start,
                    end: node.end_byte() as u32 - item_start,
                };
                let self_anchor = stash.alloc(PathAnchor {
                    kind: PathAnchorKind::Self_,
                    span,
                });
                stash.alloc(Path::Anchor(PathAnchor {
                    kind: PathAnchorKind::Super(self_anchor),
                    span,
                }))
            }
            "identifier" | "type_identifier" | "primitive_type" => {
                let name = Name::new(self.db, self.text[node.byte_range()].to_owned());
                let span = RelativeSpan {
                    start: node.start_byte() as u32 - item_start,
                    end: node.end_byte() as u32 - item_start,
                };
                let prefix = stash.alloc(Path::Relative);
                alloc_segment(stash, name, prefix, span)
            }
            "scoped_identifier" => self.build_scoped_identifier(stash, node, item_start),
            "scoped_type_identifier" => self.build_scoped_type_path(stash, node, item_start),
            "generic_type" => self.build_generic_type_path(stash, node, item_start),
            _ => {
                let name = Name::new(self.db, self.text[node.byte_range()].to_owned());
                let span = RelativeSpan {
                    start: node.start_byte() as u32 - item_start,
                    end: node.end_byte() as u32 - item_start,
                };
                let prefix = stash.alloc(Path::Relative);
                alloc_segment(stash, name, prefix, span)
            }
        }
    }

    /// Build path from `scoped_identifier` (e.g. `foo::bar`, `self::baz`, `::ext::thing`).
    fn build_scoped_identifier(
        &self,
        stash: &mut Stash,
        node: tree_sitter::Node<'a>,
        item_start: u32,
    ) -> Ptr<Path<'db>> {
        let prefix = match node.child_by_field_name("path") {
            Some(path_node) => self.build_path_prefix(stash, path_node, item_start),
            None => {
                // Leading `::` with no path field means extern crate.
                // The "name" field is the crate name.
                if let Some(name_node) = node.child_by_field_name("name") {
                    let name = Name::new(self.db, self.text[name_node.byte_range()].to_owned());
                    let span = RelativeSpan {
                        start: node.start_byte() as u32 - item_start,
                        end: name_node.end_byte() as u32 - item_start,
                    };
                    return stash.alloc(Path::Anchor(PathAnchor {
                        kind: PathAnchorKind::ExternCrate(name),
                        span,
                    }));
                }
                stash.alloc(Path::Relative)
            }
        };

        if let Some(name_node) = node.child_by_field_name("name") {
            let name = Name::new(self.db, self.text[name_node.byte_range()].to_owned());
            let span = RelativeSpan {
                start: name_node.start_byte() as u32 - item_start,
                end: name_node.end_byte() as u32 - item_start,
            };
            alloc_segment(stash, name, prefix, span)
        } else {
            prefix
        }
    }

    /// Build the prefix portion of a scoped path, handling `super` chains.
    fn build_path_prefix(
        &self,
        stash: &mut Stash,
        node: tree_sitter::Node<'a>,
        item_start: u32,
    ) -> Ptr<Path<'db>> {
        match node.kind() {
            "self" => {
                let span = RelativeSpan {
                    start: node.start_byte() as u32 - item_start,
                    end: node.end_byte() as u32 - item_start,
                };
                stash.alloc(Path::Anchor(PathAnchor {
                    kind: PathAnchorKind::Self_,
                    span,
                }))
            }
            "crate" => {
                let span = RelativeSpan {
                    start: node.start_byte() as u32 - item_start,
                    end: node.end_byte() as u32 - item_start,
                };
                stash.alloc(Path::Anchor(PathAnchor {
                    kind: PathAnchorKind::CurrentCrate,
                    span,
                }))
            }
            "super" => {
                let span = RelativeSpan {
                    start: node.start_byte() as u32 - item_start,
                    end: node.end_byte() as u32 - item_start,
                };
                let self_anchor = stash.alloc(PathAnchor {
                    kind: PathAnchorKind::Self_,
                    span,
                });
                stash.alloc(Path::Anchor(PathAnchor {
                    kind: PathAnchorKind::Super(self_anchor),
                    span,
                }))
            }
            "scoped_identifier" => {
                // Could be `super::super` chain or `self::super`.
                let inner_prefix = match node.child_by_field_name("path") {
                    Some(p) => self.build_path_prefix(stash, p, item_start),
                    None => stash.alloc(Path::Relative),
                };
                if let Some(name_node) = node.child_by_field_name("name") {
                    let text = &self.text[name_node.byte_range()];
                    let span = RelativeSpan {
                        start: name_node.start_byte() as u32 - item_start,
                        end: name_node.end_byte() as u32 - item_start,
                    };
                    if text == "super" {
                        // Nested super: wrap the inner prefix's anchor.
                        let inner_anchor = self.path_to_anchor(stash, inner_prefix, span);
                        return stash.alloc(Path::Anchor(PathAnchor {
                            kind: PathAnchorKind::Super(inner_anchor),
                            span,
                        }));
                    }
                    // Normal segment on top of the prefix.
                    let name = Name::new(self.db, text.to_owned());
                    alloc_segment(stash, name, inner_prefix, span)
                } else {
                    inner_prefix
                }
            }
            _ => self.build_path(stash, node, item_start),
        }
    }

    /// Extract or create a `PathAnchor` from a `Path` value.
    fn path_to_anchor(
        &self,
        stash: &mut Stash,
        path_ptr: Ptr<Path<'db>>,
        fallback_span: RelativeSpan,
    ) -> Ptr<PathAnchor<'db>> {
        let path = stash[path_ptr];
        match path {
            Path::Anchor(anchor) => stash.alloc(anchor),
            _ => stash.alloc(PathAnchor {
                kind: PathAnchorKind::Self_,
                span: fallback_span,
            }),
        }
    }

    /// Build path from `scoped_type_identifier` (e.g. `Foo::Bar` in type position).
    fn build_scoped_type_path(
        &self,
        stash: &mut Stash,
        node: tree_sitter::Node<'a>,
        item_start: u32,
    ) -> Ptr<Path<'db>> {
        let prefix = match node.child_by_field_name("path") {
            Some(path_node) => self.build_path(stash, path_node, item_start),
            None => stash.alloc(Path::Relative),
        };
        if let Some(name_node) = node.child_by_field_name("name") {
            let name = Name::new(self.db, self.text[name_node.byte_range()].to_owned());
            let span = RelativeSpan {
                start: name_node.start_byte() as u32 - item_start,
                end: name_node.end_byte() as u32 - item_start,
            };
            alloc_segment(stash, name, prefix, span)
        } else {
            prefix
        }
    }

    /// Build path from `generic_type` (e.g. `Vec<i32>`, `Foo::Bar<T>`).
    fn build_generic_type_path(
        &self,
        stash: &mut Stash,
        node: tree_sitter::Node<'a>,
        item_start: u32,
    ) -> Ptr<Path<'db>> {
        let type_args = self.parse_type_arguments(stash, node, item_start);

        if let Some(type_node) = node.child_by_field_name("type") {
            match type_node.kind() {
                "type_identifier" | "identifier" => {
                    let name = Name::new(self.db, self.text[type_node.byte_range()].to_owned());
                    let span = RelativeSpan {
                        start: type_node.start_byte() as u32 - item_start,
                        end: node.end_byte() as u32 - item_start,
                    };
                    let prefix = stash.alloc(Path::Relative);
                    stash.alloc(Path::Segment(PathSegment {
                        name,
                        prefix,
                        type_args,
                        span,
                    }))
                }
                "scoped_type_identifier" => {
                    let path_ptr = self.build_scoped_type_path(stash, type_node, item_start);
                    self.patch_type_args(stash, path_ptr, type_args, node, item_start)
                }
                _ => {
                    let name = Name::new(self.db, self.text[type_node.byte_range()].to_owned());
                    let span = RelativeSpan {
                        start: type_node.start_byte() as u32 - item_start,
                        end: node.end_byte() as u32 - item_start,
                    };
                    let prefix = stash.alloc(Path::Relative);
                    alloc_segment(stash, name, prefix, span)
                }
            }
        } else {
            stash.alloc(Path::Relative)
        }
    }

    /// Replace the type_args on the final segment of a path.
    fn patch_type_args(
        &self,
        stash: &mut Stash,
        path_ptr: Ptr<Path<'db>>,
        type_args: Slice<TypeCst<'db>>,
        generic_node: tree_sitter::Node<'a>,
        item_start: u32,
    ) -> Ptr<Path<'db>> {
        let path = stash[path_ptr];
        match path {
            Path::Segment(seg) => {
                let span = RelativeSpan {
                    start: seg.span.start,
                    end: generic_node.end_byte() as u32 - item_start,
                };
                stash.alloc(Path::Segment(PathSegment {
                    name: seg.name,
                    prefix: seg.prefix,
                    type_args,
                    span,
                }))
            }
            _ => path_ptr,
        }
    }

    fn parse_type_arguments(
        &self,
        stash: &mut Stash,
        node: tree_sitter::Node<'a>,
        item_start: u32,
    ) -> Slice<TypeCst<'db>> {
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
