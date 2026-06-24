use sage_stash::{Slice, Stash};

use crate::cst::generics::{GenericParamCst, TypeBoundCst};
use crate::cst::where_clause::WhereClauseCst;
use crate::name::Name;
use crate::span::RelativeSpan;

use super::Parser;

impl<'a, 'db> Parser<'a, 'db> {
    pub(super) fn parse_generics(
        &self,
        stash: &mut Stash,
        node: tree_sitter::Node<'a>,
        item_start: u32,
    ) -> Slice<GenericParamCst<'db>> {
        let type_params = node.child_by_field_name("type_parameters");
        let tp_node = match type_params {
            Some(n) => n,
            None => return stash.alloc_slice(&[]),
        };

        let mut params = Vec::new();
        let mut cursor = tp_node.walk();

        for child in tp_node.children(&mut cursor) {
            match child.kind() {
                "type_identifier" | "identifier" => {
                    let name = Name::new(self.db, self.text[child.byte_range()].to_owned());
                    let span = RelativeSpan {
                        start: child.start_byte() as u32 - item_start,
                        end: child.end_byte() as u32 - item_start,
                    };
                    params.push(GenericParamCst::Type {
                        name,
                        bounds: stash.alloc_slice(&[]),
                        span,
                    });
                }
                "constrained_type_parameter" => {
                    params.push(self.parse_constrained_type_param(stash, child, item_start));
                }
                "lifetime" => {
                    let text = &self.text[child.byte_range()];
                    let name = Name::new(self.db, text.to_owned());
                    let span = RelativeSpan {
                        start: child.start_byte() as u32 - item_start,
                        end: child.end_byte() as u32 - item_start,
                    };
                    params.push(GenericParamCst::Lifetime { name, span });
                }
                "const_parameter" => {
                    params.push(self.parse_const_param(stash, child, item_start));
                }
                _ => {}
            }
        }

        stash.alloc_slice(&params)
    }

    fn parse_constrained_type_param(
        &self,
        stash: &mut Stash,
        node: tree_sitter::Node<'a>,
        item_start: u32,
    ) -> GenericParamCst<'db> {
        let span = RelativeSpan {
            start: node.start_byte() as u32 - item_start,
            end: node.end_byte() as u32 - item_start,
        };

        let name = node
            .child_by_field_name("left")
            .map(|n| Name::new(self.db, self.text[n.byte_range()].to_owned()))
            .unwrap_or_else(|| Name::new(self.db, "_".to_owned()));

        let mut bounds = Vec::new();
        if let Some(bound_node) = node.child_by_field_name("bounds") {
            self.collect_type_bounds(stash, bound_node, item_start, &mut bounds);
        }

        GenericParamCst::Type {
            name,
            bounds: stash.alloc_slice(&bounds),
            span,
        }
    }

    fn parse_const_param(
        &self,
        stash: &mut Stash,
        node: tree_sitter::Node<'a>,
        item_start: u32,
    ) -> GenericParamCst<'db> {
        let span = RelativeSpan {
            start: node.start_byte() as u32 - item_start,
            end: node.end_byte() as u32 - item_start,
        };

        let name = node
            .child_by_field_name("name")
            .map(|n| Name::new(self.db, self.text[n.byte_range()].to_owned()))
            .unwrap_or_else(|| Name::new(self.db, "_".to_owned()));

        let ty = node
            .child_by_field_name("type")
            .map(|n| self.parse_type(stash, n, item_start))
            .unwrap_or_else(|| {
                let err = crate::cst::ty::TypeCst {
                    kind: crate::cst::ty::TypeCstKind::Error,
                    span,
                };
                stash.alloc(err)
            });

        GenericParamCst::Const { name, ty, span }
    }

    fn collect_type_bounds(
        &self,
        stash: &mut Stash,
        node: tree_sitter::Node<'a>,
        item_start: u32,
        out: &mut Vec<TypeBoundCst<'db>>,
    ) {
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            match child.kind() {
                "type_identifier" | "scoped_type_identifier" | "generic_type" => {
                    let path = self.parse_path_from_type_node(stash, child, item_start);
                    out.push(TypeBoundCst::Trait(path));
                }
                "lifetime" => {
                    let name = Name::new(self.db, self.text[child.byte_range()].to_owned());
                    out.push(TypeBoundCst::Lifetime(name));
                }
                "trait_bound" => {
                    let mut inner_cursor = child.walk();
                    for inner in child.children(&mut inner_cursor) {
                        if inner.is_named() && inner.kind() != "?" {
                            let path = self.parse_path_from_type_node(stash, inner, item_start);
                            out.push(TypeBoundCst::Trait(path));
                            break;
                        }
                    }
                }
                _ => {}
            }
        }
    }

    pub(super) fn parse_where_clauses(
        &self,
        stash: &mut Stash,
        node: tree_sitter::Node<'a>,
        item_start: u32,
    ) -> Slice<WhereClauseCst<'db>> {
        let where_node = node
            .children(&mut node.walk())
            .find(|c| c.kind() == "where_clause");
        let wn = match where_node {
            Some(n) => n,
            None => return stash.alloc_slice(&[]),
        };

        let mut clauses = Vec::new();
        let mut cursor = wn.walk();
        for child in wn.children(&mut cursor) {
            if child.kind() == "where_predicate" {
                if let Some(clause) = self.parse_where_predicate(stash, child, item_start) {
                    clauses.push(clause);
                }
            }
        }
        stash.alloc_slice(&clauses)
    }

    fn parse_where_predicate(
        &self,
        stash: &mut Stash,
        node: tree_sitter::Node<'a>,
        item_start: u32,
    ) -> Option<WhereClauseCst<'db>> {
        let span = RelativeSpan {
            start: node.start_byte() as u32 - item_start,
            end: node.end_byte() as u32 - item_start,
        };

        let left = node.child_by_field_name("left")?;
        let subject = self.parse_type(stash, left, item_start);

        let mut bounds = Vec::new();
        if let Some(bound_node) = node.child_by_field_name("bounds") {
            self.collect_type_bounds(stash, bound_node, item_start, &mut bounds);
        }
        let bounds_slice = stash.alloc_slice(&bounds);

        Some(WhereClauseCst {
            subject,
            bounds: bounds_slice,
            span,
        })
    }
}
