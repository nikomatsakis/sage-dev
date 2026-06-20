use sage_stash::{Ptr, Slice, Stash};

use crate::cst::paths::{Path, PathAnchor, PathAnchorKind, PathSegment};
use crate::cst::ty::TypeCst;
use crate::name::Name;
use crate::span::RelativeSpan;

use super::Parser;

fn alloc_relative<'db>(
    stash: &mut Stash,
    first: PathSegment<'db>,
    rest: &[PathSegment<'db>],
) -> Ptr<Path<'db>> {
    let slice = stash.alloc_slice(rest);
    stash.alloc(Path::Relative(first, slice))
}

fn alloc_anchored<'db>(
    stash: &mut Stash,
    anchor: PathAnchor<'db>,
    segs: &[PathSegment<'db>],
) -> Ptr<Path<'db>> {
    let slice = stash.alloc_slice(segs);
    stash.alloc(Path::Anchored(anchor, slice))
}

fn alloc_from_parts<'db>(
    stash: &mut Stash,
    anchor: Option<PathAnchor<'db>>,
    segments: &[PathSegment<'db>],
) -> Ptr<Path<'db>> {
    match anchor {
        Some(a) => alloc_anchored(stash, a, segments),
        None => {
            let (first, rest) = segments
                .split_first()
                .expect("relative path with no segments");
            alloc_relative(stash, *first, rest)
        }
    }
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
                let seg = PathSegment {
                    name,
                    type_args: stash.alloc_slice(&[]),
                    span,
                };
                alloc_relative(stash, seg, &[])
            }
            "scoped_type_identifier" => self.build_scoped_type_path(stash, node, item_start),
            "generic_type" => self.build_generic_type_path(stash, node, item_start),
            _ => {
                let name = Name::new(self.db, self.text[node.byte_range()].to_owned());
                let span = RelativeSpan {
                    start: node.start_byte() as u32 - item_start,
                    end: node.end_byte() as u32 - item_start,
                };
                let seg = PathSegment {
                    name,
                    type_args: stash.alloc_slice(&[]),
                    span,
                };
                alloc_relative(stash, seg, &[])
            }
        }
    }

    /// Build a flat `Path` from a tree-sitter node.
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
                let anchor = PathAnchor {
                    kind: PathAnchorKind::Self_,
                    span,
                };
                alloc_anchored(stash, anchor, &[])
            }
            "crate" => {
                let span = RelativeSpan {
                    start: node.start_byte() as u32 - item_start,
                    end: node.end_byte() as u32 - item_start,
                };
                let anchor = PathAnchor {
                    kind: PathAnchorKind::CurrentCrate,
                    span,
                };
                alloc_anchored(stash, anchor, &[])
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
                let anchor = PathAnchor {
                    kind: PathAnchorKind::Super(self_anchor),
                    span,
                };
                alloc_anchored(stash, anchor, &[])
            }
            "identifier" | "type_identifier" | "primitive_type" => {
                let name = Name::new(self.db, self.text[node.byte_range()].to_owned());
                let span = RelativeSpan {
                    start: node.start_byte() as u32 - item_start,
                    end: node.end_byte() as u32 - item_start,
                };
                let seg = PathSegment {
                    name,
                    type_args: stash.alloc_slice(&[]),
                    span,
                };
                alloc_relative(stash, seg, &[])
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
                let seg = PathSegment {
                    name,
                    type_args: stash.alloc_slice(&[]),
                    span,
                };
                alloc_relative(stash, seg, &[])
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
        let mut segments: Vec<PathSegment<'db>> = Vec::new();
        let mut anchor: Option<PathAnchor<'db>> = None;

        self.collect_scoped_parts(stash, node, item_start, &mut segments, &mut anchor);

        alloc_from_parts(stash, anchor, &segments)
    }

    /// Recursively collect segments and anchor from a scoped_identifier chain.
    fn collect_scoped_parts(
        &self,
        stash: &mut Stash,
        node: tree_sitter::Node<'a>,
        item_start: u32,
        segments: &mut Vec<PathSegment<'db>>,
        anchor: &mut Option<PathAnchor<'db>>,
    ) {
        match node.kind() {
            "scoped_identifier" => {
                match node.child_by_field_name("path") {
                    Some(path_node) => {
                        self.collect_scoped_parts(stash, path_node, item_start, segments, anchor);
                    }
                    None => {
                        // Leading `::` with no path field means extern crate.
                        if let Some(name_node) = node.child_by_field_name("name") {
                            let name =
                                Name::new(self.db, self.text[name_node.byte_range()].to_owned());
                            let span = RelativeSpan {
                                start: node.start_byte() as u32 - item_start,
                                end: name_node.end_byte() as u32 - item_start,
                            };
                            *anchor = Some(PathAnchor {
                                kind: PathAnchorKind::ExternCrate(name),
                                span,
                            });
                        }
                        return;
                    }
                }

                if let Some(name_node) = node.child_by_field_name("name") {
                    let text = &self.text[name_node.byte_range()];
                    let span = RelativeSpan {
                        start: name_node.start_byte() as u32 - item_start,
                        end: name_node.end_byte() as u32 - item_start,
                    };
                    if text == "super" {
                        // `super` in name position extends the anchor chain.
                        let inner = match anchor.take() {
                            Some(a) => stash.alloc(a),
                            None => stash.alloc(PathAnchor {
                                kind: PathAnchorKind::Self_,
                                span,
                            }),
                        };
                        *anchor = Some(PathAnchor {
                            kind: PathAnchorKind::Super(inner),
                            span,
                        });
                    } else {
                        let name = Name::new(self.db, text.to_owned());
                        segments.push(PathSegment {
                            name,
                            type_args: stash.alloc_slice(&[]),
                            span,
                        });
                    }
                }
            }
            "self" => {
                let span = RelativeSpan {
                    start: node.start_byte() as u32 - item_start,
                    end: node.end_byte() as u32 - item_start,
                };
                *anchor = Some(PathAnchor {
                    kind: PathAnchorKind::Self_,
                    span,
                });
            }
            "crate" => {
                let span = RelativeSpan {
                    start: node.start_byte() as u32 - item_start,
                    end: node.end_byte() as u32 - item_start,
                };
                *anchor = Some(PathAnchor {
                    kind: PathAnchorKind::CurrentCrate,
                    span,
                });
            }
            "super" => {
                let span = RelativeSpan {
                    start: node.start_byte() as u32 - item_start,
                    end: node.end_byte() as u32 - item_start,
                };
                let inner = match anchor.take() {
                    Some(a) => stash.alloc(a),
                    None => stash.alloc(PathAnchor {
                        kind: PathAnchorKind::Self_,
                        span,
                    }),
                };
                *anchor = Some(PathAnchor {
                    kind: PathAnchorKind::Super(inner),
                    span,
                });
            }
            "identifier" | "type_identifier" | "primitive_type" => {
                let name = Name::new(self.db, self.text[node.byte_range()].to_owned());
                let span = RelativeSpan {
                    start: node.start_byte() as u32 - item_start,
                    end: node.end_byte() as u32 - item_start,
                };
                segments.push(PathSegment {
                    name,
                    type_args: stash.alloc_slice(&[]),
                    span,
                });
            }
            _ => {
                let name = Name::new(self.db, self.text[node.byte_range()].to_owned());
                let span = RelativeSpan {
                    start: node.start_byte() as u32 - item_start,
                    end: node.end_byte() as u32 - item_start,
                };
                segments.push(PathSegment {
                    name,
                    type_args: stash.alloc_slice(&[]),
                    span,
                });
            }
        }
    }

    /// Build path from `scoped_type_identifier` (e.g. `Foo::Bar` in type position).
    fn build_scoped_type_path(
        &self,
        stash: &mut Stash,
        node: tree_sitter::Node<'a>,
        item_start: u32,
    ) -> Ptr<Path<'db>> {
        let mut segments: Vec<PathSegment<'db>> = Vec::new();
        let mut anchor: Option<PathAnchor<'db>> = None;

        if let Some(path_node) = node.child_by_field_name("path") {
            self.collect_scoped_parts(stash, path_node, item_start, &mut segments, &mut anchor);
        }

        if let Some(name_node) = node.child_by_field_name("name") {
            let name = Name::new(self.db, self.text[name_node.byte_range()].to_owned());
            let span = RelativeSpan {
                start: name_node.start_byte() as u32 - item_start,
                end: name_node.end_byte() as u32 - item_start,
            };
            segments.push(PathSegment {
                name,
                type_args: stash.alloc_slice(&[]),
                span,
            });
        }

        alloc_from_parts(stash, anchor, &segments)
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
                    let seg = PathSegment {
                        name,
                        type_args,
                        span,
                    };
                    alloc_relative(stash, seg, &[])
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
                    let seg = PathSegment {
                        name,
                        type_args: stash.alloc_slice(&[]),
                        span,
                    };
                    alloc_relative(stash, seg, &[])
                }
            }
        } else {
            unreachable!("generic_type node without a type child")
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
            Path::Relative(mut first, rest_slice) => {
                let rest = &stash[rest_slice];
                if rest.is_empty() {
                    first.type_args = type_args;
                    first.span.end = generic_node.end_byte() as u32 - item_start;
                    alloc_relative(stash, first, &[])
                } else {
                    let mut new_rest: Vec<_> = rest.to_vec();
                    let last = new_rest.last_mut().unwrap();
                    last.type_args = type_args;
                    last.span.end = generic_node.end_byte() as u32 - item_start;
                    alloc_relative(stash, first, &new_rest)
                }
            }
            Path::Anchored(anchor, seg_slice) => {
                let segs = &stash[seg_slice];
                let mut new_segs: Vec<_> = segs.to_vec();
                let last = new_segs.last_mut().expect("anchored path with no segments");
                last.type_args = type_args;
                last.span.end = generic_node.end_byte() as u32 - item_start;
                alloc_anchored(stash, anchor, &new_segs)
            }
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
